//! The desktop player agent library: decode, recognise, account, settle.
//!
//! The binary (`src/main.rs`) is a thin CLI over these modules; keeping the
//! logic in a library lets each piece be unit-tested in isolation.

pub mod config;
pub mod decode;
pub mod policy;
pub mod recognize;
pub mod session;

/// The crate-wide error type surfaced by the CLI.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// A configuration problem.
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    /// An audio decode problem.
    #[error(transparent)]
    Decode(#[from] decode::DecodeError),
    /// A session state problem.
    #[error(transparent)]
    Session(#[from] session::SessionError),
}
