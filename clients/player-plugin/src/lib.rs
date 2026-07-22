//! The desktop player agent library: decode, recognise, account, settle.
//!
//! The binary (`src/main.rs`) is a thin CLI over these modules; keeping the
//! logic in a library lets each piece be unit-tested in isolation.

pub mod config;

/// The crate-wide error type surfaced by the CLI.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// A configuration problem.
    #[error(transparent)]
    Config(#[from] config::ConfigError),
}
