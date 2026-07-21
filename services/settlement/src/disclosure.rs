//! The disclosure file — Phase 1's stand-in for ZK-verified aggregates.
//!
//! On-chain, a user submits only opaque commitments. To actually pay creators, the
//! aggregator must learn what each commitment opens to. In Phase 1 (no ZK circuits,
//! decision D2) the user shares that out-of-band as a disclosure file mapping each
//! user address to their [`Opening`]s. The settlement job recomputes each opening's
//! commitment and checks it against what the user submitted on-chain, so a user
//! cannot claim usage they did not commit to. A real deployment replaces this file
//! with a zero-knowledge proof of the same facts.

use std::collections::BTreeMap;
use std::path::Path;

use cwe_wallet_zk::commit::Opening;
use serde::{Deserialize, Serialize};

/// A disclosure file: user address (lowercase `0x` hex) → their epoch openings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Disclosure {
    /// Openings keyed by the submitting user's address.
    pub users: BTreeMap<String, Vec<Opening>>,
    /// Works recognized via perceptual fingerprint (Tier 2) this epoch — their
    /// credit is routed to escrow rather than paid directly. Absent/empty means
    /// every work was signed-recognized (Tier 1).
    #[serde(default)]
    pub escrow_works: Vec<cwe_wallet_zk::Bytes32>,
}

impl Disclosure {
    /// Load and parse a disclosure file from disk.
    pub fn load(path: &Path) -> Result<Self, DisclosureError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| DisclosureError::Io(path.display().to_string(), e.to_string()))?;
        serde_json::from_str(&raw)
            .map_err(|e| DisclosureError::Parse(path.display().to_string(), e.to_string()))
    }

    /// The openings disclosed for a user address (case-insensitive on the hex).
    pub fn for_user(&self, address: &str) -> Option<&Vec<Opening>> {
        self.users.get(&address.to_lowercase())
    }
}

/// Errors from loading a disclosure file.
#[derive(Debug, thiserror::Error)]
pub enum DisclosureError {
    /// The file could not be read.
    #[error("reading disclosure file {0}: {1}")]
    Io(String, String),
    /// The file was not valid disclosure JSON.
    #[error("parsing disclosure file {0}: {1}")]
    Parse(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cwe_wallet_zk::Bytes32;

    /// A disclosure round-trips through JSON and is looked up case-insensitively.
    #[test]
    fn disclosure_json_and_lookup() {
        let mut d = Disclosure::default();
        d.users.insert(
            "0xabc".to_string(),
            vec![Opening::new(Bytes32([1; 32]), 30, Bytes32([2; 32]))],
        );
        let json = serde_json::to_string(&d).unwrap();
        let back: Disclosure = serde_json::from_str(&json).unwrap();
        // Lookup upper-cases to the stored lowercase key.
        assert!(back.for_user("0xABC").is_some());
        assert_eq!(back.for_user("0xabc").unwrap()[0].minutes, 30);
    }
}
