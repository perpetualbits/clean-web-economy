//! The R1CS constraint system for the usage-proof circuit.

use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::convert::ToBitsGadget;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::fields::FieldVar;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::field::{fr_from_bytes32, fr_from_u128, Fr};
use crate::poseidon::{
    commitment as native_commitment, poseidon_config, pseudonym as native_pseudonym,
};
use crate::MAX_PLAYS_CIRCUIT;
#[cfg(test)]
use crate::MAX_WORKS;

/// Bit-width bound on a row's per-epoch `minutes`. 2^32 minutes far exceeds any
/// honest epoch, so this only rejects overflow/garbage while keeping the field
/// value small enough to compose safely in the DR-weight arithmetic (Task 7).
pub const MINUTES_BITS: usize = 32;
/// Bit-width bound on a row's `plays`. 7 bits caps the raw witness at 127; a
/// separate `plays <= MAX_PLAYS_CIRCUIT` gadget tightens it to the DR table's
/// domain `[0, 64]` so the Task-7 lookup index can never fall out of range.
pub const PLAYS_BITS: usize = 7;
/// Bit-width bound on the ppm factors (`price_ppm`, `region_ppm`). 40 bits
/// comfortably covers realistic ppm magnitudes while bounding the product terms.
pub const PPM_BITS: usize = 40;

/// The in-circuit Poseidon hash — absorb `inputs`, squeeze one element — using the
/// exact `poseidon_config()` the native hash uses, guaranteeing agreement.
pub(crate) fn poseidon_hash_gadget(
    cs: ConstraintSystemRef<Fr>,
    inputs: &[FpVar<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    let mut sponge = PoseidonSpongeVar::new(cs, &poseidon_config());
    sponge.absorb(&inputs.to_vec())?; // absorb the whole input vector once, matching the native sponge
    Ok(sponge.squeeze_field_elements(1)?[0].clone())
}

/// One row of a user's per-epoch usage: a work played for `minutes` over `plays`
/// sessions, hidden behind a Poseidon `commitment` and epoch-bound `pseudonym`.
/// `active` distinguishes real rows from the zero-padding that fills a proof out
/// to `MAX_WORKS`. `weight` is the prover's claimed DR-capped value (constrained
/// in Task 7); `salt` and the ppm factors are the commitment opening + payout
/// inputs.
#[derive(Clone, Debug)]
pub struct RowWitness {
    /// Whether this is a real usage row (`true`) or inactive zero-padding (`false`).
    pub active: bool,
    /// The 256-bit work identifier; reduced into the field for the commitment.
    pub work_id: [u8; 32],
    /// Minutes the work was played this epoch (range-bounded to `MINUTES_BITS`).
    pub minutes: u64,
    /// Number of listening sessions (range-bounded, `<= MAX_PLAYS_CIRCUIT`).
    pub plays: u64,
    /// The commitment's hiding salt; reduced into the field like `work_id`.
    pub salt: [u8; 32],
    /// Per-work price in parts-per-million (range-bounded to `PPM_BITS`).
    pub price_ppm: u64,
    /// Regional multiplier in parts-per-million (range-bounded to `PPM_BITS`).
    pub region_ppm: u64,
    /// Prover-claimed diminishing-returns-capped weight (bound in Task 7).
    pub weight: u128,
    /// `Poseidon(work_id, minutes, plays, salt)` — the public usage commitment.
    pub commitment: [u8; 32],
    /// `Poseidon(k_epoch, commitment)` — the epoch-bound pseudonym (bound in Task 7).
    pub pseudonym: [u8; 32],
}

/// The usage-proof statement: for a given `epoch`/`tier` a user attests that their
/// `rows` are well-formed, range-bounded, DR-capped, epoch-bound and unique,
/// exposing only the Poseidon `digest`. This task lands commitment-correctness and
/// range/well-formedness; the DR-cap + pseudonym binding (Task 7) and uniqueness +
/// digest public-input binding (Task 8) extend the same constraint system.
#[derive(Clone, Debug)]
pub struct UsageCircuit {
    /// The epoch this proof covers (public context; bound in later tasks).
    pub epoch: u64,
    /// The subscription tier identifier (public context; bound in later tasks).
    pub tier: [u8; 32],
    /// The per-epoch pseudonym key; mixes into each row's pseudonym (Task 7).
    pub k_epoch: [u8; 32],
    /// The diminishing-returns curvature parameter for the DR table (Task 7).
    pub k_ppm: u64,
    /// The `MAX_WORKS` usage rows (real rows first, zero-padding after).
    pub rows: Vec<RowWitness>,
    /// The public output digest over all rows (bound as a public input in Task 8).
    pub digest: [u8; 32],
}

/// The allocated per-row variables that later tasks reuse. Task 7 consumes
/// `work_id_var`/`weight_var`/`pseudonym_var`/`active_var` for the DR-cap and
/// epoch-binding constraints; `commitment_var` feeds the Task-8 uniqueness and
/// digest binding. Returning them from `enforce_row` keeps the growing constraint
/// system's row logic in one place.
// Fields are produced now but first *read* in Tasks 7-8; keep them wired up so the
// return shape those tasks depend on is stable from this task onward.
#[allow(dead_code)]
struct RowVars {
    /// The work identifier as a field element (`fr_from_bytes32(work_id)`).
    work_id_var: FpVar<Fr>,
    /// The prover-claimed DR-capped weight as a field element.
    weight_var: FpVar<Fr>,
    /// The epoch-bound pseudonym as a field element.
    pseudonym_var: FpVar<Fr>,
    /// The row-active flag.
    active_var: Boolean<Fr>,
    /// The usage commitment as a field element.
    commitment_var: FpVar<Fr>,
}

/// The canonical commitment of the all-zero opening — the value every inactive
/// (padding) row must carry so the eventual digest is deterministic. Computed via
/// the native helper so the circuit and prover agree bit-for-bit.
fn padded_commitment() -> [u8; 32] {
    native_commitment(&[0u8; 32], 0, 0, &[0u8; 32])
}

/// The canonical pseudonym of a padding row: `Poseidon(k_epoch, padded_commitment)`.
/// Inactive rows must carry this so their pseudonym slot is fixed and the digest
/// stays deterministic. (The in-circuit *derivation* of pseudonyms is Task 7; here
/// we only pin the padding value.)
fn padded_pseudonym(k_epoch: &[u8; 32]) -> [u8; 32] {
    native_pseudonym(k_epoch, &padded_commitment())
}

impl UsageCircuit {
    /// Allocate and constrain a single row, returning its reusable `RowVars`.
    ///
    /// Real (`active`) rows must satisfy commitment-correctness, the range bounds,
    /// and `work_id != 0`. Inactive (padding) rows must be the canonical zero row:
    /// `work_id == 0`, `weight == 0`, and the fixed padded commitment/pseudonym.
    /// Every active-only check is gated on `active`, every inactive-only check on
    /// its negation, so one uniform row routine handles both kinds.
    fn enforce_row(
        &self,
        cs: ConstraintSystemRef<Fr>,
        row: &RowWitness,
    ) -> Result<RowVars, SynthesisError> {
        // --- Witness allocation ------------------------------------------------
        // `active` is a boolean witness; every conditional check keys off it.
        let active_var = Boolean::new_witness(cs.clone(), || Ok(row.active))?;
        // `work_id`/`salt`/`commitment`/`pseudonym` are opaque 256-bit values whose
        // only in-circuit use is as field elements, so we allocate each as the
        // single reduced `FpVar` (matching native `fr_from_bytes32`, big-endian
        // reduce mod order). The raw 256-bit preimage is not otherwise constrained
        // this task, so no bit-level decomposition of them is needed.
        let work_id_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&row.work_id)))?;
        let salt_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&row.salt)))?;
        let commitment_var =
            FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&row.commitment)))?;
        let pseudonym_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&row.pseudonym)))?;
        // `minutes`/`plays` lift canonically into the field (never reduced).
        let minutes_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(row.minutes as u128)))?;
        let plays_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(row.plays as u128)))?;
        let price_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(row.price_ppm as u128)))?;
        let region_var =
            FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(row.region_ppm as u128)))?;
        let weight_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(row.weight)))?;

        // The negation drives all inactive-only constraints; clone so `active_var`
        // survives for the return value and the active-only checks.
        let inactive_var = !active_var.clone();

        // --- Commitment correctness (active rows) ------------------------------
        // Recompute the commitment in-circuit from the opening and require it to
        // equal the witnessed commitment. Corrupting `commitment` breaks this.
        let computed_commitment = poseidon_hash_gadget(
            cs.clone(),
            &[
                work_id_var.clone(),
                minutes_var.clone(),
                plays_var.clone(),
                salt_var.clone(),
            ],
        )?;
        commitment_var.conditional_enforce_equal(&computed_commitment, &active_var)?;

        // --- Range / well-formedness bounds (active rows) ----------------------
        // Each value must fit in its declared bit-width; enforced only when active
        // (padding rows carry zeros, which trivially fit anyway).
        enforce_bit_width(&minutes_var, MINUTES_BITS, &active_var)?;
        enforce_bit_width(&plays_var, PLAYS_BITS, &active_var)?;
        enforce_bit_width(&price_var, PPM_BITS, &active_var)?;
        enforce_bit_width(&region_var, PPM_BITS, &active_var)?;

        // `plays <= MAX_PLAYS_CIRCUIT`: the DR lookup (Task 7) indexes `[1, 64]`, so
        // clamp here. We enforce `MAX_PLAYS_CIRCUIT - plays` decomposes into
        // `PLAYS_BITS` bits: for `plays in [0, 64]` the difference is `[0, 64]` and
        // fits; for `plays > 64` it underflows to a near-modulus value that cannot
        // fit in 7 bits, so the constraint rejects it.
        let max_plays = FpVar::constant(fr_from_u128(MAX_PLAYS_CIRCUIT as u128));
        let plays_slack = &max_plays - &plays_var; // MAX_PLAYS_CIRCUIT - plays
        enforce_bit_width(&plays_slack, PLAYS_BITS, &active_var)?;

        // Active rows must reference a real work (non-zero id).
        work_id_var.conditional_enforce_not_equal(&FpVar::zero(), &active_var)?;

        // --- Canonical padding (inactive rows) ---------------------------------
        // Padding rows are pinned to the zero row + fixed padded commitment/
        // pseudonym so the later digest over all rows is deterministic.
        work_id_var.conditional_enforce_equal(&FpVar::zero(), &inactive_var)?;
        weight_var.conditional_enforce_equal(&FpVar::zero(), &inactive_var)?;
        let padded_c = FpVar::constant(fr_from_bytes32(&padded_commitment()));
        let padded_p = FpVar::constant(fr_from_bytes32(&padded_pseudonym(&self.k_epoch)));
        commitment_var.conditional_enforce_equal(&padded_c, &inactive_var)?;
        pseudonym_var.conditional_enforce_equal(&padded_p, &inactive_var)?;

        Ok(RowVars {
            work_id_var,
            weight_var,
            pseudonym_var,
            active_var,
            commitment_var,
        })
    }
}

/// Enforce that `value` fits in `n_bits` bits, but only when `guard` is true.
///
/// `to_bits_le` yields the canonical (constrained) little-endian decomposition of
/// the witnessed value; recomposing just the low `n_bits` and requiring it to
/// equal `value` forces every higher bit to zero — i.e. `value < 2^n_bits`. Gating
/// the equality on `guard` leaves padding rows unconstrained here.
fn enforce_bit_width(
    value: &FpVar<Fr>,
    n_bits: usize,
    guard: &Boolean<Fr>,
) -> Result<(), SynthesisError> {
    let bits = value.to_bits_le()?; // canonical LE bits, constrained to equal `value`
    let low = Boolean::le_bits_to_fp(&bits[..n_bits])?; // recompose the low `n_bits`
    value.conditional_enforce_equal(&low, guard) // high bits must be zero when guarded
}

impl ConstraintSynthesizer<Fr> for UsageCircuit {
    /// Emit the constraints for every row. This task enforces per-row commitment
    /// correctness, range bounds and well-formed padding; the DR-cap + pseudonym
    /// binding (Task 7) and uniqueness + digest binding (Task 8) will extend this.
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Allocate the public digest as an (as-yet unconstrained) witness. Task 8
        // promotes it to a public input and binds it to the Poseidon of all rows;
        // for now it is a placeholder so the system stays satisfiable.
        let _digest_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&self.digest)))?;

        // Constrain each row uniformly; `RowVars` are collected for later tasks
        // even though this task consumes only the per-row checks inside `enforce_row`.
        for row in &self.rows {
            let _vars = self.enforce_row(cs.clone(), row)?;
        }
        Ok(())
    }
}

/// Build a canonical inactive (padding) row: zero opening, zero weight, and the
/// fixed padded commitment/pseudonym for the given `k_epoch`.
#[cfg(test)]
fn padded_row(k_epoch: &[u8; 32]) -> RowWitness {
    RowWitness {
        active: false,
        work_id: [0u8; 32],
        minutes: 0,
        plays: 0,
        salt: [0u8; 32],
        price_ppm: 0,
        region_ppm: 0,
        weight: 0,
        commitment: padded_commitment(),
        pseudonym: padded_pseudonym(k_epoch),
    }
}

/// The epoch key the test circuit binds pseudonyms to.
#[cfg(test)]
const TEST_K_EPOCH: [u8; 32] = [7u8; 32];
/// The DR curvature the test circuit uses (matches the native `weight_of` input).
#[cfg(test)]
const TEST_K_PPM: u64 = 1_000_000;

/// Build a real (active) `RowWitness`, deriving `weight`/`commitment`/`pseudonym`
/// from the native helpers so the circuit's recomputation agrees exactly.
#[cfg(test)]
fn test_row(
    work_id: &[u8; 32],
    minutes: u64,
    plays: u64,
    salt: &[u8; 32],
    price_ppm: u64,
    region_ppm: u64,
) -> RowWitness {
    let commitment = native_commitment(work_id, minutes, plays, salt);
    RowWitness {
        active: true,
        work_id: *work_id,
        minutes,
        plays,
        salt: *salt,
        price_ppm,
        region_ppm,
        weight: crate::dr::weight_of(minutes, plays, price_ppm, region_ppm, TEST_K_PPM),
        commitment,
        pseudonym: native_pseudonym(&TEST_K_EPOCH, &commitment),
    }
}

/// Assemble a `UsageCircuit` from the given active `rows`, padding out to
/// `MAX_WORKS` with canonical inactive rows and setting a placeholder digest.
#[cfg(test)]
fn test_circuit(mut rows: Vec<RowWitness>) -> UsageCircuit {
    // Pad with canonical inactive rows so the row count is always `MAX_WORKS`.
    while rows.len() < MAX_WORKS {
        rows.push(padded_row(&TEST_K_EPOCH));
    }
    UsageCircuit {
        epoch: 1,
        tier: [0u8; 32],
        k_epoch: TEST_K_EPOCH,
        k_ppm: TEST_K_PPM,
        rows,
        // Placeholder digest; the digest public-input binding is Task 8.
        digest: crate::field::fr_to_bytes32(&Fr::from(0u64)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::fr_from_u128;
    use crate::poseidon::poseidon_hash;
    use ark_r1cs_std::alloc::AllocVar;
    use ark_r1cs_std::fields::fp::FpVar;
    use ark_r1cs_std::R1CSVar;
    use ark_relations::r1cs::ConstraintSystem;

    /// The in-circuit Poseidon output equals the native one for the same inputs.
    #[test]
    fn gadget_matches_native() {
        let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
        let a = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(3))).unwrap();
        let b = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(5))).unwrap();
        let out = poseidon_hash_gadget(cs.clone(), &[a, b]).unwrap();
        let native = poseidon_hash(&[fr_from_u128(3), fr_from_u128(5)]);
        assert_eq!(out.value().unwrap(), native);
        assert!(cs.is_satisfied().unwrap());
    }

    /// A hand-built single-active-row circuit satisfies its constraints; corrupting
    /// the row's commitment makes the constraint system unsatisfiable.
    #[test]
    fn commitment_and_range_constraints() {
        use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
        let row = super::test_row(&[1u8; 32], 60, 2, &[9u8; 32], 1_000_000, 1_000_000);
        let circ = super::test_circuit(vec![row.clone()]);
        let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
        circ.clone().generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());

        // Corrupt the commitment: constraints must now be violated.
        let mut bad = circ;
        bad.rows[0].commitment = [0xEE; 32];
        let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
        bad.generate_constraints(cs2.clone()).unwrap();
        assert!(!cs2.is_satisfied().unwrap());
    }
}
