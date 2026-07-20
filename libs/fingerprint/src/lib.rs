//! CWE fingerprint library — **Phase 1 deterministic stub**.
//!
//! A *work fingerprint* is a stable, opaque identifier for a piece of media: a
//! `fp:`-prefixed 256-bit hex string (e.g. `fp:9f86d0…`). The wider system uses
//! it to recognise a work regardless of container format, credit the right
//! creator, and anchor commitments (see `docs/specs/fingerprinting_specification.md`).
//!
//! # What this crate does in Phase 1
//!
//! It computes the fingerprint as the **SHA-256 of the raw sample bytes**
//! (decision D3 in `docs/plans/phase1_mvp_music_implementation_plan.md`). This is
//! deliberately a *cryptographic* hash, **not** a perceptual one: two acoustically
//! identical recordings with different byte layouts (a re-encode, a trim, added
//! noise) produce *different* fingerprints. The real system needs a perceptual
//! fingerprint that survives those transforms (spec §5); that is Phase 2 work
//! tracked in `docs/issues/004-fingerprint-tests.md`.
//!
//! The public API — [`Fingerprint::compute`] and [`compare`] — is shaped now so
//! that swapping the stub for a real perceptual pipeline later changes only the
//! internals of this crate, never its callers. In particular [`compare`] returns
//! a *similarity score* in `[0.0, 1.0]` (the stub yields exactly `1.0` or `0.0`)
//! so callers can already treat similarity as a threshold, as spec §6 requires.

#![forbid(unsafe_code)] // this crate is pure hashing; no unsafe is ever justified

use std::fmt;

use sha2::{Digest, Sha256};

/// Textual prefix every CWE fingerprint carries. It namespaces the identifier so
/// a fingerprint is never confused with a bare hash or some other 64-hex value.
pub const PREFIX: &str = "fp:";

/// Length of the underlying digest in bytes. 256 bits is the spec's default
/// fingerprint width (spec §4) and matches SHA-256's output size.
const DIGEST_LEN: usize = 32;

/// Number of hex characters in the textual form: two per digest byte.
const HEX_LEN: usize = DIGEST_LEN * 2;

/// A work fingerprint: a 256-bit digest with a canonical `fp:<64 hex>` rendering.
///
/// The value is stored as raw bytes rather than a string so equality and hashing
/// are cheap and unambiguous; the textual form is produced on demand via
/// [`Display`](fmt::Display) / [`Fingerprint::to_hex`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    /// The raw 32-byte digest. Never exposed mutably; a fingerprint is immutable
    /// once computed or parsed.
    digest: [u8; DIGEST_LEN],
}

impl Fingerprint {
    /// Compute the Phase 1 fingerprint of a buffer of raw sample bytes.
    ///
    /// The bytes are hashed with SHA-256 exactly as given, so the result is fully
    /// deterministic: the same input always yields the same fingerprint, on any
    /// platform. Callers holding decoded audio as `f32` samples should use
    /// [`Fingerprint::compute_f32`] instead, which fixes a byte ordering so the
    /// result is stable across architectures.
    pub fn compute(samples: &[u8]) -> Fingerprint {
        // Feed the whole buffer through SHA-256 in one shot.
        let mut hasher = Sha256::new();
        hasher.update(samples);
        // `finalize` returns a fixed-size `GenericArray`; copy it into our own
        // array so the type owns its bytes and carries no external dependency.
        let out = hasher.finalize();
        let mut digest = [0u8; DIGEST_LEN];
        digest.copy_from_slice(&out);
        Fingerprint { digest }
    }

    /// Compute the fingerprint of decoded floating-point audio samples.
    ///
    /// Each sample is serialised to its little-endian IEEE-754 byte representation
    /// before hashing. Pinning the byte order here guarantees that the same audio
    /// produces the same fingerprint on both little- and big-endian machines,
    /// which a naive reinterpret-cast of the `f32` slice would not.
    pub fn compute_f32(samples: &[f32]) -> Fingerprint {
        let mut hasher = Sha256::new();
        for sample in samples {
            // `to_le_bytes` is architecture-independent, so the digest is stable.
            hasher.update(sample.to_le_bytes());
        }
        let out = hasher.finalize();
        let mut digest = [0u8; DIGEST_LEN];
        digest.copy_from_slice(&out);
        Fingerprint { digest }
    }

    /// Borrow the raw 32-byte digest, e.g. to embed it in an on-chain commitment.
    pub fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.digest
    }

    /// Render just the 64-character hex digest, *without* the `fp:` prefix.
    /// Use [`Display`](fmt::Display) (or `.to_string()`) for the full `fp:<hex>` form.
    pub fn to_hex(&self) -> String {
        hex::encode(self.digest)
    }

    /// Parse a fingerprint from its canonical `fp:<64 hex>` textual form.
    ///
    /// Validation is strict: the string must start with the [`PREFIX`], the
    /// remainder must be exactly [`HEX_LEN`] characters, and every one of those
    /// characters must be a hex digit. Any deviation returns a [`FingerprintError`]
    /// rather than a best-effort guess, so malformed identifiers fail loudly.
    pub fn parse(s: &str) -> Result<Fingerprint, FingerprintError> {
        // Strip the mandatory prefix; its absence is itself an error.
        let hex_part = s
            .strip_prefix(PREFIX)
            .ok_or(FingerprintError::MissingPrefix)?;
        // A wrong length is reported explicitly so the caller can see what it got.
        if hex_part.len() != HEX_LEN {
            return Err(FingerprintError::BadLength {
                expected: HEX_LEN,
                found: hex_part.len(),
            });
        }
        // Decode; any non-hex character surfaces here as `NotHex`.
        let bytes = hex::decode(hex_part).map_err(|_| FingerprintError::NotHex)?;
        let mut digest = [0u8; DIGEST_LEN];
        // Length was already checked, so this copy cannot panic.
        digest.copy_from_slice(&bytes);
        Ok(Fingerprint { digest })
    }
}

/// Renders the fingerprint in its canonical, prefixed textual form: `fp:<64 hex>`.
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.to_hex())
    }
}

/// Debug uses the same canonical form; the digest bytes are opaque, so the hex
/// string is the most useful representation in logs and test failures.
impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

/// Compare two fingerprints, returning a similarity score in `[0.0, 1.0]`.
///
/// In Phase 1 the score is binary — `1.0` when the fingerprints are byte-for-byte
/// equal, `0.0` otherwise — because the stub cannot judge perceptual closeness.
/// The signature nonetheless matches spec §6's notion of a graded similarity, so
/// callers can already apply a threshold (`sim >= T_duplicate`) and keep working
/// unchanged once the real perceptual comparator replaces this function.
pub fn compare(a: &Fingerprint, b: &Fingerprint) -> f64 {
    if a == b {
        1.0
    } else {
        0.0
    }
}

/// Errors that can arise when parsing a fingerprint from text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FingerprintError {
    /// The string did not begin with the required `fp:` prefix.
    #[error("fingerprint must start with the 'fp:' prefix")]
    MissingPrefix,
    /// The hex portion had the wrong number of characters.
    #[error("fingerprint hex must be {expected} chars, found {found}")]
    BadLength {
        /// How many hex characters a valid fingerprint has.
        expected: usize,
        /// How many were actually supplied.
        found: usize,
    },
    /// The hex portion contained a non-hexadecimal character.
    #[error("fingerprint contains a non-hexadecimal character")]
    NotHex,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same input must always hash to the same fingerprint — the core
    /// property that makes fingerprints usable as stable work identifiers.
    #[test]
    fn compute_is_deterministic() {
        let a = Fingerprint::compute(b"the same bytes");
        let b = Fingerprint::compute(b"the same bytes");
        assert_eq!(a, b);
    }

    /// Different inputs must (overwhelmingly) produce different fingerprints.
    #[test]
    fn distinct_inputs_differ() {
        let a = Fingerprint::compute(b"track one");
        let b = Fingerprint::compute(b"track two");
        assert_ne!(a, b);
    }

    /// The textual form must be exactly `fp:` followed by 64 lowercase hex chars,
    /// matching a known SHA-256 test vector (`""` → e3b0c4…).
    #[test]
    fn display_format_is_canonical() {
        let fp = Fingerprint::compute(b"");
        let text = fp.to_string();
        assert!(text.starts_with("fp:"));
        assert_eq!(text.len(), PREFIX.len() + HEX_LEN);
        assert_eq!(
            text,
            "fp:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// A fingerprint must survive a render → parse round-trip unchanged.
    #[test]
    fn parse_round_trips_display() {
        let fp = Fingerprint::compute(b"round trip me");
        let parsed = Fingerprint::parse(&fp.to_string()).expect("valid form must parse");
        assert_eq!(fp, parsed);
    }

    /// `compute_f32` must be endian-stable and deterministic for `f32` input.
    #[test]
    fn compute_f32_is_deterministic() {
        let samples = [0.0f32, 0.5, -0.25, 1.0];
        assert_eq!(
            Fingerprint::compute_f32(&samples),
            Fingerprint::compute_f32(&samples)
        );
    }

    /// The stub comparator scores identical fingerprints 1.0 and different ones 0.0.
    #[test]
    fn compare_is_binary() {
        let a = Fingerprint::compute(b"x");
        let a2 = Fingerprint::compute(b"x");
        let b = Fingerprint::compute(b"y");
        assert_eq!(compare(&a, &a2), 1.0);
        assert_eq!(compare(&a, &b), 0.0);
    }

    /// Each malformed textual form must yield the matching, specific error.
    #[test]
    fn parse_rejects_malformed_input() {
        // Missing prefix.
        assert_eq!(
            Fingerprint::parse("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            Err(FingerprintError::MissingPrefix)
        );
        // Right prefix, too short.
        assert_eq!(
            Fingerprint::parse("fp:abcd"),
            Err(FingerprintError::BadLength {
                expected: HEX_LEN,
                found: 4
            })
        );
        // Right length, but 'z' is not a hex digit (63 valid chars + one 'z').
        let mut bad = "fp:".to_string();
        bad.push_str(&"a".repeat(HEX_LEN - 1));
        bad.push('z');
        assert_eq!(Fingerprint::parse(&bad), Err(FingerprintError::NotHex));
    }
}
