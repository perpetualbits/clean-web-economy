//! Settlement job configuration, assembled from environment variables and the
//! deployment address map written by the deploy script.

use std::path::PathBuf;

use serde::Deserialize;

/// The contract addresses the deploy script wrote to `deployments/localhost.json`.
/// Only the fields the settlement job needs are declared; extra keys are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Deployments {
    /// The `CWETiers` address (for reading tier fees).
    pub tiers: String,
    /// The `CWERegistry` address (for reading per-work prices).
    pub registry: String,
    /// The `CWEConsumption` address (for reading submission events).
    pub consumption: String,
    /// The `CWEPayouts` address (for committing the epoch root).
    pub payouts: String,
    /// The `CWEEscrow` address (for committing fingerprint-matched credit).
    pub escrow: String,
    /// The `CWEEpochBeacon` address (for reading `keyFor(epoch)` in event mode).
    pub beacon: String,
    /// The `CWEIdentity` address (for checking storage-node credentials when
    /// verifying bandwidth receipts).
    pub identity: String,
}

/// Everything the settlement run needs.
#[derive(Debug, Clone)]
pub struct Config {
    /// JSON-RPC endpoint of the target chain.
    pub rpc_url: String,
    /// Aggregator private key (hex) used to sign `commitEpoch`.
    pub private_key: String,
    /// The epoch to settle.
    pub epoch: u64,
    /// Path to the disclosure file with the users' openings. `Some` selects the
    /// legacy disclosure mode (the four legacy demos, AcceptAllVerifier); `None`
    /// selects the proven-weights event mode (the ZK path). The chain layer
    /// branches on this to pick which settlement path to run.
    pub disclosure_path: Option<PathBuf>,
    /// Where to write the withdrawal-proofs output.
    pub out_path: PathBuf,
    /// The deployed contract addresses.
    pub deployments: Deployments,
    /// Path to a receipt bundle to verify (from `RECEIPTS`). `None` means no
    /// bandwidth signal was supplied, so credibility stays neutral and payouts
    /// are exactly what they were before H5 — which is what keeps every legacy
    /// demo working unchanged.
    pub receipts_path: Option<PathBuf>,
    /// The absolute per-epoch evidence floor in bytes (from `MIN_EPOCH_BYTES`,
    /// default one whole `CHUNK_SIZE`). A user whose deduped verified bytes fall
    /// below this has every row scaled down proportionally.
    ///
    /// Aggregator-side rather than on-chain because, unlike a work's bandwidth
    /// rate, it is not settable by anyone who benefits from it.
    pub min_epoch_bytes: u64,
}

impl Config {
    /// Assemble the configuration from environment variables.
    ///
    /// Recognised variables (with defaults suited to a local Anvil devnet):
    /// `RPC_URL`, `PRIVATE_KEY` (required), `EPOCH` (required), `DISCLOSURE`
    /// (OPTIONAL — its presence selects legacy disclosure mode; its absence
    /// selects proven-weights event mode), `DEPLOYMENTS` (default
    /// `chain/deployments/localhost.json`), `OUT` (default
    /// `chain/out/epoch-<n>-proofs.json`), `RECEIPTS` (OPTIONAL — a bandwidth
    /// receipt bundle to verify; its absence keeps bandwidth neutral),
    /// `MIN_EPOCH_BYTES` (OPTIONAL — the absolute per-epoch evidence floor in
    /// bytes, default `131_072`; per-work rates now come from the chain, not
    /// an env-configured file).
    pub fn from_env() -> Result<Config, ConfigError> {
        // Default RPC points at a local Anvil node.
        let rpc_url =
            std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        // Signing key and epoch are mandatory — there is no safe default.
        let private_key = req("PRIVATE_KEY")?;
        let epoch: u64 = req("EPOCH")?
            .parse()
            .map_err(|_| ConfigError::Invalid("EPOCH".into()))?;
        // `DISCLOSURE` is now the MODE SELECTOR: set → legacy disclosure mode,
        // unset → proven-weights event mode (the ZK path). It is no longer
        // required, so a missing value is `None`, not an error.
        let disclosure_path = std::env::var("DISCLOSURE").ok().map(PathBuf::from);

        // The deployments file is read to discover contract addresses.
        let deployments_path = std::env::var("DEPLOYMENTS")
            .unwrap_or_else(|_| "chain/deployments/localhost.json".to_string());
        let raw = std::fs::read_to_string(&deployments_path)
            .map_err(|e| ConfigError::Deployments(deployments_path.clone(), e.to_string()))?;
        let deployments: Deployments = serde_json::from_str(&raw)
            .map_err(|e| ConfigError::Deployments(deployments_path, e.to_string()))?;

        // The output path defaults to a per-epoch file under chain/out.
        let out_path = std::env::var("OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("chain/out/epoch-{epoch}-proofs.json")));

        // A receipt bundle is optional; without one, bandwidth stays neutral.
        let receipts_path = std::env::var("RECEIPTS").ok().map(PathBuf::from);

        // The absolute evidence floor. One whole chunk by default: the smallest
        // statement that means anything is "at least one complete block of a
        // real work reached this user this epoch".
        let min_epoch_bytes: u64 = match std::env::var("MIN_EPOCH_BYTES") {
            Ok(v) => v
                .parse()
                .map_err(|_| ConfigError::Invalid("MIN_EPOCH_BYTES".into()))?,
            Err(_) => 131_072,
        };

        Ok(Config {
            rpc_url,
            private_key,
            epoch,
            disclosure_path,
            out_path,
            deployments,
            receipts_path,
            min_epoch_bytes,
        })
    }
}

/// Read a required environment variable or fail with a clear message.
fn req(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::Missing(name.to_string()))
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required environment variable was not set.
    #[error("missing required environment variable: {0}")]
    Missing(String),
    /// An environment variable held an unparseable value.
    #[error("invalid value for environment variable: {0}")]
    Invalid(String),
    /// The deployments file could not be read or parsed.
    #[error("loading deployments file {0}: {1}")]
    Deployments(String, String),
}
