//! CWE wallet & session-accounting library (WP4).
//!
//! This crate provides the client-side primitives of the Clean Web Economy:
//!
//! * **Commitments** — `keccak256(work_id ‖ minutes ‖ salt)`, the hiding
//!   commitment a user submits on-chain instead of raw usage (decision D2). The
//!   keccak256 primitive is chosen to match the contracts exactly, so an
//!   off-chain commitment and its on-chain verification always agree.
//! * **ZK seam** — a `generateProof`/`verifyProof` shape that Phase 1 fills
//!   with a structural `none-v0` placeholder; real ZK circuits replace only
//!   this module later (see `docs/specs/zk_usage_proof_requirements.md`).
//! * **SessionStore** — epoch-aware accrual of listening minutes
//!   (start / add-time / stop / flush), storage-agnostic so the extension can
//!   back it with `chrome.storage` while tests use an in-memory store.
//!
//! The real implementations land in WP4; this file is the WP0 skeleton so the
//! settlement job and extension can name this crate as a dependency.

#![forbid(unsafe_code)] // no unsafe: this is hashing, accounting, and serialisation only

// (WP4 adds Commitments, the ZK seam, and SessionStore here, with tests for
// accrual, flush semantics, and commitment determinism. WP0 only establishes
// the crate so the settlement job and extension can depend on it.)
