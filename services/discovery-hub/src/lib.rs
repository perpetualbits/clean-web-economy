//! Discovery Hub for the Clean Web Economy (Phase 2).
//!
//! Indexes creator-signed, chain-verified work manifests and serves
//! privacy-preserving fingerprint resolution and basic search.

#![forbid(unsafe_code)]

pub mod api;
pub mod chain;
pub mod config;
pub mod index;
pub mod manifest;
