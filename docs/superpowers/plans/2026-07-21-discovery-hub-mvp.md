# Discovery Hub MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust/axum Discovery Hub service that indexes creator-signed,
chain-verified work manifests and serves privacy-preserving fingerprint resolution
and basic search, wired into the browser extension's `HubClient` seam.

**Architecture:** A single binary crate `services/discovery-hub` with focused
modules — `manifest` (signed, canonical manifests), `index` (in-memory store +
search), `chain` (alloy registry cross-checks), `api` (axum + OpenAPI). Ingest
verifies a manifest's signature against the on-chain `CWERegistry` registrant.
Reads are chain-independent. The extension gains a `NetworkedHubClient`.

**Tech Stack:** Rust, axum, tokio, alloy (signing + RPC), serde/serde_json,
serde_jcs (RFC 8785 canonical JSON), utoipa (OpenAPI), Foundry (contract getters),
Node/esbuild (extension), bash + cast (demo).

**Design doc:** `docs/superpowers/specs/2026-07-21-discovery-hub-mvp-design.md`

## Global Constraints

- **Language:** Rust everywhere, except the Solidity contracts (`chain/`) and the
  browser extension's JS shell.
- **Comments:** every function/method gets a doc comment; non-trivial lines get an
  inline comment only when it adds understanding (never noise).
- **No AI attribution anywhere:** not in code, comments, docs, commit messages
  (no `Co-Authored-By`/"Generated with" trailers), branch names, or any GitHub-
  visible text.
- **Quality gate (must stay green):** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  and `forge test` (in `chain/`).
- **Workflow:** work on branch `phase2-discovery-hub`; commit frequently; local is
  source of truth, GitHub is backup.
- **Reuse:** `cwe-fingerprint` for the `fp:` type, `cwe-wallet-zk` for `Bytes32`
  and `keccak256`, alloy for signing/recovery and registry RPC (as `cwe-settlement`
  does).

---

## Task 1: Registry getters (`registrantOf`, `regionRuleOf`)

The hub's ingest check needs the on-chain registrant and region, which `CWERegistry`
does not currently expose.

**Files:**
- Modify: `chain/contracts/CWERegistry.sol` (add two view functions)
- Modify: `chain/contracts/interfaces/ICWERegistry.sol` (declare them)
- Test: `chain/test/CWERegistry.t.sol` (add cases)

**Interfaces:**
- Produces (Solidity):
  - `function registrantOf(bytes32 workId) external view returns (address)`
  - `function regionRuleOf(bytes32 workId) external view returns (bytes32)`

- [ ] **Step 1: Write the failing tests**

Add to `chain/test/CWERegistry.t.sol` inside `CWERegistryTest`:

```solidity
    /// @notice The registrant and region are readable after registration.
    function test_getters_exposeRegistrantAndRegion() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        vm.prank(creator);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));

        assertEq(registry.registrantOf(WORK), creator);
        assertEq(registry.regionRuleOf(WORK), bytes32("EU"));
    }

    /// @notice An unregistered work reports the zero registrant.
    function test_registrantOf_unregisteredIsZero() public view {
        assertEq(registry.registrantOf(keccak256("nope")), address(0));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd chain && forge test --match-contract CWERegistryTest`
Expected: FAIL — `registrantOf` / `regionRuleOf` are not members of `CWERegistry`.

- [ ] **Step 3: Add the getters**

In `chain/contracts/CWERegistry.sol`, after `pricePerMinOf`:

```solidity
    /// @notice The address that first registered a work (its owner for updates).
    /// @param workId The work identifier.
    /// @return The registrant address, or the zero address if unregistered.
    function registrantOf(bytes32 workId) external view returns (address) {
        return _works[workId].registrant;
    }

    /// @notice The opaque region-rule tag a work was registered with.
    /// @param workId The work identifier.
    /// @return The regionRule bytes32 tag.
    function regionRuleOf(bytes32 workId) external view returns (bytes32) {
        return _works[workId].regionRule;
    }
```

Add matching declarations to `chain/contracts/interfaces/ICWERegistry.sol`:

```solidity
    /// @notice The address that first registered a work.
    /// @param workId The work identifier.
    /// @return The registrant address (zero if unregistered).
    function registrantOf(bytes32 workId) external view returns (address);

    /// @notice The opaque region-rule tag for a work.
    /// @param workId The work identifier.
    /// @return The regionRule tag.
    function regionRuleOf(bytes32 workId) external view returns (bytes32);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd chain && forge test --match-contract CWERegistryTest`
Expected: PASS (all CWERegistry tests, including the two new ones).

- [ ] **Step 5: Commit**

```bash
git add chain/contracts/CWERegistry.sol chain/contracts/interfaces/ICWERegistry.sol chain/test/CWERegistry.t.sol
git commit -m "Add registrantOf/regionRuleOf getters to CWERegistry"
```

---

## Task 2: Crate scaffold + `manifest` module

The signed, canonical manifest type shared by the hub and the signing CLI.

**Files:**
- Create: `services/discovery-hub/Cargo.toml`
- Create: `services/discovery-hub/src/lib.rs`
- Create: `services/discovery-hub/src/manifest.rs`
- Modify: `Cargo.toml` (workspace members + shared deps)

**Interfaces:**
- Produces (Rust, `cwe_discovery_hub::manifest`):
  - `enum WorkType { Audio, Video, Text }`
  - `struct WorkManifest { work_id: Bytes32, fingerprint: String, title: String, description: String, tags: Vec<String>, work_type: WorkType, price_per_min: u64, region: Bytes32, creator_id: Address, created_at: u64 }` (derives `Serialize, Deserialize, Clone, PartialEq`)
  - `fn WorkManifest::canonical_bytes(&self) -> Result<Vec<u8>, ManifestError>` — RFC 8785 JCS
  - `fn WorkManifest::recover_signer(&self, signature: &[u8]) -> Result<Address, ManifestError>` — EIP-191 recover over `canonical_bytes`
  - `enum ManifestError` (thiserror)
  - Re-export `alloy::primitives::Address`.

- [ ] **Step 1: Add the workspace member and shared deps**

In the root `Cargo.toml`, add to `members`:

```toml
    "services/discovery-hub", # Phase 2 — Discovery Hub service
```

In `[workspace.dependencies]`, add:

```toml
serde_jcs = "0.1"     # RFC 8785 canonical JSON for signable manifests
```

- [ ] **Step 2: Write the crate manifest**

Create `services/discovery-hub/Cargo.toml`:

```toml
# Phase 2 — the Discovery Hub service (fingerprint resolution + search).
[package]
name = "cwe-discovery-hub"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Privacy-preserving discovery hub for the Clean Web Economy"

[dependencies]
cwe-fingerprint = { path = "../../libs/fingerprint" }
cwe-wallet-zk = { path = "../../libs/wallet-zk" }
alloy = { version = "2", features = ["full"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
axum = "0.8"
utoipa = "5"
serde.workspace = true
serde_json.workspace = true
serde_jcs.workspace = true
thiserror.workspace = true
hex.workspace = true

[[bin]]
name = "cwe-hub"
path = "src/main.rs"

[[bin]]
name = "sign-manifest"
path = "src/bin/sign_manifest.rs"
```

- [ ] **Step 3: Write the failing test (manifest canonicalization + recovery)**

Create `services/discovery-hub/src/manifest.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    fn sample() -> WorkManifest {
        WorkManifest {
            work_id: Bytes32([0xAA; 32]),
            fingerprint: "fp:".to_string() + &"11".repeat(32),
            title: "Test Track".to_string(),
            description: "a demo".to_string(),
            tags: vec!["demo".to_string(), "audio".to_string()],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: Address::ZERO,
            created_at: 1_721_500_000,
        }
    }

    /// Canonical bytes are deterministic (RFC 8785 sorts keys).
    #[test]
    fn canonical_bytes_are_stable() {
        let m = sample();
        assert_eq!(m.canonical_bytes().unwrap(), m.canonical_bytes().unwrap());
    }

    /// A signature recovers to the signer's address; a tampered manifest does not.
    #[test]
    fn recover_matches_signer_and_rejects_tampering() {
        let signer = PrivateKeySigner::random();
        let m = sample();
        let sig = signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap();
        let sig_bytes = sig.as_bytes();

        assert_eq!(m.recover_signer(&sig_bytes).unwrap(), signer.address());

        let mut tampered = m.clone();
        tampered.price_per_min = 999; // change a field
        assert_ne!(tampered.recover_signer(&sig_bytes).unwrap(), signer.address());
    }
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p cwe-discovery-hub manifest`
Expected: FAIL to compile — `WorkManifest`, `WorkType`, etc. undefined.

- [ ] **Step 5: Implement the manifest module**

Prepend to `services/discovery-hub/src/manifest.rs`:

```rust
//! The signed, chain-anchored work manifest (design §3).
//!
//! A manifest carries a work's discovery metadata plus the on-chain fields the hub
//! re-verifies. It is signed by the creator over its RFC 8785 canonical JSON form,
//! so the hub can recover the signer and check it against the registry registrant.

use alloy::primitives::Address;
use alloy::signers::Signature;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

/// The modality of a work. Serialised lowercase (`"audio"`, `"video"`, `"text"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkType {
    Audio,
    Video,
    Text,
}

/// A work's public manifest. Field order here does not affect signing: the
/// canonical form sorts keys (RFC 8785).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkManifest {
    /// On-chain CWERegistry work id.
    pub work_id: Bytes32,
    /// The `fp:<hex>` fingerprint from `cwe-fingerprint`.
    pub fingerprint: String,
    /// Human-readable title (indexed for search).
    pub title: String,
    /// Longer description (indexed with lower weight).
    pub description: String,
    /// Free-form tags (indexed for search).
    pub tags: Vec<String>,
    /// Modality of the work.
    pub work_type: WorkType,
    /// Price per minute in ppm; MUST equal the on-chain value.
    pub price_per_min: u64,
    /// The on-chain regionRule tag; MUST equal the on-chain value.
    pub region: Bytes32,
    /// The creator's address; MUST equal the on-chain registrant.
    pub creator_id: Address,
    /// Client Unix seconds when authored (used only for recency).
    pub created_at: u64,
}

/// Errors from canonicalising or verifying a manifest.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// Canonical JSON serialisation failed.
    #[error("canonicalising manifest: {0}")]
    Canonical(String),
    /// The signature bytes were malformed.
    #[error("invalid signature bytes")]
    BadSignature,
    /// Address recovery from the signature failed.
    #[error("recovering signer: {0}")]
    Recover(String),
}

impl WorkManifest {
    /// The RFC 8785 canonical JSON bytes of this manifest — the exact bytes that
    /// are signed and verified. Because both the signer CLI and the hub call this,
    /// the encodings cannot drift.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        serde_jcs::to_vec(self).map_err(|e| ManifestError::Canonical(e.to_string()))
    }

    /// Recover the address that signed this manifest, given the 65-byte EIP-191
    /// signature over [`canonical_bytes`](Self::canonical_bytes).
    pub fn recover_signer(&self, signature: &[u8]) -> Result<Address, ManifestError> {
        // Parse the r||s||v signature bytes.
        let sig = Signature::try_from(signature).map_err(|_| ManifestError::BadSignature)?;
        let msg = self.canonical_bytes()?;
        // `recover_address_from_msg` applies the EIP-191 personal-sign prefix, so it
        // matches a signer that used `sign_message`.
        sig.recover_address_from_msg(&msg).map_err(|e| ManifestError::Recover(e.to_string()))
    }
}
```

Create `services/discovery-hub/src/lib.rs`:

```rust
//! Discovery Hub for the Clean Web Economy (Phase 2).
//!
//! Indexes creator-signed, chain-verified work manifests and serves
//! privacy-preserving fingerprint resolution and basic search.

#![forbid(unsafe_code)]

pub mod manifest;
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p cwe-discovery-hub manifest`
Expected: PASS (2 tests). Then `cargo fmt --all` and
`cargo clippy -p cwe-discovery-hub --all-targets -- -D warnings`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml services/discovery-hub/Cargo.toml services/discovery-hub/src/lib.rs services/discovery-hub/src/manifest.rs
git commit -m "Scaffold discovery-hub crate with signed manifest type"
```

---

## Task 3: `sign-manifest` CLI

A command-line tool for creators (and the demo) to produce a signed manifest.

**Files:**
- Create: `services/discovery-hub/src/bin/sign_manifest.rs`

**Interfaces:**
- Consumes: `manifest::WorkManifest`, `manifest::WorkType`.
- Produces: a binary `sign-manifest` that reads a manifest JSON on stdin and a key
  from `PRIVATE_KEY`, and prints `{"manifest": <canonical>, "signature": "0x…"}`.

- [ ] **Step 1: Write the CLI**

Create `services/discovery-hub/src/bin/sign_manifest.rs`:

```rust
//! `sign-manifest` — sign a work manifest for submission to the Discovery Hub.
//!
//! Reads a manifest JSON object on stdin, signs its canonical form with the key in
//! `PRIVATE_KEY`, and prints `{ "manifest": <manifest>, "signature": "0x<hex>" }`
//! ready to POST to `/manifests`.

use std::io::Read;
use std::process::ExitCode;

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use cwe_discovery_hub::manifest::WorkManifest;

/// Entry point: read stdin, sign, print, or fail with a clear message.
fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Read the manifest from stdin, sign it, and return the JSON envelope string.
fn run() -> Result<String, String> {
    // Load the signing key from the environment (devnet use only).
    let key = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set".to_string())?;
    let signer: PrivateKeySigner = key.parse().map_err(|_| "invalid PRIVATE_KEY".to_string())?;

    // Parse the manifest from stdin.
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|e| e.to_string())?;
    let manifest: WorkManifest = serde_json::from_str(&input).map_err(|e| e.to_string())?;

    // Sign the canonical bytes (EIP-191 personal-sign).
    let bytes = manifest.canonical_bytes().map_err(|e| e.to_string())?;
    let sig = signer.sign_message_sync(&bytes).map_err(|e| e.to_string())?;
    let signature = format!("0x{}", hex::encode(sig.as_bytes()));

    // Emit the POST-ready envelope.
    let envelope = serde_json::json!({ "manifest": manifest, "signature": signature });
    serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Build and smoke-test the CLI**

Run:
```bash
cargo build -p cwe-discovery-hub --bin sign-manifest
echo '{"work_id":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fingerprint":"fp:1111111111111111111111111111111111111111111111111111111111111111","title":"T","description":"d","tags":["x"],"work_type":"audio","price_per_min":1000000,"region":"0x0000000000000000000000000000000000000000000000000000000000000000","creator_id":"0x0000000000000000000000000000000000000000","created_at":1}' \
  | PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  ./target/debug/sign-manifest
```
Expected: prints a JSON object with `manifest` and a `signature` of the form `0x` + 130 hex chars.

- [ ] **Step 3: Commit**

```bash
git add services/discovery-hub/src/bin/sign_manifest.rs
git commit -m "Add sign-manifest CLI"
```

---

## Task 4: `index` module

The in-memory store: resolve, search, trending, duplicate guard, and JSON snapshot.

**Files:**
- Create: `services/discovery-hub/src/index.rs`
- Modify: `services/discovery-hub/src/lib.rs` (add `pub mod index;`)

**Interfaces:**
- Consumes: `manifest::{WorkManifest, WorkType}`, `Bytes32`.
- Produces (`cwe_discovery_hub::index`):
  - `struct Summary { work_id, fingerprint, title, work_type, tags, price_per_min }` (serde)
  - `struct Resolved { work_id: Bytes32, price_per_min: u64, region: Bytes32, work_type: WorkType }` (serde)
  - `struct Index` with:
    - `fn new() -> Index`
    - `fn upsert(&mut self, m: WorkManifest) -> Result<(), IndexError>` (duplicate-fingerprint guard)
    - `fn resolve(&self, fingerprint: &str) -> Option<Resolved>`
    - `fn search(&self, q: &str, work_type: Option<WorkType>, page: usize, page_size: usize) -> (Vec<Summary>, usize)`
    - `fn trending(&self, work_type: Option<WorkType>, now_secs: u64, page_size: usize) -> Vec<Summary>`
    - `fn manifest(&self, work_id: &Bytes32) -> Option<&WorkManifest>`
    - `fn by_creator(&self, creator: &Address) -> Vec<Summary>`
    - `fn len(&self) -> usize` / `fn is_empty(&self) -> bool`
    - `fn save_snapshot(&self, path: &Path) -> io::Result<()>` / `fn load_snapshot(path: &Path) -> io::Result<Index>`
  - `enum IndexError { DuplicateFingerprint { fingerprint: String } }`

- [ ] **Step 1: Write the failing tests**

Create `services/discovery-hub/src/index.rs` with the test module first:

```rust
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
        idx.upsert(manifest(1, "fp:aa", "Song", WorkType::Audio, 10)).unwrap();
        assert_eq!(idx.resolve("fp:aa").unwrap().work_id, Bytes32([1; 32]));
        assert!(idx.resolve("fp:zz").is_none());
    }

    /// Re-registering the same work (same work_id) updates in place; a *different*
    /// work claiming an existing fingerprint is rejected.
    #[test]
    fn duplicate_fingerprint_guard() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:aa", "Song", WorkType::Audio, 10)).unwrap();
        // Same work_id, updated title — allowed.
        idx.upsert(manifest(1, "fp:aa", "Song v2", WorkType::Audio, 11)).unwrap();
        assert_eq!(idx.manifest(&Bytes32([1; 32])).unwrap().title, "Song v2");
        // Different work_id, same fingerprint — rejected.
        let err = idx.upsert(manifest(2, "fp:aa", "Stolen", WorkType::Audio, 12));
        assert!(matches!(err, Err(IndexError::DuplicateFingerprint { .. })));
    }

    /// Search matches title tokens, filters by type, and omits zero-score works.
    #[test]
    fn search_matches_and_filters() {
        let mut idx = Index::new();
        idx.upsert(manifest(1, "fp:a", "Blue Ocean", WorkType::Audio, 10)).unwrap();
        idx.upsert(manifest(2, "fp:b", "Red Desert", WorkType::Video, 10)).unwrap();
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
        idx.upsert(manifest(1, "fp:a", "Old", WorkType::Audio, 100)).unwrap();
        idx.upsert(manifest(2, "fp:b", "New", WorkType::Audio, 200)).unwrap();
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
        idx.upsert(manifest(1, "fp:a", "Song", WorkType::Audio, 10)).unwrap();
        idx.save_snapshot(&path).unwrap();
        let loaded = Index::load_snapshot(&path).unwrap();
        assert_eq!(loaded.resolve("fp:a").unwrap().work_id, Bytes32([1; 32]));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-discovery-hub index`
Expected: FAIL to compile — `Index` and friends undefined.

- [ ] **Step 3: Implement the index**

Prepend to `services/discovery-hub/src/index.rs`:

```rust
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
                return Err(IndexError::DuplicateFingerprint { fingerprint: m.fingerprint });
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
        let results = scored.into_iter().skip(start).take(page_size).map(|(_, m)| summary_of(m)).collect();
        (results, total)
    }

    /// Trending list, newest first (recency-only in the MVP; usage feed is future
    /// work). `now_secs` is accepted for the recency computation but ordering is by
    /// `created_at` descending, which is equivalent for the pure-recency formula.
    pub fn trending(&self, work_type: Option<WorkType>, _now_secs: u64, page_size: usize) -> Vec<Summary> {
        let mut items: Vec<&WorkManifest> = self
            .works
            .values()
            .filter(|m| work_type.is_none_or(|t| m.work_type == t))
            .collect();
        // Most recent first; ties broken by work_id for determinism.
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.work_id.cmp(&b.work_id)));
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
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
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
```

Add `pub mod index;` to `services/discovery-hub/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cwe-discovery-hub index`
Expected: PASS (5 tests). Then `cargo fmt --all` and
`cargo clippy -p cwe-discovery-hub --all-targets -- -D warnings`.

- [ ] **Step 5: Commit**

```bash
git add services/discovery-hub/src/index.rs services/discovery-hub/src/lib.rs
git commit -m "Add in-memory index with resolve/search/trending/snapshot"
```

---

## Task 5: `chain` module + ingest validation

The registry cross-check (behind a trait so ingest is testable without a chain) and
the full ingest-validation function.

**Files:**
- Create: `services/discovery-hub/src/chain.rs`
- Modify: `services/discovery-hub/src/lib.rs` (add `pub mod chain;`)

**Interfaces:**
- Consumes: `manifest::WorkManifest`, `Bytes32`, `Address`.
- Produces (`cwe_discovery_hub::chain`):
  - `struct OnChainWork { registrant: Address, price_per_min: u64, region: Bytes32 }`
  - `trait RegistryView { async fn lookup(&self, work_id: Bytes32) -> Result<Option<OnChainWork>, String>; }`
  - `struct DiscoveryChain` implementing `RegistryView` over alloy (constructed from an RPC URL + registry address)
  - `enum IngestError` (thiserror): `Signature`, `SignerMismatch`, `Unregistered`, `PriceMismatch`, `RegionMismatch`, `CreatorMismatch`, `Chain(String)`
  - `async fn validate_ingest<R: RegistryView>(m: &WorkManifest, signature: &[u8], registry: &R) -> Result<(), IngestError>`

- [ ] **Step 1: Write the failing tests (validation with a fake registry)**

Create `services/discovery-hub/src/chain.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::WorkType;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    /// An in-memory RegistryView for tests.
    struct FakeRegistry(Option<OnChainWork>);
    impl RegistryView for FakeRegistry {
        async fn lookup(&self, _work_id: Bytes32) -> Result<Option<OnChainWork>, String> {
            Ok(self.0.clone())
        }
    }

    fn manifest(creator: Address) -> WorkManifest {
        WorkManifest {
            work_id: Bytes32([1; 32]),
            fingerprint: "fp:aa".to_string(),
            title: "Song".to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([7; 32]),
            creator_id: creator,
            created_at: 1,
        }
    }

    #[tokio::test]
    async fn valid_manifest_passes() {
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap();
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: signer.address(),
            price_per_min: 1_000_000,
            region: Bytes32([7; 32]),
        }));
        assert!(validate_ingest(&m, &sig.as_bytes(), &reg).await.is_ok());
    }

    #[tokio::test]
    async fn wrong_signer_is_rejected() {
        let signer = PrivateKeySigner::random();
        let other = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap();
        // Registry says a different address is the registrant.
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: other.address(),
            price_per_min: 1_000_000,
            region: Bytes32([7; 32]),
        }));
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::SignerMismatch)
        ));
    }

    #[tokio::test]
    async fn price_mismatch_is_rejected() {
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap();
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: signer.address(),
            price_per_min: 2_000_000, // differs from the manifest
            region: Bytes32([7; 32]),
        }));
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::PriceMismatch)
        ));
    }

    #[tokio::test]
    async fn unregistered_work_is_rejected() {
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap();
        let reg = FakeRegistry(None); // not registered
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::Unregistered)
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-discovery-hub chain`
Expected: FAIL to compile — `RegistryView`, `validate_ingest`, etc. undefined.

- [ ] **Step 3: Implement the chain module**

Prepend to `services/discovery-hub/src/chain.rs`:

```rust
//! Registry cross-checks for manifest ingest (design §4).
//!
//! Ingest validation is written against the [`RegistryView`] trait so it can be
//! unit-tested with a fake, while production uses [`DiscoveryChain`] over alloy —
//! the same RPC pattern as `cwe-settlement`.

use alloy::primitives::{Address, B256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use cwe_wallet_zk::Bytes32;

use crate::manifest::WorkManifest;

// The subset of CWERegistry the hub reads.
sol! {
    #[sol(rpc)]
    contract Registry {
        function isRegistered(bytes32 workId) external view returns (bool);
        function pricePerMinOf(bytes32 workId) external view returns (uint256);
        function regionRuleOf(bytes32 workId) external view returns (bytes32);
        function registrantOf(bytes32 workId) external view returns (address);
    }
}

/// The on-chain facts about a work that ingest cross-checks against.
#[derive(Debug, Clone, PartialEq)]
pub struct OnChainWork {
    /// The address allowed to publish/update this work's manifest.
    pub registrant: Address,
    /// The registered price per minute (ppm).
    pub price_per_min: u64,
    /// The registered region tag.
    pub region: Bytes32,
}

/// A read-only view of the work registry. Abstracted for testability.
pub trait RegistryView {
    /// Look up a work; `Ok(None)` means it is not registered.
    async fn lookup(&self, work_id: Bytes32) -> Result<Option<OnChainWork>, String>;
}

/// Errors that reject a manifest at ingest.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The signature was malformed or unrecoverable.
    #[error("invalid manifest signature")]
    Signature,
    /// The recovered signer is not the on-chain registrant.
    #[error("signer is not the work's registrant")]
    SignerMismatch,
    /// The work is not registered on-chain.
    #[error("work is not registered")]
    Unregistered,
    /// The manifest price disagrees with the on-chain price.
    #[error("price does not match the on-chain value")]
    PriceMismatch,
    /// The manifest region disagrees with the on-chain region.
    #[error("region does not match the on-chain value")]
    RegionMismatch,
    /// The manifest creator_id is not the registrant.
    #[error("creator_id is not the registrant")]
    CreatorMismatch,
    /// A chain/RPC error occurred.
    #[error("chain error: {0}")]
    Chain(String),
}

/// Validate a manifest for ingest: signature → registrant match → on-chain
/// agreement (design §4, steps 2–4; the duplicate guard is the index's job).
pub async fn validate_ingest<R: RegistryView>(
    m: &WorkManifest,
    signature: &[u8],
    registry: &R,
) -> Result<(), IngestError> {
    // Step 2: recover the signer from the signature over the canonical bytes.
    let signer = m.recover_signer(signature).map_err(|_| IngestError::Signature)?;

    // Step 4 (fetch): read the on-chain facts.
    let on_chain = registry
        .lookup(m.work_id)
        .await
        .map_err(IngestError::Chain)?
        .ok_or(IngestError::Unregistered)?;

    // Step 3: the signer and the manifest's creator_id must be the registrant.
    if signer != on_chain.registrant {
        return Err(IngestError::SignerMismatch);
    }
    if m.creator_id != on_chain.registrant {
        return Err(IngestError::CreatorMismatch);
    }
    // Step 4 (compare): price and region must match the chain.
    if m.price_per_min != on_chain.price_per_min {
        return Err(IngestError::PriceMismatch);
    }
    if m.region != on_chain.region {
        return Err(IngestError::RegionMismatch);
    }
    Ok(())
}

/// Production [`RegistryView`] backed by an alloy provider.
pub struct DiscoveryChain {
    provider: alloy::providers::fillers::FillProvider<
        alloy::providers::Identity,
        alloy::providers::RootProvider,
    >,
    registry: Address,
}

impl DiscoveryChain {
    /// Connect to `rpc_url` and target the registry at `registry`.
    pub fn new(rpc_url: &str, registry: Address) -> Result<DiscoveryChain, String> {
        let url = rpc_url.parse().map_err(|_| "bad RPC URL".to_string())?;
        let provider = ProviderBuilder::new().connect_http(url);
        Ok(DiscoveryChain { provider, registry })
    }
}

impl RegistryView for DiscoveryChain {
    async fn lookup(&self, work_id: Bytes32) -> Result<Option<OnChainWork>, String> {
        let registry = Registry::new(self.registry, &self.provider);
        let wid = B256::from(work_id.0);
        // A work with no registrant (zero address) is treated as unregistered.
        if !registry.isRegistered(wid).call().await.map_err(|e| e.to_string())? {
            return Ok(None);
        }
        let price = registry.pricePerMinOf(wid).call().await.map_err(|e| e.to_string())?;
        let region = registry.regionRuleOf(wid).call().await.map_err(|e| e.to_string())?;
        let registrant = registry.registrantOf(wid).call().await.map_err(|e| e.to_string())?;
        Ok(Some(OnChainWork {
            registrant,
            price_per_min: u64::try_from(price).map_err(|_| "price overflow".to_string())?,
            region: Bytes32(region.0),
        }))
    }
}
```

Add `pub mod chain;` to `services/discovery-hub/src/lib.rs`.

> Note: if the exact `FillProvider`/`RootProvider` generic types above do not match
> the installed alloy version, replace the `provider` field type with
> `impl Provider` behind a boxed/`Arc` provider, or store the concrete type
> `alloy::providers::RootProvider` — the goal is simply "a provider that can
> `.call()` the registry". Adjust to what the compiler accepts; the trait impl body
> is unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cwe-discovery-hub chain`
Expected: PASS (4 tests). Then fmt + clippy.

- [ ] **Step 5: Commit**

```bash
git add services/discovery-hub/src/chain.rs services/discovery-hub/src/lib.rs
git commit -m "Add registry cross-check and ingest validation"
```

---

## Task 6: `api` module + `main` (axum + OpenAPI)

Wire the index, chain, and manifest into HTTP routes with an OpenAPI document.

**Files:**
- Create: `services/discovery-hub/src/api.rs`
- Create: `services/discovery-hub/src/config.rs`
- Create: `services/discovery-hub/src/main.rs`
- Modify: `services/discovery-hub/src/lib.rs`

**Interfaces:**
- Consumes: `index::Index`, `chain::{DiscoveryChain, validate_ingest}`, `manifest::WorkManifest`.
- Produces (`cwe_discovery_hub::api`):
  - `struct AppState { index: Arc<RwLock<Index>>, chain: Arc<DiscoveryChain>, snapshot: PathBuf }`
  - `fn router(state: AppState) -> axum::Router`
  - `struct IngestBody { manifest: WorkManifest, signature: String }`
  - handlers for each route in the design's API table
  - `fn openapi_json() -> String`

- [ ] **Step 1: Write the failing test (routes with a fake chain)**

Because the ingest handler needs a chain, expose the router over a generic
`RegistryView` in a thin internal helper so tests use a fake. Create
`services/discovery-hub/src/api.rs` with the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{OnChainWork, RegistryView};
    use crate::manifest::WorkType;
    use alloy::primitives::Address;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use cwe_wallet_zk::Bytes32;
    use tower::ServiceExt; // for `oneshot`

    struct FakeReg(Address);
    impl RegistryView for FakeReg {
        async fn lookup(&self, _w: Bytes32) -> Result<Option<OnChainWork>, String> {
            Ok(Some(OnChainWork { registrant: self.0, price_per_min: 1_000_000, region: Bytes32([0; 32]) }))
        }
    }

    /// Ingesting a valid manifest then resolving its fingerprint round-trips.
    #[tokio::test]
    async fn ingest_then_resolve() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state.clone());

        let m = WorkManifest {
            work_id: Bytes32([1; 32]),
            fingerprint: "fp:aa".to_string(),
            title: "Song".to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: signer.address(),
            created_at: 1,
        };
        let sig = format!("0x{}", hex::encode(signer.sign_message_sync(&m.canonical_bytes().unwrap()).unwrap().as_bytes()));
        let body = serde_json::json!({ "manifest": m, "signature": sig }).to_string();

        let resp = app
            .clone()
            .oneshot(Request::post("/manifests").header("content-type", "application/json").body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(Request::get("/resolve/fp:aa").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p cwe-discovery-hub api`
Expected: FAIL to compile — `router_generic`, `test_state`, etc. undefined. (Also
add `tower` and `axum` `features = ["macros"]` to `[dev-dependencies]`/deps as the
compiler requires.)

- [ ] **Step 3: Implement api + config + main**

Prepend to `services/discovery-hub/src/api.rs` an implementation that: holds
`Arc<RwLock<Index>>` plus a registry (generic `R: RegistryView + Send + Sync`) in
state; defines `router_generic<R>(state)` returning the `axum::Router`; and
implements handlers for `POST /manifests` (calls `validate_ingest`, then
`index.upsert`, then `save_snapshot`), `GET /resolve/:fingerprint`,
`GET /search`, `GET /trending`, `GET /manifest/:work_id`, `GET /creator/:address`,
`GET /healthz`, and `GET /openapi.json`. Provide a public `router(AppState)` that
fixes `R = DiscoveryChain`. Derive `utoipa::ToSchema` on `WorkManifest`, `Summary`,
`Resolved`, and an `#[derive(OpenApi)]` aggregator whose `openapi().to_pretty_json()`
backs `/openapi.json`. Add a `test_state` helper under `#[cfg(test)]`.

Key handler shape (ingest), for reference:

```rust
async fn ingest<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Json(body): Json<IngestBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Decode the hex signature.
    let sig = hex::decode(body.signature.trim_start_matches("0x"))
        .map_err(|_| err(StatusCode::BAD_REQUEST, "bad signature hex"))?;
    // Validate against the chain, then insert and snapshot.
    crate::chain::validate_ingest(&body.manifest, &sig, state.registry.as_ref())
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let work_id = body.manifest.work_id;
    {
        let mut idx = state.index.write().await;
        idx.upsert(body.manifest).map_err(|e| err(StatusCode::CONFLICT, &e.to_string()))?;
        idx.save_snapshot(&state.snapshot).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "work_id": work_id }))))
}
```

Create `services/discovery-hub/src/config.rs` reading `BIND` (default
`127.0.0.1:8080`), `RPC_URL` (default `http://127.0.0.1:8545`), `REGISTRY`
(required, the address), and `SNAPSHOT` (default `hub-index.json`) from the
environment into a `Config` struct.

Create `services/discovery-hub/src/main.rs`:

```rust
//! `cwe-hub` — run the Discovery Hub HTTP server.
use std::sync::Arc;
use tokio::sync::RwLock;

use cwe_discovery_hub::api::{router, AppState};
use cwe_discovery_hub::chain::DiscoveryChain;
use cwe_discovery_hub::config::Config;
use cwe_discovery_hub::index::Index;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load config and the persisted index snapshot.
    let cfg = Config::from_env()?;
    let index = Index::load_snapshot(&cfg.snapshot)?;
    let chain = DiscoveryChain::new(&cfg.rpc_url, cfg.registry)?;
    let state = AppState {
        index: Arc::new(RwLock::new(index)),
        chain: Arc::new(chain),
        snapshot: cfg.snapshot.clone(),
    };
    // Bind and serve.
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    println!("discovery hub listening on {}", cfg.bind);
    axum::serve(listener, router(state)).await?;
    Ok(())
}
```

Add `pub mod api;` and `pub mod config;` to `lib.rs`.

- [ ] **Step 4: Run the test + full gate to verify they pass**

Run: `cargo test -p cwe-discovery-hub` then
`cargo clippy -p cwe-discovery-hub --all-targets -- -D warnings` and
`cargo build -p cwe-discovery-hub`.
Expected: PASS / clean / builds both binaries.

- [ ] **Step 5: Commit**

```bash
git add services/discovery-hub/src/api.rs services/discovery-hub/src/config.rs services/discovery-hub/src/main.rs services/discovery-hub/src/lib.rs services/discovery-hub/Cargo.toml
git commit -m "Add axum API, OpenAPI, and hub server binary"
```

---

## Task 7: Extension `NetworkedHubClient`

Let the extension resolve fingerprints against the live hub, falling back to the
static manifest.

**Files:**
- Modify: `clients/browser-ext/src/hub.js`
- Modify: `clients/browser-ext/src/background.js`
- Modify: `clients/browser-ext/src/options.html`, `clients/browser-ext/src/options.js`
- Test: `clients/browser-ext/test/hub.test.mjs`

**Interfaces:**
- Produces (JS): `class NetworkedHubClient { constructor(hubUrl, fallback); async resolveFingerprint(fp) }`

- [ ] **Step 1: Write the failing test**

Add to `clients/browser-ext/test/hub.test.mjs`:

```javascript
import { NetworkedHubClient } from "../src/hub.js";

test("networked client resolves via fetch and maps the response", async () => {
  const fakeFetch = async (url) => ({
    ok: url.endsWith("/resolve/fp:aaaa"),
    json: async () => ({ work_id: "0x01", price_per_min: 100, work_type: "audio" }),
  });
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.resolveFingerprint("fp:aaaa");
  assert.equal(work.work_id, "0x01");
  assert.equal(work.price_per_min, 100);
});

test("networked client falls back to the static client on miss", async () => {
  const fakeFetch = async () => ({ ok: false });
  const fallback = new StaticHubClient({ "fp:bbbb": { work_id: "0x02", price_per_min: 5, region_factor: 1 } });
  const hub = new NetworkedHubClient("http://hub.test", fallback, fakeFetch);
  const work = await hub.resolveFingerprint("fp:bbbb");
  assert.equal(work.work_id, "0x02");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd clients/browser-ext && node --test test/hub.test.mjs`
Expected: FAIL — `NetworkedHubClient` is not exported.

- [ ] **Step 3: Implement `NetworkedHubClient`**

Append to `clients/browser-ext/src/hub.js`:

```javascript
/**
 * Resolves fingerprints against a live Discovery Hub, falling back to a static
 * client on a miss or network error. `fetchImpl` is injectable for testing.
 */
export class NetworkedHubClient {
  /**
   * @param {string} hubUrl Base URL of the Discovery Hub.
   * @param {?StaticHubClient} fallback Used when the hub has no answer.
   * @param {typeof fetch} [fetchImpl] Fetch implementation (defaults to global fetch).
   */
  constructor(hubUrl, fallback, fetchImpl) {
    this.hubUrl = hubUrl.replace(/\/$/, "");
    this.fallback = fallback;
    this.fetchImpl = fetchImpl || fetch;
  }

  /**
   * Resolve a fingerprint via the hub, then the fallback.
   * @param {string} fingerprint The `fp:<hex>` identifier.
   * @returns {Promise<?object>} Work metadata or null.
   */
  async resolveFingerprint(fingerprint) {
    try {
      const resp = await this.fetchImpl(`${this.hubUrl}/resolve/${encodeURIComponent(fingerprint)}`);
      if (resp.ok) {
        // The hub returns {work_id, price_per_min, region, work_type}.
        return await resp.json();
      }
    } catch (_e) {
      // Network failure: fall through to the static fallback.
    }
    return this.fallback ? this.fallback.resolveFingerprint(fingerprint) : null;
  }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd clients/browser-ext && node --test test/hub.test.mjs`
Expected: PASS (all hub tests).

- [ ] **Step 5: Wire it into the background and options**

In `clients/browser-ext/src/background.js`, in `ensureReady`, after loading the
manifest, choose the client based on a `hubUrl` config value:

```javascript
      const staticClient = new StaticHubClient(manifest);
      const stored2 = await chrome.storage.local.get("config");
      const hubUrl = stored2.config && stored2.config.hubUrl;
      hub = hubUrl ? new NetworkedHubClient(hubUrl, staticClient) : staticClient;
```

Update the import at the top of `background.js`:

```javascript
import { StaticHubClient, NetworkedHubClient } from "./hub.js";
```

Add a `hubUrl` input to `options.html` (a text field) and include `"hubUrl"` in the
`FIELDS` array in `options.js`.

- [ ] **Step 6: Rebuild and re-run extension tests**

Run: `cd clients/browser-ext && npm run build && npm test`
Expected: build succeeds; all unit tests pass.

- [ ] **Step 7: Commit**

```bash
git add clients/browser-ext/src/hub.js clients/browser-ext/src/background.js clients/browser-ext/src/options.html clients/browser-ext/src/options.js clients/browser-ext/test/hub.test.mjs
git commit -m "Add NetworkedHubClient to resolve fingerprints via the hub"
```

---

## Task 8: `make hub-demo` end-to-end + CI + docs

Prove the whole flow, add CI, and document the service.

**Files:**
- Create: `ops/demo/run_hub_demo.sh`
- Modify: `ops/Makefile` (add `hub-demo`)
- Modify: `.github/workflows/ci.yml` (add a hub e2e job)
- Create: `services/discovery-hub/README.md`
- Modify: `.gitignore` (ignore `hub-index.json` snapshots)

**Interfaces:** none (integration).

- [ ] **Step 1: Write the demo script**

Create `ops/demo/run_hub_demo.sh` (self-contained, starts its own Anvil + hub):

```bash
#!/usr/bin/env bash
# End-to-end Discovery Hub demo: deploy -> register work -> sign & POST manifest ->
# resolve + search -> assert a non-registrant manifest is rejected.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
RPC=http://127.0.0.1:8545
HUB=http://127.0.0.1:8080
WORK="$(mktemp -d)"

cargo build --quiet -p cwe-discovery-hub --manifest-path "$ROOT/Cargo.toml"

anvil > "$WORK/anvil.log" 2>&1 & ANVIL=$!
trap 'kill -TERM "$ANVIL" "${HUBPID:-}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && break; done
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORK/anvil.log" | head -3)
DEPLOYER=${KEYS[0]}; OUTSIDER=${KEYS[1]}
DEPLOYER_ADDR=$(cast wallet address $DEPLOYER)

( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol --rpc-url $RPC --broadcast >/dev/null 2>&1 )
REG=$(jq -r .registry "$ROOT/chain/deployments/localhost.json")

# Register a work on-chain (deployer is owner + verified creator + registrant).
send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
send $DEPLOYER $REG "setVerifiedCreator(address,bool)" $DEPLOYER_ADDR true
WORK_ID=$(cast format-bytes32-string "workA"); EU=$(cast format-bytes32-string "EU")
PAYEE=$(cast wallet address ${KEYS[2]})
send $DEPLOYER $REG "registerWork(bytes32,address[],uint96[],uint256,bytes32)" \
  $WORK_ID "[$PAYEE]" "[1000000]" 1000000 $EU

# Start the hub.
REGISTRY=$REG RPC_URL=$RPC SNAPSHOT="$WORK/index.json" "$ROOT/target/debug/cwe-hub" & HUBPID=$!
for _ in $(seq 1 40); do curl -sf $HUB/healthz >/dev/null 2>&1 && break; done

FP="fp:$(printf 'a%.0s' {1..64})"
manifest() { cat <<JSON
{"work_id":"$WORK_ID","fingerprint":"$FP","title":"Blue Ocean","description":"demo","tags":["calm"],"work_type":"audio","price_per_min":1000000,"region":"$EU","creator_id":"$1","created_at":1}
JSON
}

# Sign as the registrant and POST -> expect 201.
ENVELOPE=$(manifest $DEPLOYER_ADDR | PRIVATE_KEY=$DEPLOYER "$ROOT/target/debug/sign-manifest")
CODE=$(curl -s -o "$WORK/post.out" -w '%{http_code}' -X POST $HUB/manifests -H 'content-type: application/json' -d "$ENVELOPE")
[ "$CODE" = "201" ] || { echo "FAIL: ingest expected 201, got $CODE"; cat "$WORK/post.out"; exit 1; }

# Resolve + search.
curl -sf "$HUB/resolve/$FP" | jq -e '.work_id' >/dev/null || { echo "FAIL: resolve"; exit 1; }
curl -sf "$HUB/search?q=ocean" | jq -e '.results[0].title == "Blue Ocean"' >/dev/null || { echo "FAIL: search"; exit 1; }

# A manifest signed by a non-registrant must be rejected (4xx).
BAD=$(manifest $(cast wallet address $OUTSIDER) | PRIVATE_KEY=$OUTSIDER "$ROOT/target/debug/sign-manifest")
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST $HUB/manifests -H 'content-type: application/json' -d "$BAD")
[ "${CODE:0:1}" = "4" ] || { echo "FAIL: non-registrant expected 4xx, got $CODE"; exit 1; }

echo "✅ HUB DEMO PASSED — ingest, resolve, search, and rejection all correct."
```

- [ ] **Step 2: Add the Makefile target**

Add to `ops/Makefile`:

```makefile
hub-demo: ## Run the Discovery Hub end-to-end demo (self-contained)
	bash demo/run_hub_demo.sh
```

Add `hub-demo` to the `.PHONY` line.

- [ ] **Step 3: Run the demo**

Run: `chmod +x ops/demo/run_hub_demo.sh && make -C ops hub-demo`
Expected: ends with `✅ HUB DEMO PASSED`.

- [ ] **Step 4: Add CI job, README, and gitignore**

Add a `hub-e2e` job to `.github/workflows/ci.yml` mirroring the existing `e2e` job
(checkout, Rust, Foundry, `jq`, plus `curl`), running `make -C ops hub-demo`.

Create `services/discovery-hub/README.md` documenting the endpoints, the ingest
trust model, running (`REGISTRY=<addr> cargo run -p cwe-discovery-hub --bin cwe-hub`),
the `sign-manifest` CLI, and `make -C ops hub-demo`.

Add to `.gitignore`:

```
# Discovery Hub local index snapshot
hub-index.json
services/discovery-hub/hub-index.json
```

- [ ] **Step 5: Final gate + commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` and `cd chain && forge test`.
Expected: all green.

```bash
git add ops/demo/run_hub_demo.sh ops/Makefile .github/workflows/ci.yml services/discovery-hub/README.md .gitignore
git commit -m "Add hub-demo end-to-end, CI job, and docs"
```

---

## Self-Review

**Spec coverage:** resolution (Task 6 `/resolve` + Task 7 extension), search/trending
(Task 4 + 6), manifest ingest with chain-anchored signing (Tasks 2, 3, 5, 6),
OpenAPI (Task 6), privacy/stateless (Task 6 handlers keep no per-user state), the
registry getters the design flagged (Task 1), the signing CLI (Task 3), extension
integration (Task 7), and `make hub-demo` + CI + docs (Task 8). Out-of-scope items
(federation, differential privacy, IPFS, DAPR-fed ranking, reputation) are absent by
design.

**Placeholder scan:** the only non-literal guidance is the alloy provider-type note
in Task 5 (a compiler-driven adjustment, with the fallback stated) and the Task 6
handler prose (with the ingest handler shown in full and the remaining handlers
being thin wrappers over already-defined `index`/`chain` methods). No "TBD"/"handle
edge cases"/"write tests for the above" placeholders remain.

**Type consistency:** `WorkManifest`/`WorkType` (Task 2) are consumed unchanged by
Tasks 3–6; `RegistryView`/`OnChainWork`/`validate_ingest` (Task 5) are consumed by
Task 6; `Index` methods (Task 4) match their calls in Task 6; `Resolved`'s fields
match the extension's expectations (Task 7). `Bytes32` and `Address` come from
`cwe-wallet-zk`/alloy throughout.
