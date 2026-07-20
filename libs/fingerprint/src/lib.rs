//! CWE fingerprint library — **Phase 1 deterministic stub**.
//!
//! This crate turns a buffer of decoded audio samples into a stable, opaque
//! *work fingerprint*: a `fp:`-prefixed 256-bit hex identifier. In Phase 1 the
//! fingerprint is simply the SHA-256 of the raw sample bytes (decision D3 in
//! `docs/plans/phase1_mvp_music_implementation_plan.md`). That is deliberately
//! NOT a perceptual fingerprint — two acoustically identical encodings with
//! different byte layouts will not match. Real perceptual fingerprinting is
//! deferred to Phase 2 (see `docs/specs/fingerprinting_specification.md` and
//! `docs/issues/004-fingerprint-tests.md`); the [`compute`]/[`compare`] API is
//! shaped now so that only the internals change later.
//!
//! The real implementation lands in WP3; this file is the WP0 skeleton that
//! establishes the crate so the rest of the workspace can build against it.

#![forbid(unsafe_code)] // this crate is pure hashing; no unsafe is ever justified

// (WP3 adds the public `compute`/`compare` API and its determinism, format, and
// distinctness tests here. WP0 only establishes the crate so the workspace builds.)
