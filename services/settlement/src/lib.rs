//! Off-chain epoch settlement for the Clean Web Economy (WP5).
//!
//! Closes an epoch end to end: read usage submissions from the chain, open their
//! commitments from a disclosure file, run the shared DAPR payout math, build the
//! Merkle tree the payout contract verifies against, commit the root on-chain, and
//! write per-creator withdrawal proofs.
//!
//! The crate is split so that the numerically-critical parts carry no chain
//! dependency:
//! * [`merkle`] and [`settle`] are pure and unit-tested;
//! * [`chain`] holds the alloy RPC/signing code, validated by the WP7 e2e demo.

#![forbid(unsafe_code)]

pub mod chain;
pub mod config;
pub mod disclosure;
pub mod merkle;
pub mod settle;
