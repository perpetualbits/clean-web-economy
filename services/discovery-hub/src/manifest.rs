//! The signed, chain-anchored work manifest (design §3).
//!
//! A manifest carries a work's discovery metadata plus the on-chain fields the hub
//! re-verifies. It is signed by the creator over its RFC 8785 canonical JSON form,
//! so the hub can recover the signer and check it against the registry registrant.

pub use alloy::primitives::Address;
use alloy::primitives::{keccak256, FixedBytes, U256};
use alloy::signers::Signature;
use alloy::sol_types::SolValue;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

/// The modality of a work. Serialised lowercase (`"audio"`, `"video"`, `"text"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkType {
    Audio,
    Video,
    Text,
}

/// A work's public manifest. Field order here does not affect signing: the
/// canonical form sorts keys (RFC 8785).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct WorkManifest {
    /// On-chain CWERegistry work id.
    #[schema(value_type = String)]
    pub work_id: Bytes32,
    /// On-chain content id (distinct from `work_id`; identifies the specific
    /// content revision the split table applies to).
    #[schema(value_type = String)]
    pub content_id: Bytes32,
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
    #[schema(value_type = String)]
    pub region: Bytes32,
    /// The creator's address; MUST equal the on-chain registrant.
    #[schema(value_type = String)]
    pub creator_id: Address,
    /// Client Unix seconds when authored (used only for recency).
    pub created_at: u64,
    /// The revenue split table: each payee's address and its share, out of the
    /// contract's total share denominator (matches `CWERegistry`'s split table).
    #[schema(value_type = Vec<(String, u64)>)]
    pub payees: Vec<(Address, u64)>,
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

/// The keccak256 digest a payee signs to consent to their split share,
/// matching the Solidity `CWERegistry.consentDigest`
/// (`keccak256(abi.encode(workId, contentId, payee, share))`).
///
/// `share` is encoded as a `U256`: `abi.encode` left-pads a `uint96` (numeric
/// ABI words are right-aligned) into the same 32-byte word as a `uint256`, so
/// for any `share < 2^96` the encoded bytes are identical to the contract's
/// `uint96` encoding.
pub fn consent_digest(
    work_id: Bytes32,
    content_id: Bytes32,
    payee: Address,
    share: u64,
) -> [u8; 32] {
    let tuple = (
        FixedBytes::<32>::from(work_id.0),
        FixedBytes::<32>::from(content_id.0),
        payee,
        U256::from(share),
    );
    *keccak256(tuple.abi_encode())
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
        sig.recover_address_from_msg(&msg)
            .map_err(|e| ManifestError::Recover(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    fn sample() -> WorkManifest {
        WorkManifest {
            work_id: Bytes32([0xAA; 32]),
            content_id: Bytes32([0xBB; 32]),
            fingerprint: "fp:".to_string() + &"11".repeat(128),
            title: "Test Track".to_string(),
            description: "a demo".to_string(),
            tags: vec!["demo".to_string(), "audio".to_string()],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: Address::ZERO,
            created_at: 1_721_500_000,
            payees: vec![(Address::ZERO, 1_000_000)],
        }
    }

    /// Canonical bytes are deterministic (RFC 8785 sorts keys).
    #[test]
    fn canonical_bytes_are_stable() {
        let m = sample();
        assert_eq!(m.canonical_bytes().unwrap(), m.canonical_bytes().unwrap());
    }

    /// A consent signature produced for a share recovers to the payee, and the digest
    /// matches the encoding the contract uses (keccak256(abi.encode(work,content,payee,share))).
    #[test]
    fn consent_digest_and_recover() {
        let signer = PrivateKeySigner::random();
        let digest = consent_digest(
            Bytes32([1; 32]),
            Bytes32([2; 32]),
            signer.address(),
            700_000,
        );
        let sig = signer.sign_message_sync(&digest).unwrap();
        // EIP-191 recover over the 32-byte digest.
        assert_eq!(
            sig.recover_address_from_msg(digest).unwrap(),
            signer.address()
        );
    }

    /// A signature recovers to the signer's address; a tampered manifest does not.
    #[test]
    fn recover_matches_signer_and_rejects_tampering() {
        let signer = PrivateKeySigner::random();
        let m = sample();
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
        let sig_bytes = sig.as_bytes();

        assert_eq!(m.recover_signer(&sig_bytes).unwrap(), signer.address());

        let mut tampered = m.clone();
        tampered.price_per_min = 999; // change a field
        assert_ne!(
            tampered.recover_signer(&sig_bytes).unwrap(),
            signer.address()
        );
    }
}
