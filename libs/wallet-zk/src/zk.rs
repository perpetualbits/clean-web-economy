//! The zero-knowledge proof seam (plan decision D2).
//!
//! Phase 1 does not run ZK circuits; it accounts usage with the [`crate::commit`]
//! commitments and a single trusted aggregator. This module keeps the *shape* of
//! the eventual proof API — [`generate_proof`] / [`verify_proof`] — so that when
//! real circuits arrive (see `docs/specs/zk_usage_proof_requirements.md`) they
//! slot in without changing callers. The placeholder scheme is tagged `none-v0`
//! so a real proof can never be mistaken for it.

use serde::{Deserialize, Serialize};

use crate::Bytes32;

/// The scheme tag carried by every Phase 1 placeholder proof.
pub const PLACEHOLDER_SCHEME: &str = "none-v0";

/// One work's usage: which work, and how many minutes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct UsageEntry {
    /// The work consumed.
    pub work_id: Bytes32,
    /// Minutes of usage.
    pub minutes: u64,
}

/// A usage proof. In Phase 1 this carries no cryptographic content — only the
/// public totals a real proof would attest to — and is used solely to exercise
/// the submission/verification path end to end.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Proof {
    /// Scheme identifier; always [`PLACEHOLDER_SCHEME`] for Phase 1.
    pub scheme: String,
    /// Number of works the proof covers (must match the commitment count).
    pub work_count: usize,
    /// Sum of minutes across all works (a public output a real circuit would prove).
    pub total_minutes: u64,
}

/// Produce a placeholder proof describing `usage`.
///
/// A real implementation would prove, in zero knowledge, that the committed
/// per-work minutes are non-negative and sum within the tier allowance. Here we
/// simply record the public totals.
pub fn generate_proof(usage: &[UsageEntry]) -> Proof {
    // Sum minutes with saturating addition so a pathological input can never panic.
    let total_minutes = usage
        .iter()
        .fold(0u64, |acc, e| acc.saturating_add(e.minutes));
    Proof {
        scheme: PLACEHOLDER_SCHEME.to_string(),
        work_count: usage.len(),
        total_minutes,
    }
}

/// Structurally validate a placeholder proof against the commitments it accompanies.
///
/// Phase 1 can only check consistency, not cryptographic soundness: the scheme
/// must be the placeholder tag and the proof must cover exactly as many works as
/// there are commitments. This mirrors the accept-all on-chain verifier.
pub fn verify_proof(commitments: &[crate::commit::Commitment], proof: &Proof) -> bool {
    proof.scheme == PLACEHOLDER_SCHEME && proof.work_count == commitments.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::{Commitment, Opening};

    fn b32(fill: u8) -> Bytes32 {
        Bytes32([fill; 32])
    }

    /// A generated proof records the work count and the summed minutes.
    #[test]
    fn generate_records_totals() {
        let usage = [
            UsageEntry {
                work_id: b32(1),
                minutes: 30,
            },
            UsageEntry {
                work_id: b32(2),
                minutes: 12,
            },
        ];
        let proof = generate_proof(&usage);
        assert_eq!(proof.scheme, PLACEHOLDER_SCHEME);
        assert_eq!(proof.work_count, 2);
        assert_eq!(proof.total_minutes, 42);
    }

    /// A matching proof/commitments pair verifies; a mismatched count does not.
    #[test]
    fn verify_checks_scheme_and_count() {
        let commitments: Vec<Commitment> = vec![
            Opening::new(b32(1), 30, b32(9)).commit(),
            Opening::new(b32(2), 12, b32(9)).commit(),
        ];
        let good = generate_proof(&[
            UsageEntry {
                work_id: b32(1),
                minutes: 30,
            },
            UsageEntry {
                work_id: b32(2),
                minutes: 12,
            },
        ]);
        assert!(verify_proof(&commitments, &good));

        // A proof covering the wrong number of works is rejected.
        let bad = generate_proof(&[UsageEntry {
            work_id: b32(1),
            minutes: 30,
        }]);
        assert!(!verify_proof(&commitments, &bad));

        // A proof with the wrong scheme tag is rejected.
        let mut wrong_scheme = good.clone();
        wrong_scheme.scheme = "groth16".to_string();
        assert!(!verify_proof(&commitments, &wrong_scheme));
    }
}
