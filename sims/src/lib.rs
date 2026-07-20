//! DAPR payout-math reference library (WP2).
//!
//! Implements the Decentralised Aggregated Payout & Reconciliation formula from
//! `sims/README.md` and `docs/specs/DAPR_usage_aggregation_protocol.md`. For a
//! single user with usage rows `i`:
//!
//! ```text
//! D_total = Σ (minutes_i · price_i · region_factor_i)     // the user's total "value"
//! W_i     = (minutes_i · price_i · region_factor_i) / D_total   // per-work weight
//! R_i     = tier_fee · W_i                                 // credit routed to work i
//! ```
//!
//! Credits for the same work are then summed across all users to produce the
//! per-work payout that the settlement job commits on-chain. This crate is the
//! authoritative implementation: WP5's settlement job calls it directly so the
//! simulator and the chain settlement can never disagree numerically.
//!
//! The real implementation (including fixed-point/ppm arithmetic to keep the
//! math exactly reproducible on-chain) lands in WP2; this file is the WP0
//! skeleton establishing the crate.

#![forbid(unsafe_code)] // pure arithmetic and data-shuffling; no unsafe needed

// (WP2 adds the ppm fixed-point payout math and its fairness-invariant property
// tests here. WP0 only establishes the crate so WP5's settlement job can link it.)
