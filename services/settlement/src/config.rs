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
    /// Path to the disclosure file with the users' openings.
    pub disclosure_path: PathBuf,
    /// Where to write the withdrawal-proofs output.
    pub out_path: PathBuf,
    /// The deployed contract addresses.
    pub deployments: Deployments,
}

impl Config {
    /// Assemble the configuration from environment variables.
    ///
    /// Recognised variables (with defaults suited to a local Anvil devnet):
    /// `RPC_URL`, `PRIVATE_KEY` (required), `EPOCH` (required), `DISCLOSURE`
    /// (required), `DEPLOYMENTS` (default `chain/deployments/localhost.json`),
    /// `OUT` (default `chain/out/epoch-<n>-proofs.json`).
    pub fn from_env() -> Result<Config, ConfigError> {
        // Default RPC points at a local Anvil node.
        let rpc_url =
            std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        // Signing key and epoch are mandatory — there is no safe default.
        let private_key = req("PRIVATE_KEY")?;
        let epoch: u64 = req("EPOCH")?
            .parse()
            .map_err(|_| ConfigError::Invalid("EPOCH".into()))?;
        let disclosure_path = PathBuf::from(req("DISCLOSURE")?);

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

        Ok(Config {
            rpc_url,
            private_key,
            epoch,
            disclosure_path,
            out_path,
            deployments,
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
