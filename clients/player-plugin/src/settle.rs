//! Settlement: turn accrued usage into on-chain commitments and a disclosure.
//!
//! The agent is itself the *user*: it submits its usage commitments to
//! `CWEConsumption` and writes a disclosure file mapping its address to the
//! openings (plus the fingerprint-recognised `escrow_works`). The settlement
//! job — run separately as the aggregator — reads that disclosure to pay
//! creators, routing signed works directly and fingerprint works to escrow. The
//! disclosure shape is identical to `services/settlement/src/disclosure.rs`, and
//! reuses the same `Opening` type, so the two cannot drift.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use alloy::primitives::{Address, FixedBytes, B256};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use cwe_wallet_zk::commit::Opening;
use cwe_wallet_zk::zk::UsageEntry;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

use crate::config::PlayerConfig;

// The one on-chain call the agent makes: submit this epoch's usage commitments.
sol! {
    #[sol(rpc)]
    contract Consumption {
        function submitConsumption(bytes32 tierId, bytes32[] workCommitments, bytes proof) external;
    }
}

/// A disclosure file: user address (lowercased) → openings, plus escrow works.
/// Mirrors `services/settlement/src/disclosure.rs`'s `Disclosure` exactly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Disclosure {
    /// Openings keyed by the submitting user's address.
    pub users: BTreeMap<String, Vec<Opening>>,
    /// Works recognised via fingerprint (Tier 2) — routed to escrow. `default`
    /// mirrors the settlement crate's field so an absent key deserialises equally.
    #[serde(default)]
    pub escrow_works: Vec<Bytes32>,
}

/// Turn flushed usage into openings, drawing a fresh salt per entry via `salt_fn`
/// (a real run passes a CSPRNG; tests pass a fixed salt for determinism).
pub fn build_openings(usage: &[UsageEntry], salt_fn: impl Fn(usize) -> Bytes32) -> Vec<Opening> {
    usage
        .iter()
        .enumerate()
        // Each opening binds work + minutes + plays + a hiding salt; its
        // commitment is what goes on-chain, the opening itself into the disclosure.
        .map(|(i, u)| Opening::new(u.work_id, u.minutes, u.plays, salt_fn(i)))
        .collect()
}

/// Write the disclosure JSON for `user_addr`'s `openings` and `escrow_works`.
pub fn write_disclosure(
    path: &Path,
    user_addr: &str,
    openings: &[Opening],
    escrow_works: &[Bytes32],
) -> Result<(), SettleError> {
    let mut users = BTreeMap::new();
    // Lowercase the address so the settlement job's case-insensitive lookup hits.
    users.insert(user_addr.to_lowercase(), openings.to_vec());
    let disc = Disclosure {
        users,
        escrow_works: escrow_works.to_vec(),
    };
    let json =
        serde_json::to_string_pretty(&disc).map_err(|e| SettleError::Encode(e.to_string()))?;
    std::fs::write(path, json + "\n").map_err(|e| SettleError::Io(e.to_string()))
}

/// Derive the agent's `0x`-hex address from its private key, without sending
/// any transaction — so the disclosure (which keys on this address) can be
/// written before the irreversible on-chain submit.
pub fn signer_address(private_key: &str) -> Result<String, SettleError> {
    let signer =
        PrivateKeySigner::from_str(private_key).map_err(|e| SettleError::Signer(e.to_string()))?;
    Ok(format!("{:#x}", signer.address()))
}

/// Submit the openings' commitments to `CWEConsumption`, returning the tx hash
/// and the agent's address (the disclosure key). Async: uses alloy's provider.
pub async fn submit_consumption(
    cfg: &PlayerConfig,
    openings: &[Opening],
) -> Result<(String, String), SettleError> {
    let (private_key, consumption, tier_id) = cfg
        .require_chain()
        .map_err(|e| SettleError::Config(e.to_string()))?;

    // Build a signing provider; the signer's address is the disclosure key.
    let signer =
        PrivateKeySigner::from_str(private_key).map_err(|e| SettleError::Signer(e.to_string()))?;
    let user_addr = format!("{:#x}", signer.address());
    let provider = ProviderBuilder::new().wallet(signer).connect_http(
        cfg.rpc_url
            .parse()
            .map_err(|e: url::ParseError| SettleError::Rpc(e.to_string()))?,
    );

    let consumption_addr =
        Address::from_str(consumption).map_err(|e| SettleError::Config(e.to_string()))?;
    let tier = B256::from_str(tier_id).map_err(|e| SettleError::Config(e.to_string()))?;
    let contract = Consumption::new(consumption_addr, &provider);

    // Each opening's commitment is one bytes32 in the submission array.
    let commitments: Vec<FixedBytes<32>> = openings
        .iter()
        .map(|o| FixedBytes::from(o.commit().0 .0))
        .collect();

    // Submit with an empty proof (Phase 1 accept-all verifier), await the receipt.
    let pending = contract
        .submitConsumption(tier, commitments, alloy::primitives::Bytes::new())
        .send()
        .await
        .map_err(|e| SettleError::Tx(e.to_string()))?;
    let receipt = pending
        .get_receipt()
        .await
        .map_err(|e| SettleError::Tx(e.to_string()))?;
    Ok((format!("{:#x}", receipt.transaction_hash), user_addr))
}

/// Errors from the settle flow.
#[derive(Debug, thiserror::Error)]
pub enum SettleError {
    /// A required chain config field was missing/invalid.
    #[error("settle config: {0}")]
    Config(String),
    /// The signing key was invalid.
    #[error("signer: {0}")]
    Signer(String),
    /// The RPC endpoint was invalid/unreachable.
    #[error("rpc: {0}")]
    Rpc(String),
    /// The submission transaction failed.
    #[error("submit tx: {0}")]
    Tx(String),
    /// Disclosure serialisation failed.
    #[error("encoding disclosure: {0}")]
    Encode(String),
    /// Disclosure file IO failed.
    #[error("disclosure IO: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cwe_wallet_zk::commit::Opening;
    use cwe_wallet_zk::zk::UsageEntry;
    use cwe_wallet_zk::Bytes32;

    /// Built openings preserve work/minutes and commit exactly like `Opening`.
    #[test]
    fn openings_commit_matches() {
        let usage = vec![UsageEntry {
            work_id: Bytes32([7; 32]),
            minutes: 4,
            plays: 2,
        }];
        // A fixed salt makes the commitment deterministic for the assertion.
        let salt = Bytes32([9; 32]);
        let openings = build_openings(&usage, |_| salt);
        assert_eq!(openings.len(), 1);
        assert_eq!(openings[0].minutes, 4);
        assert_eq!(openings[0].plays, 2);
        let expected = Opening::new(Bytes32([7; 32]), 4, 2, salt).commit();
        assert_eq!(openings[0].commit(), expected);
    }

    /// The well-known Anvil dev key derives its well-known address, so the
    /// pre-submit disclosure write keys on the same address the tx will use.
    #[test]
    fn signer_address_matches_anvil_dev_key() {
        let addr =
            signer_address("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
                .unwrap();
        assert_eq!(
            addr.to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    /// The disclosure JSON has the settlement job's exact shape.
    #[test]
    fn disclosure_shape() {
        let dir = std::env::temp_dir().join("cwe-player-settle-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("disclosure.json");
        let openings = vec![Opening::new(Bytes32([7; 32]), 4, 2, Bytes32([9; 32]))];
        write_disclosure(&path, "0xABC", &openings, &[Bytes32([2; 32])]).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Keyed by the lowercased user address; opening carries work_id/minutes/plays/salt.
        assert!(v["users"]["0xabc"].is_array());
        assert_eq!(v["users"]["0xabc"][0]["minutes"], 4);
        assert_eq!(v["users"]["0xabc"][0]["plays"], 2);
        assert!(v["users"]["0xabc"][0]["work_id"].is_string());
        assert!(v["users"]["0xabc"][0]["salt"].is_string());
        // escrow_works lists the fingerprint-recognised works.
        assert_eq!(v["escrow_works"].as_array().unwrap().len(), 1);
    }
}
