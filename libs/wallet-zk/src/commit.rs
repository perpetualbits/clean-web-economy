//! Usage commitments (plan decision D2).
//!
//! Instead of revealing raw usage on-chain, a user submits, per work, a *hiding
//! commitment* `Poseidon(work_id, minutes, plays, salt)`. During settlement
//! the trusted aggregator receives the openings (the `(work_id, minutes, plays,
//! salt)` quadruples) out-of-band and recomputes each commitment to confirm it
//! matches what was submitted. The random salt stops anyone from brute-forcing
//! `minutes`/`plays` from the commitment, and lets the user later reveal a
//! commitment if arbitration ever needs it.
//!
//! Hashing: the commitment is the single Poseidon sponge hash of four field
//! elements — `work_id` and `salt` reduced into the field, and `minutes`/
//! `plays` lifted canonically — computed by [`cwe_zk_circuits::poseidon`].
//! Poseidon (rather than keccak256) is what makes the commitment provable
//! *inside* the H2 usage-proof circuit: the same hash, with the same
//! parameters, runs both natively here and as an in-circuit gadget, so a
//! commitment made by the wallet and one proved by the circuit agree
//! bit-for-bit. Binding `plays` (not just `minutes`) into the commitment means
//! a user cannot later claim a different play count for the same disclosed
//! usage than the one they committed to on-chain.

use serde::{Deserialize, Serialize};

use crate::Bytes32;

/// A usage commitment: the Poseidon hash of an [`Opening`]'s four field elements.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Commitment(pub Bytes32);

impl Commitment {
    /// The commitment's raw 32 bytes (what is submitted on-chain as `bytes32`).
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

/// The secret pre-image behind a [`Commitment`]: which work, how many minutes,
/// how many plays, and the random salt. Openings are shared with the aggregator
/// during settlement.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Opening {
    /// The work the usage is for.
    pub work_id: Bytes32,
    /// Minutes of usage being committed to.
    pub minutes: u64,
    /// Number of plays of the work being committed to.
    pub plays: u64,
    /// Random 32-byte salt that hides `minutes`/`plays` and makes the commitment binding.
    pub salt: Bytes32,
}

impl Opening {
    /// Build an opening from its parts.
    pub fn new(work_id: Bytes32, minutes: u64, plays: u64, salt: Bytes32) -> Self {
        Opening {
            work_id,
            minutes,
            plays,
            salt,
        }
    }

    /// Compute the commitment this opening hashes to.
    ///
    /// Delegates to [`cwe_zk_circuits::poseidon::commitment`] — the canonical
    /// Poseidon hash of `(work_id, minutes, plays, salt)` that the H2 circuit
    /// and prover also use, so a commitment made here is exactly what the
    /// circuit can later prove a statement about.
    pub fn commit(&self) -> Commitment {
        let digest = cwe_zk_circuits::poseidon::commitment(
            self.work_id.as_bytes(),
            self.minutes,
            self.plays,
            self.salt.as_bytes(),
        );
        Commitment(Bytes32(digest))
    }

    /// Check that this opening reproduces `commitment`.
    ///
    /// Used by the settlement job to reject any opening that does not match the
    /// commitment the user actually submitted.
    pub fn verify(&self, commitment: &Commitment) -> bool {
        &self.commit() == commitment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A helper to build a `Bytes32` from a single fill byte, for terse tests.
    fn b32(fill: u8) -> Bytes32 {
        Bytes32([fill; 32])
    }

    /// Changing only the play count changes the commitment (plays is bound).
    #[test]
    fn commitment_binds_plays() {
        let o1 = Opening::new(Bytes32([1; 32]), 60, 3, Bytes32([9; 32]));
        let o2 = Opening::new(Bytes32([1; 32]), 60, 4, Bytes32([9; 32])); // only plays differ
        assert_ne!(o1.commit(), o2.commit(), "plays must be bound");
        assert!(o1.verify(&o1.commit()));
    }

    /// The same opening always produces the same commitment.
    #[test]
    fn commit_is_deterministic() {
        let o = Opening::new(b32(0x11), 120, 1, b32(0x22));
        assert_eq!(o.commit(), o.commit());
    }

    /// Changing the minutes changes the commitment (it binds to the value).
    #[test]
    fn minutes_change_commitment() {
        let a = Opening::new(b32(0x11), 120, 1, b32(0x22)).commit();
        let b = Opening::new(b32(0x11), 121, 1, b32(0x22)).commit();
        assert_ne!(a, b);
    }

    /// Changing the salt changes the commitment (it hides the value).
    #[test]
    fn salt_changes_commitment() {
        let a = Opening::new(b32(0x11), 120, 1, b32(0x22)).commit();
        let b = Opening::new(b32(0x11), 120, 1, b32(0x23)).commit();
        assert_ne!(a, b);
    }

    /// An opening verifies against its own commitment and not against another's.
    #[test]
    fn verify_matches_only_correct_opening() {
        let o = Opening::new(b32(0x11), 120, 1, b32(0x22));
        let c = o.commit();
        assert!(o.verify(&c));

        let wrong = Opening::new(b32(0x11), 999, 1, b32(0x22));
        assert!(!wrong.verify(&c));
    }

    /// An opening round-trips through JSON (the disclosure-file format).
    #[test]
    fn opening_json_round_trip() {
        let o = Opening::new(b32(0xAB), 42, 1, b32(0xCD));
        let json = serde_json::to_string(&o).unwrap();
        let back: Opening = serde_json::from_str(&json).unwrap();
        assert_eq!(o, back);
        // Sanity-check the human-readable hex encoding is present.
        assert!(json.contains("0xabab"));
    }

    /// The commitment matches the canonical Poseidon commitment the ZK circuit
    /// and prover use, byte-for-byte — this is what makes `Opening::commit()` a
    /// valid stand-in for the circuit's public commitment.
    #[test]
    fn commit_matches_zk_poseidon() {
        let o = Opening::new(Bytes32([1; 32]), 60, 2, Bytes32([9; 32]));
        let expected = cwe_zk_circuits::poseidon::commitment(&[1u8; 32], 60, 2, &[9u8; 32]);
        assert_eq!(o.commit().0 .0, expected);
    }
}
