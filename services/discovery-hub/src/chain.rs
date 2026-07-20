//! Registry cross-checks for manifest ingest (design §4).
//!
//! Ingest validation is written against the [`RegistryView`] trait so it can be
//! unit-tested with a fake, while production uses [`DiscoveryChain`] over alloy —
//! the same RPC pattern as `cwe-settlement`.

use alloy::primitives::{Address, B256};
use alloy::providers::ProviderBuilder;
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
///
/// The future must be `Send` so that `validate_ingest`, generic over this
/// trait, can itself be awaited from a multi-threaded runtime (e.g. an axum
/// handler).
pub trait RegistryView {
    /// Look up a work; `Ok(None)` means it is not registered.
    fn lookup(
        &self,
        work_id: Bytes32,
    ) -> impl std::future::Future<Output = Result<Option<OnChainWork>, String>> + Send;
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
    let signer = m
        .recover_signer(signature)
        .map_err(|_| IngestError::Signature)?;

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

/// Production [`RegistryView`] backed by an alloy provider, connected fresh on
/// each call. Storing the RPC URL rather than a live provider sidesteps naming
/// alloy's provider-stack generic type, which is fiddly to pin exactly.
pub struct DiscoveryChain {
    rpc_url: String,
    registry: Address,
}

impl DiscoveryChain {
    /// Target the registry at `registry`, reachable via `rpc_url`.
    pub fn new(rpc_url: &str, registry: Address) -> DiscoveryChain {
        DiscoveryChain {
            rpc_url: rpc_url.to_string(),
            registry,
        }
    }
}

impl RegistryView for DiscoveryChain {
    async fn lookup(&self, work_id: Bytes32) -> Result<Option<OnChainWork>, String> {
        let provider = ProviderBuilder::new().connect_http(
            self.rpc_url
                .parse()
                .map_err(|_| "bad RPC URL".to_string())?,
        );
        let registry = Registry::new(self.registry, &provider);
        let wid = B256::from(work_id.0);
        // A work with no registrant (zero address) is treated as unregistered.
        if !registry
            .isRegistered(wid)
            .call()
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(None);
        }
        let price = registry
            .pricePerMinOf(wid)
            .call()
            .await
            .map_err(|e| e.to_string())?;
        let region = registry
            .regionRuleOf(wid)
            .call()
            .await
            .map_err(|e| e.to_string())?;
        let registrant = registry
            .registrantOf(wid)
            .call()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Some(OnChainWork {
            registrant,
            price_per_min: u64::try_from(price).map_err(|_| "price overflow".to_string())?,
            region: Bytes32(region.0),
        }))
    }
}

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
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
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
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
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
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
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
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
        let reg = FakeRegistry(None); // not registered
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::Unregistered)
        ));
    }

    #[tokio::test]
    async fn creator_mismatch_is_rejected() {
        let signer = PrivateKeySigner::random();
        let other = PrivateKeySigner::random();
        // The manifest's creator_id disagrees with the signer/registrant.
        let mut m = manifest(signer.address());
        m.creator_id = other.address();
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: signer.address(),
            price_per_min: 1_000_000,
            region: Bytes32([7; 32]),
        }));
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::CreatorMismatch)
        ));
    }

    #[tokio::test]
    async fn region_mismatch_is_rejected() {
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: signer.address(),
            price_per_min: 1_000_000,
            region: Bytes32([9; 32]), // differs from the manifest
        }));
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::RegionMismatch)
        ));
    }

    /// Compile-time assertion that a value's type is `Send`.
    fn assert_send<T: Send>(_: &T) {}

    #[test]
    fn validate_ingest_future_is_send() {
        // If `validate_ingest`'s future were not `Send`, this would fail to
        // compile — which is exactly what Task 6's axum handler requires.
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = [0u8; 65];
        let reg = FakeRegistry(None);
        let fut = validate_ingest(&m, &sig, &reg);
        assert_send(&fut);
    }
}
