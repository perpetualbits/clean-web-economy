//! CWE client-side accounting library (WP4).
//!
//! This crate holds the **portable, chain-agnostic** primitives that the browser
//! extension (WP6) and the off-chain settlement job (WP5) both depend on:
//!
//! * [`commit`] — keccak256 usage commitments and their openings (decision D2).
//! * [`zk`] — the `generate_proof`/`verify_proof` seam that real ZK circuits
//!   replace later; Phase 1 fills it with a structural `none-v0` placeholder.
//! * [`session`] — epoch-aware accrual of listening time (start / add-time /
//!   stop / flush), storage-agnostic so the extension can persist it.
//!
//! # Scope note
//!
//! The `Wallet` signer and the `ChainClient` seam described in dev-spec §4.1 are
//! **not** here: they need a secp256k1/RPC stack that would weigh this portable
//! (wasm-targeted) crate down. They live with the concrete provider integration
//! in the settlement job (WP5) and the extension's chain layer (WP6). This crate
//! stays free of network and heavy crypto dependencies so it compiles cleanly to
//! WebAssembly.
//!
//! keccak256 is used throughout (matching the on-chain hash) so a commitment made
//! here and opened during settlement agree bit-for-bit.

#![forbid(unsafe_code)] // hashing, accounting, and serialisation only — no unsafe

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use tiny_keccak::{Hasher, Keccak};

pub mod commit;
pub mod session;
pub mod zk;

/// Compute the keccak256 digest of `data`.
///
/// This is the same hash the Solidity contracts use, which is why commitments and
/// Merkle leaves built here can be verified on-chain and re-derived during settlement.
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut out = [0u8; 32];
    hasher.finalize(&mut out); // write the 32-byte digest into `out`
    out
}

/// A 32-byte value (work id, salt, commitment, …) with a `0x<64 hex>` text form.
///
/// Serialising as a hex string keeps disclosure/manifest JSON human-readable and
/// interoperable with the hex the chain tooling emits.
///
/// `Ord`/`PartialOrd` (over the raw bytes) let it key a `BTreeMap`, which gives
/// deterministic iteration for reproducible flush output and Merkle trees.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Bytes32(pub [u8; 32]);

impl Bytes32 {
    /// Borrow the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Renders as `0x` followed by 64 lowercase hex characters.
impl fmt::Display for Bytes32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

/// Debug reuses the hex form — the raw bytes are opaque, so hex is most useful.
impl fmt::Debug for Bytes32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes32({self})")
    }
}

/// Errors from parsing a [`Bytes32`] out of text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Bytes32Error {
    /// The decoded byte length was not exactly 32.
    #[error("expected 32 bytes (64 hex chars), found {0}")]
    BadLength(usize),
    /// The string contained a non-hexadecimal character.
    #[error("invalid hex in 32-byte value")]
    NotHex,
}

/// Parses either `0x`-prefixed or bare 64-character hex into a [`Bytes32`].
impl FromStr for Bytes32 {
    type Err = Bytes32Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Accept an optional 0x prefix so both conventions round-trip.
        let hex_part = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(hex_part).map_err(|_| Bytes32Error::NotHex)?;
        if bytes.len() != 32 {
            return Err(Bytes32Error::BadLength(bytes.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes); // length checked above, cannot panic
        Ok(Bytes32(out))
    }
}

/// Serialises as the canonical `0x<hex>` string.
impl Serialize for Bytes32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Deserialises from the `0x<hex>` (or bare hex) string form.
impl<'de> Deserialize<'de> for Bytes32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Bytes32::from_str(&s).map_err(de::Error::custom)
    }
}
