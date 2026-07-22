//! Usage commitments (plan decision D2).
//!
//! Instead of revealing raw usage on-chain, a user submits, per work, a *hiding
//! commitment* `keccak256(work_id ‖ minutes ‖ plays ‖ salt)`. During settlement
//! the trusted aggregator receives the openings (the `(work_id, minutes, plays,
//! salt)` quadruples) out-of-band and recomputes each commitment to confirm it
//! matches what was submitted. The random salt stops anyone from brute-forcing
//! `minutes`/`plays` from the commitment, and lets the user later reveal a
//! commitment if arbitration ever needs it.
//!
//! Encoding: the pre-image is the tight concatenation of four 32-byte words —
//! the work id, minutes as a big-endian integer, plays as a big-endian integer,
//! and the salt (128 bytes total). Minutes and plays each occupy a full 32-byte
//! word so the layout matches Ethereum's `abi.encodePacked` convention, keeping
//! the door open for an on-chain / in-circuit check later. Binding `plays` (not
//! just `minutes`) into the commitment means a user cannot later claim a
//! different play count for the same disclosed usage than the one they
//! committed to on-chain.

use serde::{Deserialize, Serialize};

use crate::{keccak256, Bytes32};

/// A usage commitment: the keccak256 of an [`Opening`]'s pre-image.
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
    /// Lays out the 128-byte pre-image (work id, 32-byte big-endian minutes,
    /// 32-byte big-endian plays, salt) and hashes it with keccak256.
    pub fn commit(&self) -> Commitment {
        let mut preimage = [0u8; 128];
        // Bytes 0..32: the work id.
        preimage[0..32].copy_from_slice(self.work_id.as_bytes());
        // Bytes 32..64: minutes as a big-endian 256-bit word (value in the low 8 bytes).
        preimage[56..64].copy_from_slice(&self.minutes.to_be_bytes());
        // Bytes 64..96: plays as a big-endian 256-bit word (value in the low 8 bytes).
        preimage[88..96].copy_from_slice(&self.plays.to_be_bytes());
        // Bytes 96..128: the salt.
        preimage[96..128].copy_from_slice(self.salt.as_bytes());
        Commitment(Bytes32(keccak256(&preimage)))
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
}
