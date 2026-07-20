//! The pure settlement computation: dataset → committable root + withdrawal proofs.
//!
//! This module is deliberately free of any chain or IO code so it can be unit
//! tested in isolation. It takes an already-assembled DAPR [`Dataset`] (built by
//! the chain layer from on-chain events, the registry, and the disclosure file),
//! runs the shared payout math, and turns the per-work credits into a Merkle root
//! plus a per-work inclusion proof — exactly what `CWEPayouts.commitEpoch` and
//! `withdraw` consume.

use std::str::FromStr;

use cwe_dapr::{allocate, DaprError, Dataset};
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

use crate::merkle::{leaf_hash, MerkleTree};

/// One work's settlement result: how much it is owed and the proof to claim it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementEntry {
    /// The work's on-chain id.
    pub work_id: Bytes32,
    /// Credit owed to the work. Serialised as a string so large values survive
    /// JSON consumers that use 53-bit floats (e.g. JavaScript tooling).
    #[serde(with = "u128_string")]
    pub amount: u128,
    /// The Merkle inclusion proof for `(work_id, amount)`.
    pub proof: Vec<Bytes32>,
}

/// The full result of settling an epoch: the root to commit, the total credited,
/// and every work's withdrawal proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    /// The epoch that was settled.
    pub epoch: u64,
    /// The Merkle root to commit on-chain.
    pub merkle_root: Bytes32,
    /// Sum of all entry amounts — the `totalCredits` argument to `commitEpoch`.
    #[serde(with = "u128_string")]
    pub total_credits: u128,
    /// Fee that had no attributable usage and stays undistributed in the pool.
    #[serde(with = "u128_string")]
    pub unallocated: u128,
    /// Per-work credits and their proofs.
    pub entries: Vec<SettlementEntry>,
}

/// Errors settlement can raise.
#[derive(Debug, thiserror::Error)]
pub enum SettleError {
    /// The DAPR computation failed (e.g. arithmetic overflow).
    #[error("payout computation failed: {0}")]
    Dapr(#[from] DaprError),
    /// A work id in the dataset was not a valid 32-byte hex value.
    #[error("work id '{0}' is not a valid 32-byte hex value")]
    BadWorkId(String),
    /// The epoch produced no credited works, so there is nothing to commit.
    #[error("no credited works to settle for this epoch")]
    NoCredits,
}

/// Settle an epoch: run DAPR over `dataset`, build the credit Merkle tree, and
/// return the root plus a withdrawal proof for every credited work.
///
/// Work ids in the dataset must be 32-byte hex strings (the on-chain `bytes32`
/// form); the chain layer guarantees this when it assembles the dataset.
pub fn settle(epoch: u64, dataset: &Dataset) -> Result<Settlement, SettleError> {
    // 1. Compute per-work credits with the shared payout math.
    let payouts = allocate(dataset)?;

    // A settlement with no credited works is a caller/config error (e.g. an epoch
    // with no usage); surface it rather than committing an empty root.
    if payouts.per_work.is_empty() {
        return Err(SettleError::NoCredits);
    }

    // 2. Parse work ids and build the ordered leaf list. `per_work` is a BTreeMap,
    //    so iteration is sorted and deterministic; that same order indexes the tree.
    let mut work_ids: Vec<Bytes32> = Vec::with_capacity(payouts.per_work.len());
    let mut amounts: Vec<u128> = Vec::with_capacity(payouts.per_work.len());
    let mut leaves: Vec<[u8; 32]> = Vec::with_capacity(payouts.per_work.len());
    for (work_hex, amount) in &payouts.per_work {
        let work =
            Bytes32::from_str(work_hex).map_err(|_| SettleError::BadWorkId(work_hex.clone()))?;
        leaves.push(leaf_hash(*work.as_bytes(), *amount));
        work_ids.push(work);
        amounts.push(*amount);
    }

    // 3. Build the tree and read the root.
    let tree = MerkleTree::build(leaves);
    let root = tree.root();

    // 4. Emit one entry (with its proof) per credited work.
    let entries = (0..work_ids.len())
        .map(|i| SettlementEntry {
            work_id: work_ids[i],
            amount: amounts[i],
            proof: tree.proof(i).into_iter().map(Bytes32).collect(),
        })
        .collect();

    Ok(Settlement {
        epoch,
        merkle_root: Bytes32(root),
        total_credits: payouts.total_to_works(),
        unallocated: payouts.unallocated,
        entries,
    })
}

/// Serde helper serialising `u128` as a decimal string (JSON-number-safe).
mod u128_string {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialise the value as its decimal string form.
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    /// Parse the value back from a decimal string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::verify;
    use cwe_dapr::UsageRow;
    use std::collections::BTreeMap;

    /// Build a dataset whose work ids are valid 32-byte hex values.
    fn hex_dataset() -> Dataset {
        // Two works, distinguishable 32-byte ids.
        let work_a = format!("0x{}", "aa".repeat(32));
        let work_b = format!("0x{}", "bb".repeat(32));
        let mut tier_fees = BTreeMap::new();
        tier_fees.insert("u1".to_string(), 1_000_000u128);
        Dataset {
            tier_fees,
            usage: vec![
                UsageRow {
                    user: "u1".to_string(),
                    work: work_a,
                    minutes: 60,
                    price_ppm: 1_000_000,
                    region_ppm: 1_000_000,
                },
                UsageRow {
                    user: "u1".to_string(),
                    work: work_b,
                    minutes: 20,
                    price_ppm: 1_000_000,
                    region_ppm: 1_000_000,
                },
            ],
        }
    }

    /// Settlement conserves the fee total and every proof verifies against the root.
    #[test]
    fn settle_conserves_and_proofs_verify() {
        let ds = hex_dataset();
        let s = settle(3, &ds).unwrap();

        // Total credited plus unallocated equals the fees paid.
        let sum_entries: u128 = s.entries.iter().map(|e| e.amount).sum();
        assert_eq!(sum_entries, s.total_credits);
        assert_eq!(s.total_credits + s.unallocated, 1_000_000);

        // Each entry's proof reconstructs the committed root.
        for e in &s.entries {
            let leaf = leaf_hash(*e.work_id.as_bytes(), e.amount);
            let proof: Vec<[u8; 32]> = e.proof.iter().map(|b| *b.as_bytes()).collect();
            assert!(verify(&proof, *s.merkle_root.as_bytes(), leaf));
        }
    }

    /// Settlement per-work amounts match the shared DAPR allocation exactly.
    #[test]
    fn settle_matches_dapr_allocation() {
        let ds = hex_dataset();
        let expected = allocate(&ds).unwrap();
        let s = settle(3, &ds).unwrap();
        for e in &s.entries {
            assert_eq!(
                expected.per_work.get(&e.work_id.to_string()),
                Some(&e.amount)
            );
        }
    }

    /// The Settlement serialises to JSON with string amounts and round-trips.
    #[test]
    fn settlement_json_round_trip() {
        let s = settle(3, &hex_dataset()).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"total_credits\":\"1000000\"")); // amount is a string
        let back: Settlement = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
