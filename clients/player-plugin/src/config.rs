//! Player-agent configuration, assembled from environment variables.
//!
//! `play`/`status` need only `HUB_URL` (+ optional `THRESHOLD`, `STATE`); the
//! chain fields (`PRIVATE_KEY`, `CONSUMPTION`, `TIER_ID`) are required only by
//! `settle`, which validates their presence before sending any transaction. The
//! variable names mirror the settlement job and hub so one devnet's environment
//! carries across every tool.

use std::path::PathBuf;

/// Fully-resolved player configuration.
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Discovery Hub base URL (recognition).
    pub hub_url: String,
    /// JSON-RPC endpoint (settle).
    pub rpc_url: String,
    /// The agent's wallet key — it is the listener/user (settle only).
    pub private_key: Option<String>,
    /// `CWEConsumption` contract address (settle only).
    pub consumption: Option<String>,
    /// The `bytes32` tier id the agent submits under (settle only).
    pub tier_id: Option<String>,
    /// Optional price-per-minute cap; `None` allows any price.
    pub threshold: Option<u64>,
    /// Where the session snapshot is persisted between invocations.
    pub state_path: PathBuf,
    /// Where `settle` writes the disclosure (openings + escrow_works).
    pub disclosure_path: PathBuf,
}

impl PlayerConfig {
    /// Build a config from the process environment.
    pub fn from_env() -> Result<PlayerConfig, ConfigError> {
        Self::from_map(&|k| std::env::var(k).ok())
    }

    /// Build a config from an arbitrary lookup, so tests need not touch the real
    /// environment. `get` returns the value for a variable name, or `None`.
    pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<PlayerConfig, ConfigError> {
        // HUB_URL is the one variable every subcommand needs.
        let hub_url = get("HUB_URL").ok_or_else(|| ConfigError::Missing("HUB_URL".into()))?;
        // A default RPC keeps the common local-devnet case zero-config.
        let rpc_url = get("RPC_URL").unwrap_or_else(|| "http://127.0.0.1:8545".to_string());
        // THRESHOLD, when present, must parse; a typo should fail loudly.
        let threshold = match get("THRESHOLD") {
            Some(s) => Some(
                s.parse::<u64>()
                    .map_err(|_| ConfigError::Invalid("THRESHOLD".into()))?,
            ),
            None => None,
        };
        // State/disclosure default under the system temp dir for a fresh run.
        let state_path = get("STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("cwe-player-state.json"));
        let disclosure_path = get("DISCLOSURE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("cwe-player-disclosure.json"));
        Ok(PlayerConfig {
            hub_url,
            rpc_url,
            private_key: get("PRIVATE_KEY"),
            consumption: get("CONSUMPTION"),
            tier_id: get("TIER_ID"),
            threshold,
            state_path,
            disclosure_path,
        })
    }

    /// Assert the fields `settle` needs are present, returning them together.
    /// Returns `(private_key, consumption, tier_id)` or a clear error naming the
    /// first missing variable, so no transaction is attempted half-configured.
    pub fn require_chain(&self) -> Result<(&str, &str, &str), ConfigError> {
        let pk = self
            .private_key
            .as_deref()
            .ok_or_else(|| ConfigError::Missing("PRIVATE_KEY".into()))?;
        let cons = self
            .consumption
            .as_deref()
            .ok_or_else(|| ConfigError::Missing("CONSUMPTION".into()))?;
        let tier = self
            .tier_id
            .as_deref()
            .ok_or_else(|| ConfigError::Missing("TIER_ID".into()))?;
        Ok((pk, cons, tier))
    }
}

/// Errors assembling the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable was absent.
    #[error("missing required environment variable: {0}")]
    Missing(String),
    /// A variable held an unparseable value.
    #[error("invalid value for environment variable: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config built from an explicit map resolves defaults and parses values.
    #[test]
    fn from_map_defaults_and_parses() {
        let mut env = std::collections::HashMap::new();
        env.insert("HUB_URL".to_string(), "http://hub.test".to_string());
        env.insert("THRESHOLD".to_string(), "500".to_string());
        let cfg = PlayerConfig::from_map(&|k| env.get(k).cloned()).unwrap();
        assert_eq!(cfg.hub_url, "http://hub.test");
        assert_eq!(cfg.threshold, Some(500));
        assert_eq!(cfg.rpc_url, "http://127.0.0.1:8545"); // default
        assert!(cfg.private_key.is_none()); // only needed for settle
    }

    /// A missing HUB_URL is a clear error.
    #[test]
    fn missing_hub_url_errors() {
        let err = PlayerConfig::from_map(&|_| None).unwrap_err();
        assert!(matches!(err, ConfigError::Missing(ref k) if k == "HUB_URL"));
    }

    /// A non-numeric THRESHOLD is rejected rather than silently ignored.
    #[test]
    fn bad_threshold_errors() {
        let env = |k: &str| {
            if k == "HUB_URL" {
                Some("h".to_string())
            } else if k == "THRESHOLD" {
                Some("abc".to_string())
            } else {
                None
            }
        };
        assert!(matches!(
            PlayerConfig::from_map(&env).unwrap_err(),
            ConfigError::Invalid(_)
        ));
    }
}
