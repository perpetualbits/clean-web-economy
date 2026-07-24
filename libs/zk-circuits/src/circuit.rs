//! The R1CS constraint system for the usage-proof circuit.

use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::convert::ToBitsGadget;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::fields::FieldVar;
use ark_r1cs_std::R1CSVar;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::field::{fr_from_bytes32, fr_from_u128, Fr};
use crate::poseidon::{commitment as native_commitment, poseidon_config};
// The native pseudonym is used only to build expected values in the tests; the
// production circuit now derives pseudonyms in-circuit from the `k_epoch` variable.
#[cfg(test)]
use crate::poseidon::pseudonym as native_pseudonym;
use crate::{MAX_PLAYS_CIRCUIT, MAX_WORKS};

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
/// Bit-width bound on a row's DR-capped `weight`. The claimed weight equals
/// `floor(value * d / 1e6)` where `value = minutes * price_ppm * region_ppm`, so
/// `value < 2^MINUTES_BITS * 2^PPM_BITS * 2^PPM_BITS = 2^(32+40+40) = 2^112`, and
/// `d <= 1e6` gives `weight <= value < 2^112`. Bounding `weight` to `2^120`
/// leaves a comfortable margin while still pinning it far below the field
/// modulus. Without this bound the proven floor-division `weight*1e6 + r == value*d`
/// admits spurious full-width-field `weight` solutions, so the DR cap is only
/// complete once `weight` is bit-bounded here.
pub const WEIGHT_BITS: usize = 120;

/// The fixed-point scale (parts-per-million) the DR-capped weight divides by,
/// mirroring `cwe_dapr`'s `mul_div(value, d_ppm, 1e6)` in `weight_of`.
const PPM_SCALE: u128 = 1_000_000;

/// The diminishing-returns curvature the circuit bakes into its constant lookup
/// table. For the MVP this is the neutral `1_000_000` (`D(1)` full weight);
/// governance-tunable `k_ppm` is deferred — see the design doc. Fixing it as a
/// circuit constant keeps the DR table (and hence the verifying key) stable.
const K_PPM_CIRCUIT: u64 = 1_000_000;

/// Bit-width used to range-check the floor-division remainder `r`. `PPM_SCALE`
/// (`1_000_000`) is below `2^20`, so 20 bits bounds `r` well under the field
/// modulus; a companion `PPM_SCALE-1 - r` check then pins `r < PPM_SCALE`.
const REMAINDER_BITS: usize = 20;

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

/// The allocated per-row variables the circuit-wide passes reuse: uniqueness
/// reads `work_id_var`/`active_var`, and the digest binding reads
/// `pseudonym_var`/`work_id_var`/`weight_var` (with `active_var` implicitly bound
/// via the padding constraints). Returning them from `enforce_row` keeps the
/// constraint system's row logic in one place. The commitment is fully
/// constrained inside `enforce_row` and does not enter any later pass, so it is
/// not carried here.
struct RowVars {
    /// The work identifier as a field element (`fr_from_bytes32(work_id)`).
    work_id_var: FpVar<Fr>,
    /// The prover-claimed DR-capped weight as a field element.
    weight_var: FpVar<Fr>,
    /// The epoch-bound pseudonym as a field element.
    pseudonym_var: FpVar<Fr>,
    /// The row-active flag.
    active_var: Boolean<Fr>,
}

/// The canonical commitment of the all-zero opening — the value every inactive
/// (padding) row must carry so the eventual digest is deterministic. Computed via
/// the native helper so the circuit and prover agree bit-for-bit.
fn padded_commitment() -> [u8; 32] {
    native_commitment(&[0u8; 32], 0, 0, &[0u8; 32])
}

/// The canonical pseudonym of a padding row: `Poseidon(k_epoch, padded_commitment)`.
/// Inactive rows must carry this so their pseudonym slot is fixed and the digest
/// stays deterministic. As of Task 7 the circuit derives this value *in-circuit*
/// from the shared `k_epoch` variable, so this native helper only builds the
/// matching expected value for the tests (hence `#[cfg(test)]`).
#[cfg(test)]
fn padded_pseudonym(k_epoch: &[u8; 32]) -> [u8; 32] {
    native_pseudonym(k_epoch, &padded_commitment())
}

impl UsageCircuit {
    /// Allocate and constrain a single row, returning its reusable `RowVars`.
    ///
    /// Real (`active`) rows must satisfy commitment-correctness, the range bounds,
    /// `work_id != 0`, the DR-cap on `weight`, and the epoch-binding
    /// `pseudonym == Poseidon(k_epoch, commitment)`. Inactive (padding) rows must
    /// be the canonical zero row: `work_id == 0`, `weight == 0`, the fixed padded
    /// commitment, and the padded pseudonym `Poseidon(k_epoch, padded_commitment)`.
    /// Every active-only check is gated on `active`, every inactive-only check on
    /// its negation, so one uniform row routine handles both kinds.
    ///
    /// `k_epoch_var` is the single circuit-wide epoch-key variable allocated by
    /// `generate_constraints`; both the active pseudonym derivation and the
    /// inactive padded-pseudonym derivation read it, so a prover cannot smuggle in
    /// an inconsistent `k_epoch` across rows.
    fn enforce_row(
        &self,
        cs: ConstraintSystemRef<Fr>,
        row: &RowWitness,
        k_epoch_var: &FpVar<Fr>,
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

        // --- DR-cap on the claimed weight (active rows) ------------------------
        // Prove `weight == floor(minutes·price·region · D(plays) / 1e6)`, exactly
        // the native `weight_of`, via an in-circuit DR table + proven division.
        enforce_dr_weight(
            row,
            &minutes_var,
            &plays_var,
            &price_var,
            &region_var,
            &weight_var,
            &active_var,
        )?;
        // Upper-bound the claimed weight so the floor-division proof is complete:
        // without this, `weight*1e6 + r == value*d` has spurious full-width-field
        // solutions. `weight < 2^WEIGHT_BITS` (>= its true `2^112` ceiling) closes
        // that gap before `weight` feeds the digest below.
        enforce_bit_width(&weight_var, WEIGHT_BITS, &active_var)?;

        // --- Epoch binding (active rows) ---------------------------------------
        // The pseudonym must be the in-circuit Poseidon of the shared epoch key
        // and this row's commitment; corrupting either breaks equality. Sharing
        // `k_epoch_var` with the inactive derivation below closes the Task-6
        // soundness gap (no per-row `k_epoch` freedom).
        let derived_pseudonym =
            poseidon_hash_gadget(cs.clone(), &[k_epoch_var.clone(), commitment_var.clone()])?;
        pseudonym_var.conditional_enforce_equal(&derived_pseudonym, &active_var)?;

        // --- Canonical padding (inactive rows) ---------------------------------
        // Padding rows are pinned to the zero row + fixed padded commitment and a
        // pseudonym *derived in-circuit* from the same `k_epoch_var`, so the later
        // digest over all rows is deterministic and the epoch key is bound even on
        // padding rows (rather than a natively-precomputed constant).
        work_id_var.conditional_enforce_equal(&FpVar::zero(), &inactive_var)?;
        weight_var.conditional_enforce_equal(&FpVar::zero(), &inactive_var)?;
        let padded_c = FpVar::constant(fr_from_bytes32(&padded_commitment()));
        commitment_var.conditional_enforce_equal(&padded_c, &inactive_var)?;
        // Same derivation as the active path, over the canonical padded commitment.
        let padded_p = poseidon_hash_gadget(cs.clone(), &[k_epoch_var.clone(), padded_c.clone()])?;
        pseudonym_var.conditional_enforce_equal(&padded_p, &inactive_var)?;

        Ok(RowVars {
            work_id_var,
            weight_var,
            pseudonym_var,
            active_var,
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

/// Enforce the diminishing-returns cap on an active row's claimed `weight`.
///
/// Proves `weight == floor(minutes·price·region · D(plays) / 1e6)` — bit-for-bit
/// the native `crate::dr::weight_of` — in two parts, both gated on `active`:
///
/// 1. **DR lookup.** `D(plays)` is selected from the constant table
///    `d_ppm_table(K_PPM_CIRCUIT)` by a one-hot selector `s_0..s_63`: exactly one
///    bit is set (`Σ s_i == 1`), its index equals `plays-1` (`Σ i·s_i == plays-1`),
///    and the chosen multiplier is `d == Σ s_i·table_i`. Because the index sum
///    lands in `[0, 63]` while `plays==0` would require it to equal the field
///    value `-1`, an active `plays == 0` is implicitly rejected (matching the
///    fact that `weight_of` clamps `plays` to `>= 1`).
/// 2. **Proven floor division.** With `value == minutes·price·region` (exact in
///    the field — every factor is bit-bounded so the product stays far below the
///    modulus), enforce `weight·1e6 + r == value·d` and `0 <= r < 1e6`. That pins
///    `weight` to the floor of `value·d/1e6` and `r` to the remainder.
///
/// The remainder `r` is computed natively via modular reduction (`value·d` can
/// exceed `u128`, so we reduce each factor mod `PPM_SCALE` first). The constraint
/// system is taken from `weight_var` (all inputs share one system).
fn enforce_dr_weight(
    row: &RowWitness,
    minutes_var: &FpVar<Fr>,
    plays_var: &FpVar<Fr>,
    price_var: &FpVar<Fr>,
    region_var: &FpVar<Fr>,
    weight_var: &FpVar<Fr>,
    active: &Boolean<Fr>,
) -> Result<(), SynthesisError> {
    // All allocated vars live in the same system; reuse it for the new witnesses.
    let cs = weight_var.cs();
    // The constant DR multiplier table `D(1..=64)` in ppm at the neutral curvature.
    let table = crate::dr::d_ppm_table(K_PPM_CIRCUIT);

    // The hot selector index, clamped like `weight_of` (`plays` 0 → index 0). For
    // active rows `plays` is in `[1, 64]`, so the clamp is a no-op there; it only
    // gives padding/degenerate rows a well-formed witness (their sum checks below
    // are not enforced, being gated on `active`).
    let hot = (row.plays.clamp(1, MAX_PLAYS_CIRCUIT) - 1) as usize;

    // Allocate one boolean selector per table slot.
    let mut selectors = Vec::with_capacity(MAX_PLAYS_CIRCUIT as usize);
    for i in 0..MAX_PLAYS_CIRCUIT as usize {
        selectors.push(Boolean::new_witness(cs.clone(), || Ok(i == hot))?);
    }

    // Accumulate the three selector sums in one pass.
    let mut sum_sel = FpVar::<Fr>::zero(); // Σ s_i
    let mut sum_idx = FpVar::<Fr>::zero(); // Σ i·s_i
    let mut d_var = FpVar::<Fr>::zero(); // Σ s_i·table_i (the selected D(plays))
    for (i, s) in selectors.iter().enumerate() {
        let s_fp = FpVar::from(s.clone()); // boolean → {0,1} field element
        let idx_c = FpVar::constant(fr_from_u128(i as u128)); // slot index constant
        let tab_c = FpVar::constant(fr_from_u128(table[i] as u128)); // D(i+1) constant
        sum_sel = &sum_sel + &s_fp;
        sum_idx = &sum_idx + &(&s_fp * &idx_c);
        d_var = &d_var + &(&s_fp * &tab_c);
    }

    // Exactly one slot selected, and its index is `plays-1` (ties `d` to `plays`).
    sum_sel.conditional_enforce_equal(&FpVar::one(), active)?;
    let plays_minus_one = plays_var - FpVar::one(); // valid: active plays >= 1
    sum_idx.conditional_enforce_equal(&plays_minus_one, active)?;

    // `value = minutes·price·region`, exact in-field (factors are bit-bounded).
    let value_var = &(minutes_var * price_var) * region_var;

    // Native remainder `r = (value·d) mod 1e6`. Reduce each factor mod `PPM_SCALE`
    // first so the intermediate product cannot overflow `u128`.
    let value_native = (row.minutes as u128) * (row.price_ppm as u128) * (row.region_ppm as u128);
    let d_native = table[hot] as u128;
    let r_native = ((value_native % PPM_SCALE) * (d_native % PPM_SCALE)) % PPM_SCALE;
    let r_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(r_native)))?;

    // Proven floor division: `weight·1e6 + r == value·d`.
    let scale = FpVar::constant(fr_from_u128(PPM_SCALE));
    let lhs = &(weight_var * &scale) + &r_var;
    let rhs = &value_var * &d_var;
    lhs.conditional_enforce_equal(&rhs, active)?;

    // Range-check `0 <= r < 1e6`: `r` fits in 20 bits (so it does not wrap the
    // field), and `PPM_SCALE-1 - r` also fits in 20 bits (so `r <= 999_999`).
    enforce_bit_width(&r_var, REMAINDER_BITS, active)?;
    let r_slack = &FpVar::constant(fr_from_u128(PPM_SCALE - 1)) - &r_var;
    enforce_bit_width(&r_slack, REMAINDER_BITS, active)?;

    Ok(())
}

impl ConstraintSynthesizer<Fr> for UsageCircuit {
    /// Emit the full usage-proof constraint system: per-row commitment
    /// correctness, range bounds, DR-cap, epoch-binding and well-formed padding;
    /// per-work uniqueness over active rows; and binding of the single public
    /// `digest` input to the Poseidon of `(epoch, tier, k_epoch, per-row outputs)`.
    ///
    /// The circuit has a fixed shape: it MUST be built with exactly `MAX_WORKS`
    /// rows (real rows first, canonical padding after). This is a caller contract
    /// — the prover path (Task 10) pads before synthesis — enforced here so a
    /// mis-sized circuit can never be turned into a Groth16 instance with the
    /// wrong verifying key; a wrong length returns `SynthesisError::Unsatisfiable`.
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Fixed-size circuit invariant: the row count pins the shape (and thus the
        // verifying key). Reject any other length rather than silently synthesising
        // a differently-shaped system.
        if self.rows.len() != MAX_WORKS {
            return Err(SynthesisError::Unsatisfiable);
        }

        // The digest is the SINGLE public input — the only value the on-chain
        // verifier sees. Everything else (epoch, tier, k_epoch, per-row data) is a
        // witness bound to this input through the recomputation below.
        let digest_var = FpVar::new_input(cs.clone(), || Ok(fr_from_bytes32(&self.digest)))?;

        // Allocate the per-epoch pseudonym key ONCE as a single witness variable.
        // Every row's active/inactive pseudonym is derived from this same variable,
        // so a prover cannot pick an inconsistent `k_epoch` between rows — the
        // fixed Groth16 circuit stays sound. (It is a witness, not a public input:
        // the epoch key is the user's secret; the public digest binds it.)
        let k_epoch_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&self.k_epoch)))?;

        // Constrain each row uniformly, collecting the reusable per-row variables
        // for the uniqueness and digest-binding passes below.
        let mut row_vars = Vec::with_capacity(MAX_WORKS);
        for row in &self.rows {
            row_vars.push(self.enforce_row(cs.clone(), row, &k_epoch_var)?);
        }

        // --- Per-work uniqueness (over active rows) ----------------------------
        // For every ordered pair i<j, forbid two *active* rows sharing a work id.
        // Gating on both rows being active is essential: padding rows all carry
        // `work_id == 0` and must NOT trip uniqueness. This makes the DR cap
        // un-bypassable by splitting one work across several rows.
        for i in 0..MAX_WORKS {
            for j in (i + 1)..MAX_WORKS {
                // work_id_i == work_id_j ?
                let ids_equal = row_vars[i].work_id_var.is_eq(&row_vars[j].work_id_var)?;
                // Enforce `!(active_i AND active_j AND ids_equal)`: at least one of
                // the three must be false, so two active rows can never collide.
                Boolean::enforce_kary_nand(&[
                    row_vars[i].active_var.clone(),
                    row_vars[j].active_var.clone(),
                    ids_equal,
                ])?;
            }
        }

        // --- Digest binding (single public input) ------------------------------
        // Recompute the digest in-circuit in the SAME field order as the native
        // `poseidon::digest`: [epoch, tier, k_epoch, (pseudonym, work_id, weight)*].
        // epoch and tier are witnesses here (only the digest is public); binding
        // them into the digest ties them to the on-chain-visible value.
        let epoch_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(self.epoch as u128)))?;
        let tier_var = FpVar::new_witness(cs.clone(), || Ok(fr_from_bytes32(&self.tier)))?;
        let mut digest_inputs = Vec::with_capacity(3 + 3 * MAX_WORKS);
        digest_inputs.push(epoch_var);
        digest_inputs.push(tier_var);
        digest_inputs.push(k_epoch_var.clone());
        for vars in &row_vars {
            // Per-row order mirrors the native digest exactly.
            digest_inputs.push(vars.pseudonym_var.clone());
            digest_inputs.push(vars.work_id_var.clone());
            digest_inputs.push(vars.weight_var.clone());
        }
        let computed_digest = poseidon_hash_gadget(cs.clone(), &digest_inputs)?;
        // Bind the public input to the recomputed digest (unconditional — the
        // digest commits to the whole submission, padding included).
        digest_var.enforce_equal(&computed_digest)?;

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
    let epoch = 1u64;
    let tier = [0u8; 32];
    // Project each (padded) row into its public form and take the native digest,
    // so the circuit's in-circuit recomputation must match exactly.
    let public_rows: Vec<crate::prove::PublicRow> = rows
        .iter()
        .map(|r| crate::prove::PublicRow {
            work_id: r.work_id,
            commitment: r.commitment,
            pseudonym: r.pseudonym,
            weight: r.weight,
        })
        .collect();
    let digest = crate::poseidon::digest(epoch, &tier, &TEST_K_EPOCH, &public_rows);
    UsageCircuit {
        epoch,
        tier,
        k_epoch: TEST_K_EPOCH,
        k_ppm: TEST_K_PPM,
        rows,
        digest,
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

    /// The DR-cap and epoch-binding constraints reject a row whose `weight` is
    /// inflated by one (breaking the proven floor division) and a row whose
    /// `pseudonym` does not equal `Poseidon(k_epoch, commitment)`.
    #[test]
    fn weight_and_pseudonym_constraints() {
        use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
        let base = super::test_row(&[1u8; 32], 60, 2, &[9u8; 32], 1_000_000, 1_000_000);

        // Inflated weight → violated.
        let mut infl = super::test_circuit(vec![base.clone()]);
        infl.rows[0].weight += 1;
        let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
        infl.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());

        // Wrong pseudonym → violated.
        let mut wp = super::test_circuit(vec![base]);
        wp.rows[0].pseudonym = [0x77; 32];
        let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
        wp.generate_constraints(cs2.clone()).unwrap();
        assert!(!cs2.is_satisfied().unwrap());
    }

    /// Uniqueness over active work ids and the single-public-input digest binding:
    /// (a) duplicate active work ids are rejected; (b) distinct rows carrying the
    /// correct native digest satisfy the system; (c) tampering the public digest
    /// makes the system unsatisfiable.
    #[test]
    fn uniqueness_and_digest() {
        use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
        // Two active rows, same work id → uniqueness violated.
        let r1 = super::test_row(&[1u8; 32], 10, 1, &[9u8; 32], 1_000_000, 1_000_000);
        let r2 = super::test_row(&[1u8; 32], 20, 1, &[8u8; 32], 1_000_000, 1_000_000);
        let dup = super::test_circuit(vec![r1, r2]);
        let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
        dup.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());

        // Distinct rows with the correct digest → satisfied.
        let a = super::test_row(&[1u8; 32], 10, 1, &[9u8; 32], 1_000_000, 1_000_000);
        let b = super::test_row(&[2u8; 32], 20, 1, &[8u8; 32], 1_000_000, 1_000_000);
        let ok = super::test_circuit(vec![a, b]);
        let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
        ok.clone().generate_constraints(cs2.clone()).unwrap();
        assert!(cs2.is_satisfied().unwrap());

        // Tamper the public digest → violated.
        let mut bad = ok;
        bad.digest = [0x00; 32];
        let cs3 = ConstraintSystem::<crate::field::Fr>::new_ref();
        bad.generate_constraints(cs3.clone()).unwrap();
        assert!(!cs3.is_satisfied().unwrap());
    }
}
