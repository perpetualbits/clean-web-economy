//! Usage commitments (plan decision D2).
//!
//! Instead of revealing raw usage on-chain, a user submits, per work, a *hiding
//! commitment* `keccak256(work_id ‖ minutes ‖ salt)`. During settlement the
//! trusted aggregator receives the openings (the `(work_id, minutes, salt)`
//! triples) out-of-band and recomputes each commitment to confirm it matches what
//! was submitted. The random salt stops anyone from brute-forcing `minutes` from
//! the commitment, and lets the user later reveal a commitment if arbitration
//! ever needs it.
//!
//! Encoding: the pre-image is the tight concatenation of the 32-byte work id, the
//! minutes as a 32-byte big-endian integer, and the 32-byte salt (96 bytes total).
//! Minutes use a full 32-byte word so the layout matches Ethereum's `abi.encodePacked`
//! convention, keeping the door open for an on-chain / in-circuit check later.

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

/// The secret pre-image behind a [`Commitment`]: which work, how many minutes, and
/// the random salt. Openings are shared with the aggregator during settlement.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Opening {
    /// The work the usage is for.
    pub work_id: Bytes32,
    /// Minutes of usage being committed to.
    pub minutes: u64,
    /// Random 32-byte salt that hides `minutes` and makes the commitment binding.
    pub salt: Bytes32,
}

impl Opening {
    /// Build an opening from its parts.
    pub fn new(work_id: Bytes32, minutes: u64, salt: Bytes32) -> Self {
        Opening {
            work_id,
            minutes,
            salt,
        }
    }

    /// Compute the commitment this opening hashes to.
    ///
    /// Lays out the 96-byte pre-image (work id, 32-byte big-endian minutes, salt)
    /// and hashes it with keccak256.
    pub fn commit(&self) -> Commitment {
        let mut preimage = [0u8; 96];
        // Bytes 0..32: the work id.
        preimage[0..32].copy_from_slice(self.work_id.as_bytes());
        // Bytes 32..64: minutes as a big-endian 256-bit word (value in the low 8 bytes).
        preimage[56..64].copy_from_slice(&self.minutes.to_be_bytes());
        // Bytes 64..96: the salt.
        preimage[64..96].copy_from_slice(self.salt.as_bytes());
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

    /// The same opening always produces the same commitment.
    #[test]
    fn commit_is_deterministic() {
        let o = Opening::new(b32(0x11), 120, b32(0x22));
        assert_eq!(o.commit(), o.commit());
    }

    /// Changing the minutes changes the commitment (it binds to the value).
    #[test]
    fn minutes_change_commitment() {
        let a = Opening::new(b32(0x11), 120, b32(0x22)).commit();
        let b = Opening::new(b32(0x11), 121, b32(0x22)).commit();
        assert_ne!(a, b);
    }

    /// Changing the salt changes the commitment (it hides the value).
    #[test]
    fn salt_changes_commitment() {
        let a = Opening::new(b32(0x11), 120, b32(0x22)).commit();
        let b = Opening::new(b32(0x11), 120, b32(0x23)).commit();
        assert_ne!(a, b);
    }

    /// An opening verifies against its own commitment and not against another's.
    #[test]
    fn verify_matches_only_correct_opening() {
        let o = Opening::new(b32(0x11), 120, b32(0x22));
        let c = o.commit();
        assert!(o.verify(&c));

        let wrong = Opening::new(b32(0x11), 999, b32(0x22));
        assert!(!wrong.verify(&c));
    }

    /// An opening round-trips through JSON (the disclosure-file format).
    #[test]
    fn opening_json_round_trip() {
        let o = Opening::new(b32(0xAB), 42, b32(0xCD));
        let json = serde_json::to_string(&o).unwrap();
        let back: Opening = serde_json::from_str(&json).unwrap();
        assert_eq!(o, back);
        // Sanity-check the human-readable hex encoding is present.
        assert!(json.contains("0xabab"));
    }
}
