//! Runtime configuration for the `cwe-hub` server, loaded from the environment.

use std::path::PathBuf;

use alloy::primitives::Address;

/// Server configuration assembled from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP listener binds to.
    pub bind: String,
    /// JSON-RPC URL for the chain the registry is deployed on.
    pub rpc_url: String,
    /// The `CWERegistry` contract address, cross-checked on every ingest.
    pub registry: Address,
    /// Path to the index snapshot file, loaded at startup and rewritten on every ingest.
    pub snapshot: PathBuf,
}

/// Errors loading [`Config`] from the environment.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `REGISTRY` was not set; unlike the other variables it has no safe default.
    #[error("REGISTRY environment variable is required")]
    MissingRegistry,
    /// `REGISTRY` was set but is not a valid `0x`-prefixed address.
    #[error("REGISTRY is not a valid address: {0}")]
    BadRegistry(String),
}

impl Config {
    /// Read configuration from the environment, applying defaults for `BIND`,
    /// `RPC_URL`, and `SNAPSHOT`. `REGISTRY` is required.
    pub fn from_env() -> Result<Config, ConfigError> {
        let bind = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
        let rpc_url =
            std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        let registry_raw = std::env::var("REGISTRY").map_err(|_| ConfigError::MissingRegistry)?;
        let registry: Address = registry_raw
            .parse()
            .map_err(|_| ConfigError::BadRegistry(registry_raw))?;
        let snapshot = std::env::var("SNAPSHOT").unwrap_or_else(|_| "hub-index.json".to_string());
        Ok(Config {
            bind,
            rpc_url,
            registry,
            snapshot: PathBuf::from(snapshot),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the environment mutations in this test module so they cannot
    /// race with each other under the default parallel test runner.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Clear every variable `Config::from_env` reads.
    fn clear_env() {
        for var in ["BIND", "RPC_URL", "REGISTRY", "SNAPSHOT"] {
            std::env::remove_var(var);
        }
    }

    /// Without `REGISTRY` set, loading configuration fails with a clear error.
    #[test]
    fn missing_registry_is_an_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::MissingRegistry)
        ));
    }

    /// Defaults apply for everything but `REGISTRY`, which is required.
    #[test]
    fn defaults_apply_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("REGISTRY", "0x0000000000000000000000000000000000000001");
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.bind, "127.0.0.1:8080");
        assert_eq!(cfg.rpc_url, "http://127.0.0.1:8545");
        assert_eq!(cfg.snapshot, PathBuf::from("hub-index.json"));
        clear_env();
    }

    /// An invalid `REGISTRY` value is rejected rather than silently accepted.
    #[test]
    fn bad_registry_is_an_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();
        std::env::set_var("REGISTRY", "not-an-address");
        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::BadRegistry(_))
        ));
        clear_env();
    }
}
