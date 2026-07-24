//! Poseidon sponge parameters and the in-circuit/out-of-circuit hashing used for the public usage digest.

use ark_crypto_primitives::sponge::poseidon::{find_poseidon_ark_and_mds, PoseidonConfig};
use ark_crypto_primitives::sponge::{poseidon::PoseidonSponge, CryptographicSponge};
use ark_ff::PrimeField;

use crate::field::{fr_from_bytes32, fr_from_u128, fr_to_bytes32, Fr};
use crate::prove::PublicRow;

/// The single Poseidon configuration shared by the native hash and the in-circuit
/// gadget. Both derive from these exact round/MDS constants, so a commitment made
/// natively verifies inside the circuit bit-for-bit. Parameters: rate 2, capacity
/// 1 (width 3), full/partial rounds standard for a 128-bit-secure BN254 sponge.
pub fn poseidon_config() -> PoseidonConfig<Fr> {
    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5u64; // S-box exponent x^5 (BN254 standard)
    let rate = 2;
    let capacity = 1;
    // `find_poseidon_ark_and_mds` takes the sponge's *rate* alone and internally
    // widens the state by its own fixed capacity of 1 (see
    // `ark_crypto_primitives::sponge::poseidon::traits::find_poseidon_ark_and_mds`,
    // which always builds `rate + 1`-wide rows). Passing `rate + capacity` here
    // would double-count the capacity and produce 4-wide matrices, which then
    // fail `PoseidonConfig::new`'s `rate + capacity == 3` assertion below. So we
    // pass `rate` alone, and use the field's real modulus bit size (254 for
    // BN254's scalar field) rather than a hardcoded guess.
    let (ark, mds) = find_poseidon_ark_and_mds::<Fr>(
        Fr::MODULUS_BIT_SIZE as u64,
        rate,
        full_rounds,
        partial_rounds,
        0,
    );
    PoseidonConfig::new(
        full_rounds as usize,
        partial_rounds as usize,
        alpha,
        mds,
        ark,
        rate,
        capacity,
    )
}

/// Absorb `inputs` and squeeze one field element — the native Poseidon hash used
/// for commitments, pseudonyms, and the public-output digest.
pub fn poseidon_hash(inputs: &[Fr]) -> Fr {
    let mut sponge = PoseidonSponge::new(&poseidon_config());
    sponge.absorb(&inputs.to_vec());
    sponge.squeeze_field_elements(1)[0]
}

/// `Poseidon(work_id, minutes, plays, salt)` — the usage commitment. `work_id`
/// and `salt` are reduced into the field; `minutes`/`plays` lift canonically.
pub fn commitment(work_id: &[u8; 32], minutes: u64, plays: u64, salt: &[u8; 32]) -> [u8; 32] {
    let f = poseidon_hash(&[
        fr_from_bytes32(work_id),
        fr_from_u128(minutes as u128),
        fr_from_u128(plays as u128),
        fr_from_bytes32(salt),
    ]);
    fr_to_bytes32(&f)
}

/// `Poseidon(k_epoch, C)` — the epoch-bound pseudonym.
pub fn pseudonym(k_epoch: &[u8; 32], commitment: &[u8; 32]) -> [u8; 32] {
    let f = poseidon_hash(&[fr_from_bytes32(k_epoch), fr_from_bytes32(commitment)]);
    fr_to_bytes32(&f)
}

/// The single public digest committing to a whole per-epoch submission.
///
/// The preimage is the fixed-length field vector, in this exact order — the
/// in-circuit recomputation in `UsageCircuit` MUST match it element-for-element,
/// or honest proofs will not verify:
///
/// ```text
/// [ epoch,                       // fr_from_u128(epoch)
///   tier,                        // fr_from_bytes32(tier)
///   k_epoch,                     // fr_from_bytes32(k_epoch)
///   pseudonym_0, work_id_0, weight_0,   // row 0
///   pseudonym_1, work_id_1, weight_1,   // row 1
///   ...                                 // through row MAX_WORKS-1
/// ]
/// ```
///
/// Its length is exactly `3 + 3 * MAX_WORKS`. The caller MUST pass exactly
/// `MAX_WORKS` rows — real rows first, then canonical padding rows (`work_id = 0`,
/// `commitment = padded_commitment`, `pseudonym = Poseidon(k_epoch, padded_commitment)`,
/// `weight = 0`). Only `pseudonym`, `work_id` and `weight` of each row are hashed;
/// the per-row `commitment` is not part of the preimage.
pub fn digest(epoch: u64, tier: &[u8; 32], k_epoch: &[u8; 32], rows: &[PublicRow]) -> [u8; 32] {
    // The preimage always has the same shape so the verifying key is stable.
    let mut inputs = Vec::with_capacity(3 + 3 * crate::MAX_WORKS);
    inputs.push(fr_from_u128(epoch as u128)); // epoch lifts canonically into the field
    inputs.push(fr_from_bytes32(tier)); // tier reduces mod order like any 256-bit id
    inputs.push(fr_from_bytes32(k_epoch)); // the shared per-epoch key
    for row in rows {
        // Per-row order: pseudonym, work_id, weight — the circuit repeats this order.
        inputs.push(fr_from_bytes32(&row.pseudonym));
        inputs.push(fr_from_bytes32(&row.work_id));
        inputs.push(fr_from_u128(row.weight));
    }
    fr_to_bytes32(&poseidon_hash(&inputs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same opening always yields the same commitment; changing plays changes it.
    #[test]
    fn commitment_is_deterministic_and_binds_plays() {
        let w = [1u8; 32];
        let s = [9u8; 32];
        let c1 = commitment(&w, 60, 3, &s);
        assert_eq!(c1, commitment(&w, 60, 3, &s));
        assert_ne!(c1, commitment(&w, 60, 4, &s)); // plays bound
        assert_ne!(c1, commitment(&w, 61, 3, &s)); // minutes bound
        assert_ne!(c1, commitment(&w, 60, 3, &[8u8; 32])); // salt hides
    }

    /// Pseudonym depends on both the epoch key and the commitment.
    #[test]
    fn pseudonym_binds_epoch_and_commitment() {
        let c = commitment(&[1u8; 32], 10, 1, &[2u8; 32]);
        let p = pseudonym(&[7u8; 32], &c);
        assert_ne!(p, pseudonym(&[8u8; 32], &c)); // epoch-bound (anti-replay)
        assert_ne!(p, c);
    }
}
