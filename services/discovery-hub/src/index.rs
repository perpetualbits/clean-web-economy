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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Summary {
    #[schema(value_type = String)]
    pub work_id: Bytes32,
    pub fingerprint: String,
    pub title: String,
    pub work_type: WorkType,
    pub tags: Vec<String>,
    pub price_per_min: u64,
}

/// The payload returned by `GET /resolve/:fingerprint` (the extension seam).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Resolved {
    #[schema(value_type = String)]
    pub work_id: Bytes32,
    pub price_per_min: u64,
    #[schema(value_type = String)]
    pub region: Bytes32,
    pub work_type: WorkType,
}

/// Errors from mutating the index.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IndexError {
    /// A different work already claims this fingerprint.
    #[error("fingerprint {fingerprint} is already registered to another work")]
    DuplicateFingerprint { fingerprint: String },
    /// A different work already claims this content id.
    #[error("content id {content_id} is already registered to another work")]
    DuplicateContentId { content_id: Bytes32 },
    /// The manifest's fingerprint string is not a well-formed `cwe-fingerprint`.
    #[error("fingerprint does not parse: {0}")]
    BadFingerprint(#[from] cwe_fingerprint::FingerprintError),
}

/// The in-memory index. `works` is keyed by work id; the fingerprint and
/// content-id maps are secondary lookups kept in sync on every upsert.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    /// work_id -> manifest.
    works: BTreeMap<Bytes32, WorkManifest>,
    /// fingerprint -> work_id (secondary index for O(1) resolution).
    by_fingerprint: BTreeMap<String, Bytes32>,
    /// content_id -> work_id (Tier 1's authoritative, exact-match index).
    by_content: BTreeMap<Bytes32, Bytes32>,
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
    /// work claiming an existing fingerprint or content id is rejected (the
    /// duplicate guards). The fingerprint string must parse as a
    /// [`cwe_fingerprint::Fingerprint`] — malformed fingerprints never enter the
    /// index, so `nearest_fingerprint`'s scan can assume every stored fingerprint
    /// parses.
    pub fn upsert(&mut self, m: WorkManifest) -> Result<(), IndexError> {
        cwe_fingerprint::Fingerprint::parse(&m.fingerprint)?;
        // Reject if this fingerprint already points at a different work.
        if let Some(existing) = self.by_fingerprint.get(&m.fingerprint) {
            if existing != &m.work_id {
                return Err(IndexError::DuplicateFingerprint {
                    fingerprint: m.fingerprint,
                });
            }
        }
        // Reject if this content id already points at a different work (Tier 1
        // authoritative identity, design §3, must not be reassignable).
        if let Some(existing) = self.by_content.get(&m.content_id) {
            if existing != &m.work_id {
                return Err(IndexError::DuplicateContentId {
                    content_id: m.content_id,
                });
            }
        }
        // If this work previously had a different fingerprint/content id, drop
        // the stale secondary-index entries.
        if let Some(prev) = self.works.get(&m.work_id) {
            if prev.fingerprint != m.fingerprint {
                self.by_fingerprint.remove(&prev.fingerprint);
            }
            if prev.content_id != m.content_id {
                self.by_content.remove(&prev.content_id);
            }
        }
        self.by_fingerprint.insert(m.fingerprint.clone(), m.work_id);
        self.by_content.insert(m.content_id, m.work_id);
        self.works.insert(m.work_id, m);
        Ok(())
    }

    /// Resolve a fingerprint to its work's payout-relevant fields (exact string
    /// match against the secondary index).
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

    /// Resolve a content id to its work's payout-relevant fields — the Tier 1
    /// authoritative, signed-exact lookup (design §3): an exact `content_id`
    /// match always resolves to the registered owner, with no fuzzy matching.
    pub fn resolve_content(&self, content_id: &Bytes32) -> Option<Resolved> {
        let work_id = self.by_content.get(content_id)?;
        let m = self.works.get(work_id)?;
        Some(Resolved {
            work_id: m.work_id,
            price_per_min: m.price_per_min,
            region: m.region,
            work_type: m.work_type,
        })
    }

    /// Tier 2's cautious fallback (design §3.2/§6): find the best-scoring
    /// registered fingerprint within Hamming similarity of `fp`, above
    /// `threshold`. A plain linear scan over every indexed work — fine for the
    /// MVP's index size; a production index would use locality-sensitive
    /// hashing (LSH) for sub-linear lookup, left as future work.
    pub fn nearest_fingerprint(
        &self,
        fp: &cwe_fingerprint::Fingerprint,
        threshold: f64,
    ) -> Option<(Summary, f64)> {
        self.works
            .values()
            .filter_map(|m| {
                // Every stored fingerprint parsed successfully at upsert time.
                let candidate = cwe_fingerprint::Fingerprint::parse(&m.fingerprint).ok()?;
                let score = cwe_fingerprint::compare(fp, &candidate);
                (score > threshold).then(|| (summary_of(m), score))
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
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
    use cwe_fingerprint::Fingerprint;
    use std::f32::consts::PI;

    /// A well-formed `fp:<256 hex>` fingerprint string, distinguished by `byte`.
    fn fp_str(byte: u8) -> String {
        format!("fp:{}", hex::encode([byte; 128]))
    }

    fn manifest(wid: u8, fp: &str, title: &str, wt: WorkType, created: u64) -> WorkManifest {
        WorkManifest {
            work_id: Bytes32([wid; 32]),
            content_id: Bytes32([wid; 32]),
            fingerprint: fp.to_string(),
            title: title.to_string(),
            description: String::new(),
            tags: vec!["music".to_string()],
            work_type: wt,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: Address::ZERO,
            created_at: created,
            payees: vec![(Address::ZERO, 1_000_000)],
        }
    }

    /// Generate `secs` of a mono sine wave at `freq` Hz, amplitude `amp`, for
    /// building realistic fingerprints in tests (mirrors `cwe-fingerprint`'s own
    /// test fixture).
    fn tone(freq: f32, amp: f32, secs: f32, sr: u32) -> Vec<f32> {
        let n = (secs * sr as f32) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    /// A stored manifest resolves by its fingerprint.
    #[test]
    fn resolve_hit_and_miss() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, &fp_str(0xaa), "Song", WorkType::Audio, 10))
            .unwrap();
        assert_eq!(
            idx.resolve(&fp_str(0xaa)).unwrap().work_id,
            Bytes32([1; 32])
        );
        assert!(idx.resolve(&fp_str(0xff)).is_none());
    }

    /// Re-registering the same work (same work_id) updates in place; a *different*
    /// work claiming an existing fingerprint is rejected.
    #[test]
    fn duplicate_fingerprint_guard() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, &fp_str(0xaa), "Song", WorkType::Audio, 10))
            .unwrap();
        // Same work_id, updated title — allowed.
        idx.upsert(manifest(1, &fp_str(0xaa), "Song v2", WorkType::Audio, 11))
            .unwrap();
        assert_eq!(idx.manifest(&Bytes32([1; 32])).unwrap().title, "Song v2");
        // Different work_id, same fingerprint — rejected.
        let err = idx.upsert(manifest(2, &fp_str(0xaa), "Stolen", WorkType::Audio, 12));
        assert!(matches!(err, Err(IndexError::DuplicateFingerprint { .. })));
    }

    /// A manifest whose fingerprint string does not parse is rejected.
    #[test]
    fn upsert_rejects_unparsable_fingerprint() {
        let mut idx = Index::new();
        let err = idx.upsert(manifest(
            1,
            "not-a-fingerprint",
            "Song",
            WorkType::Audio,
            10,
        ));
        assert!(matches!(err, Err(IndexError::BadFingerprint(_))));
    }

    /// A different work claiming an already-registered content id is rejected,
    /// mirroring the fingerprint duplicate guard (content id is Tier 1's
    /// authoritative key, design §3).
    #[test]
    fn duplicate_content_id_guard() {
        let mut idx = Index::new();
        let mut a = manifest(1, &fp_str(1), "Song", WorkType::Audio, 10);
        a.content_id = Bytes32([0x42; 32]);
        idx.upsert(a).unwrap();
        let mut b = manifest(2, &fp_str(2), "Stolen", WorkType::Audio, 11);
        b.content_id = Bytes32([0x42; 32]);
        let err = idx.upsert(b);
        assert!(matches!(err, Err(IndexError::DuplicateContentId { .. })));
    }

    /// `resolve_content` is the exact, authoritative Tier 1 lookup by content id.
    #[test]
    fn resolve_content_hit_and_miss() {
        let mut idx = Index::new();
        let mut m = manifest(1, &fp_str(3), "Song", WorkType::Audio, 10);
        m.content_id = Bytes32([0x11; 32]);
        idx.upsert(m).unwrap();
        assert_eq!(
            idx.resolve_content(&Bytes32([0x11; 32])).unwrap().work_id,
            Bytes32([1; 32])
        );
        assert!(idx.resolve_content(&Bytes32([0x99; 32])).is_none());
    }

    /// A near-identical (volume-changed) fingerprint matches above the threshold;
    /// a distinct fingerprint does not (design §3.2/§6, Tier 2 fallback).
    #[test]
    fn nearest_fingerprint_matches_above_threshold_and_rejects_distinct() {
        let mut idx = Index::new();
        let loud = Fingerprint::compute(&tone(440.0, 0.9, 3.0, 11025), 11025);
        let mut m = manifest(1, &loud.to_string(), "Song", WorkType::Audio, 10);
        m.content_id = Bytes32([0x22; 32]);
        idx.upsert(m).unwrap();

        // Same tone, halved amplitude: gain-invariant, should score well above 0.85.
        let quiet = Fingerprint::compute(&tone(440.0, 0.45, 3.0, 11025), 11025);
        let (candidate, score) = idx.nearest_fingerprint(&quiet, 0.85).unwrap();
        assert_eq!(candidate.work_id, Bytes32([1; 32]));
        assert!(score > 0.85, "expected a near match, got {score}");

        // A distinct tone must not clear the bar.
        let distinct = Fingerprint::compute(&tone(1200.0, 0.9, 3.0, 11025), 11025);
        assert!(idx.nearest_fingerprint(&distinct, 0.85).is_none());
    }

    /// Search matches title tokens, filters by type, and omits zero-score works.
    #[test]
    fn search_matches_and_filters() {
        let mut idx = Index::new();
        idx.upsert(manifest(
            1,
            &fp_str(0xa1),
            "Blue Ocean",
            WorkType::Audio,
            10,
        ))
        .unwrap();
        idx.upsert(manifest(
            2,
            &fp_str(0xb1),
            "Red Desert",
            WorkType::Video,
            10,
        ))
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
        idx.upsert(manifest(1, &fp_str(0xa2), "Old", WorkType::Audio, 100))
            .unwrap();
        idx.upsert(manifest(2, &fp_str(0xb2), "New", WorkType::Audio, 200))
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
        idx.upsert(manifest(1, &fp_str(0xa3), "Song", WorkType::Audio, 10))
            .unwrap();
        idx.save_snapshot(&path).unwrap();
        let loaded = Index::load_snapshot(&path).unwrap();
        assert_eq!(
            loaded.resolve(&fp_str(0xa3)).unwrap().work_id,
            Bytes32([1; 32])
        );
    }

    /// Re-keying a work_id to a new fingerprint drops the stale fingerprint entry.
    #[test]
    fn upsert_rekey_drops_stale_fingerprint() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, &fp_str(0xaa), "Song", WorkType::Audio, 10))
            .unwrap();
        idx.upsert(manifest(1, &fp_str(0xbb), "Song", WorkType::Audio, 11))
            .unwrap();
        assert!(idx.resolve(&fp_str(0xaa)).is_none());
        assert_eq!(
            idx.resolve(&fp_str(0xbb)).unwrap().work_id,
            Bytes32([1; 32])
        );
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
        idx.upsert(manifest(
            1,
            &fp_str(0xa4),
            "Ocean Song",
            WorkType::Audio,
            10,
        ))
        .unwrap();
        idx.upsert(manifest(
            2,
            &fp_str(0xb4),
            "Ocean Waves",
            WorkType::Audio,
            11,
        ))
        .unwrap();
        idx.upsert(manifest(
            3,
            &fp_str(0xc4),
            "Ocean Breeze",
            WorkType::Audio,
            12,
        ))
        .unwrap();
        let (results, total) = idx.search("ocean", None, 2, 2);
        assert_eq!(total, 3);
        assert_eq!(results.len(), 1);
    }

    /// Equal-relevance matches are ordered by ascending work_id, deterministically.
    #[test]
    fn search_ties_break_by_work_id_ascending() {
        let mut idx = Index::new();
        idx.upsert(manifest(
            2,
            &fp_str(0xb5),
            "Ocean Song",
            WorkType::Audio,
            10,
        ))
        .unwrap();
        idx.upsert(manifest(
            1,
            &fp_str(0xa5),
            "Ocean Song",
            WorkType::Audio,
            10,
        ))
        .unwrap();
        let (first, _) = idx.search("ocean", None, 1, 20);
        assert_eq!(first[0].work_id, Bytes32([1; 32]));
        assert_eq!(first[1].work_id, Bytes32([2; 32]));
        let (second, _) = idx.search("ocean", None, 1, 20);
        assert_eq!(second[0].work_id, first[0].work_id);
        assert_eq!(second[1].work_id, first[1].work_id);
    }
}
