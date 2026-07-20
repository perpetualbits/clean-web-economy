//! In-memory work index (design §5–6): resolve, search, trending, persistence.
//!
//! Backed by plain maps for O(1) resolution and a linear scan for search — ample
//! for a devnet MVP node. The whole index serialises to a JSON snapshot so it
//! survives restarts.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use alloy::primitives::Address;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

use crate::manifest::{WorkManifest, WorkType};

/// A compact listing entry returned by search/trending/creator endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub work_id: Bytes32,
    pub fingerprint: String,
    pub title: String,
    pub work_type: WorkType,
    pub tags: Vec<String>,
    pub price_per_min: u64,
}

/// The payload returned by `GET /resolve/:fingerprint` (the extension seam).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub work_id: Bytes32,
    pub price_per_min: u64,
    pub region: Bytes32,
    pub work_type: WorkType,
}

/// Errors from mutating the index.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IndexError {
    /// A different work already claims this fingerprint.
    #[error("fingerprint {fingerprint} is already registered to another work")]
    DuplicateFingerprint { fingerprint: String },
}

/// The in-memory index. `works` is keyed by work id; the fingerprint map is a
/// secondary lookup kept in sync on every upsert.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    /// work_id -> manifest.
    works: BTreeMap<Bytes32, WorkManifest>,
    /// fingerprint -> work_id (secondary index for O(1) resolution).
    by_fingerprint: BTreeMap<String, Bytes32>,
}

impl Index {
    /// Create an empty index.
    pub fn new() -> Index {
        Index::default()
    }

    /// Number of indexed works.
    pub fn len(&self) -> usize {
        self.works.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.works.is_empty()
    }

    /// Insert or update a work. A work updates in place by `work_id`; a *different*
    /// work claiming an existing fingerprint is rejected (the duplicate guard).
    pub fn upsert(&mut self, m: WorkManifest) -> Result<(), IndexError> {
        // Reject if this fingerprint already points at a different work.
        if let Some(existing) = self.by_fingerprint.get(&m.fingerprint) {
            if existing != &m.work_id {
                return Err(IndexError::DuplicateFingerprint {
                    fingerprint: m.fingerprint,
                });
            }
        }
        // If this work previously had a different fingerprint, drop the stale entry.
        if let Some(prev) = self.works.get(&m.work_id) {
            if prev.fingerprint != m.fingerprint {
                self.by_fingerprint.remove(&prev.fingerprint);
            }
        }
        self.by_fingerprint.insert(m.fingerprint.clone(), m.work_id);
        self.works.insert(m.work_id, m);
        Ok(())
    }

    /// Resolve a fingerprint to its work's payout-relevant fields.
    pub fn resolve(&self, fingerprint: &str) -> Option<Resolved> {
        let work_id = self.by_fingerprint.get(fingerprint)?;
        let m = self.works.get(work_id)?;
        Some(Resolved {
            work_id: m.work_id,
            price_per_min: m.price_per_min,
            region: m.region,
            work_type: m.work_type,
        })
    }

    /// The full manifest for a work id.
    pub fn manifest(&self, work_id: &Bytes32) -> Option<&WorkManifest> {
        self.works.get(work_id)
    }

    /// All of a creator's works, as summaries.
    pub fn by_creator(&self, creator: &Address) -> Vec<Summary> {
        self.works
            .values()
            .filter(|m| &m.creator_id == creator)
            .map(summary_of)
            .collect()
    }

    /// Ranked text search. Scores each work by query-token matches in title (x3),
    /// tags (x2), and description (x1); drops zero-score works; filters by type.
    /// Returns one page plus the total match count.
    pub fn search(
        &self,
        q: &str,
        work_type: Option<WorkType>,
        page: usize,
        page_size: usize,
    ) -> (Vec<Summary>, usize) {
        let tokens: Vec<String> = tokenize(q);
        // Score every matching work.
        let mut scored: Vec<(u32, &WorkManifest)> = self
            .works
            .values()
            .filter(|m| work_type.is_none_or(|t| m.work_type == t))
            .filter_map(|m| {
                let score = relevance(m, &tokens);
                if score > 0 {
                    Some((score, m))
                } else {
                    None
                }
            })
            .collect();
        // Highest score first; ties broken by work_id for determinism.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.work_id.cmp(&b.1.work_id)));

        let total = scored.len();
        // Take the requested 1-based page.
        let start = page.saturating_sub(1) * page_size;
        let results = scored
            .into_iter()
            .skip(start)
            .take(page_size)
            .map(|(_, m)| summary_of(m))
            .collect();
        (results, total)
    }

    /// Trending list, newest first (recency-only in the MVP; usage feed is future
    /// work). `now_secs` is accepted for the recency computation but ordering is by
    /// `created_at` descending, which is equivalent for the pure-recency formula.
    pub fn trending(
        &self,
        work_type: Option<WorkType>,
        _now_secs: u64,
        page_size: usize,
    ) -> Vec<Summary> {
        let mut items: Vec<&WorkManifest> = self
            .works
            .values()
            .filter(|m| work_type.is_none_or(|t| m.work_type == t))
            .collect();
        // Most recent first; ties broken by work_id for determinism.
        items.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then(a.work_id.cmp(&b.work_id))
        });
        items.into_iter().take(page_size).map(summary_of).collect()
    }

    /// Persist the whole index to a pretty JSON snapshot.
    pub fn save_snapshot(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    /// Load an index from a snapshot, or an empty index if the file is absent.
    pub fn load_snapshot(path: &Path) -> io::Result<Index> {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Index::new()),
            Err(e) => Err(e),
        }
    }
}

/// Build a `Summary` from a manifest.
fn summary_of(m: &WorkManifest) -> Summary {
    Summary {
        work_id: m.work_id,
        fingerprint: m.fingerprint.clone(),
        title: m.title.clone(),
        work_type: m.work_type,
        tags: m.tags.clone(),
        price_per_min: m.price_per_min,
    }
}

/// Lowercase and split a string into alphanumeric word tokens.
fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Weighted relevance of a manifest to a set of query tokens.
fn relevance(m: &WorkManifest, tokens: &[String]) -> u32 {
    let title = tokenize(&m.title);
    let desc = tokenize(&m.description);
    let tags: Vec<String> = m.tags.iter().flat_map(|t| tokenize(t)).collect();
    let mut score = 0u32;
    for tok in tokens {
        if title.contains(tok) {
            score += 3;
        }
        if tags.contains(tok) {
            score += 2;
        }
        if desc.contains(tok) {
            score += 1;
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::WorkType;

    fn manifest(wid: u8, fp: &str, title: &str, wt: WorkType, created: u64) -> WorkManifest {
        WorkManifest {
            work_id: Bytes32([wid; 32]),
            fingerprint: fp.to_string(),
            title: title.to_string(),
            description: String::new(),
            tags: vec!["music".to_string()],
            work_type: wt,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: Address::ZERO,
            created_at: created,
        }
    }

    /// A stored manifest resolves by its fingerprint.
    #[test]
    fn resolve_hit_and_miss() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:aa", "Song", WorkType::Audio, 10))
            .unwrap();
        assert_eq!(idx.resolve("fp:aa").unwrap().work_id, Bytes32([1; 32]));
        assert!(idx.resolve("fp:zz").is_none());
    }

    /// Re-registering the same work (same work_id) updates in place; a *different*
    /// work claiming an existing fingerprint is rejected.
    #[test]
    fn duplicate_fingerprint_guard() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:aa", "Song", WorkType::Audio, 10))
            .unwrap();
        // Same work_id, updated title — allowed.
        idx.upsert(manifest(1, "fp:aa", "Song v2", WorkType::Audio, 11))
            .unwrap();
        assert_eq!(idx.manifest(&Bytes32([1; 32])).unwrap().title, "Song v2");
        // Different work_id, same fingerprint — rejected.
        let err = idx.upsert(manifest(2, "fp:aa", "Stolen", WorkType::Audio, 12));
        assert!(matches!(err, Err(IndexError::DuplicateFingerprint { .. })));
    }

    /// Search matches title tokens, filters by type, and omits zero-score works.
    #[test]
    fn search_matches_and_filters() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:a", "Blue Ocean", WorkType::Audio, 10))
            .unwrap();
        idx.upsert(manifest(2, "fp:b", "Red Desert", WorkType::Video, 10))
            .unwrap();
        let (results, total) = idx.search("ocean", None, 1, 20);
        assert_eq!(total, 1);
        assert_eq!(results[0].work_id, Bytes32([1; 32]));
        // Type filter excludes the audio work.
        let (video, _) = idx.search("ocean", Some(WorkType::Video), 1, 20);
        assert!(video.is_empty());
    }

    /// Trending orders by recency (newer first).
    #[test]
    fn trending_orders_by_recency() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:a", "Old", WorkType::Audio, 100))
            .unwrap();
        idx.upsert(manifest(2, "fp:b", "New", WorkType::Audio, 200))
            .unwrap();
        let list = idx.trending(None, 300, 20);
        assert_eq!(list[0].work_id, Bytes32([2; 32])); // newer first
    }

    /// The index survives a snapshot save/load round-trip.
    #[test]
    fn snapshot_round_trip() {
        let dir = std::env::temp_dir().join("cwe_hub_test_snap");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("snap.json");
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:a", "Song", WorkType::Audio, 10))
            .unwrap();
        idx.save_snapshot(&path).unwrap();
        let loaded = Index::load_snapshot(&path).unwrap();
        assert_eq!(loaded.resolve("fp:a").unwrap().work_id, Bytes32([1; 32]));
    }

    /// Re-keying a work_id to a new fingerprint drops the stale fingerprint entry.
    #[test]
    fn upsert_rekey_drops_stale_fingerprint() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:aa", "Song", WorkType::Audio, 10))
            .unwrap();
        idx.upsert(manifest(1, "fp:bb", "Song", WorkType::Audio, 11))
            .unwrap();
        assert!(idx.resolve("fp:aa").is_none());
        assert_eq!(idx.resolve("fp:bb").unwrap().work_id, Bytes32([1; 32]));
    }

    /// Loading a snapshot from a path that doesn't exist yields an empty index.
    #[test]
    fn load_snapshot_missing_file_is_empty() {
        let dir = std::env::temp_dir().join("cwe_hub_test_snap_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("does_not_exist.json");
        let idx = Index::load_snapshot(&path).unwrap();
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
    }

    /// The second page of search results returns the remainder and the correct total.
    #[test]
    fn search_pagination_second_page() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:a", "Ocean Song", WorkType::Audio, 10))
            .unwrap();
        idx.upsert(manifest(2, "fp:b", "Ocean Waves", WorkType::Audio, 11))
            .unwrap();
        idx.upsert(manifest(3, "fp:c", "Ocean Breeze", WorkType::Audio, 12))
            .unwrap();
        let (results, total) = idx.search("ocean", None, 2, 2);
        assert_eq!(total, 3);
        assert_eq!(results.len(), 1);
    }

    /// Equal-relevance matches are ordered by ascending work_id, deterministically.
    #[test]
    fn search_ties_break_by_work_id_ascending() {
        let mut idx = Index::new();
        idx.upsert(manifest(2, "fp:b", "Ocean Song", WorkType::Audio, 10))
            .unwrap();
        idx.upsert(manifest(1, "fp:a", "Ocean Song", WorkType::Audio, 10))
            .unwrap();
        let (first, _) = idx.search("ocean", None, 1, 20);
        assert_eq!(first[0].work_id, Bytes32([1; 32]));
        assert_eq!(first[1].work_id, Bytes32([2; 32]));
        let (second, _) = idx.search("ocean", None, 1, 20);
        assert_eq!(second[0].work_id, first[0].work_id);
        assert_eq!(second[1].work_id, first[1].work_id);
    }
}
