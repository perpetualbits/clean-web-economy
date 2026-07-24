//! Zero-knowledge usage-proof circuits (H2 cycle 1).
//!
//! A Groth16/BN254 circuit proving a user's per-epoch usage is well-formed,
//! range-bounded, diminishing-returns-capped, epoch-bound, and per-work unique,
//! exposing only a single Poseidon digest on-chain. See
//! `docs/superpowers/specs/2026-07-24-zk-usage-proofs-design.md`.

pub mod circuit;
pub mod dr;
pub mod evm;
pub mod field;
pub mod poseidon;
pub mod prove;
pub mod setup;

/// Maximum works a single proof covers; heavier users submit multiple proofs.
pub const MAX_WORKS: usize = 16;
/// In-circuit cap on repeat plays; `d_ppm` is near its floor by here, so the
/// bounded lookup table stays small without materially changing payouts.
pub const MAX_PLAYS_CIRCUIT: u64 = 64;
