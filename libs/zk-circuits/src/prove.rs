//! Witness generation and Groth16 proof creation for the usage-proof circuit.

use ark_bn254::Bn254;
use ark_groth16::{Groth16, Proof, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::rand::rngs::StdRng;
use ark_std::rand::SeedableRng;

use crate::circuit::{RowWitness, UsageCircuit};
use crate::field::fr_from_bytes32;
use crate::MAX_WORKS;

/// The public, per-work projection of a usage row that feeds the on-chain digest.
///
/// A [`crate::poseidon::digest`] is taken over exactly `MAX_WORKS` of these (real
/// rows first, canonical padding after), so the prover and the on-chain verifier
/// agree on a single Poseidon commitment to the whole submission. Only
/// `pseudonym`, `work_id` and `weight` are hashed into the digest; `commitment`
/// is carried here because later tasks (proof assembly / verification) reference
/// the per-row commitment even though it does not enter the digest preimage.
///
/// serde derives are intentionally omitted: the crate does not yet depend on
/// serde. They will be added in a later task alongside the wire format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicRow {
    /// The 256-bit work identifier (big-endian), reduced into the field when hashed.
    pub work_id: [u8; 32],
    /// The Poseidon usage commitment for this row (not part of the digest preimage).
    pub commitment: [u8; 32],
    /// The epoch-bound pseudonym `Poseidon(k_epoch, commitment)` for this row.
    pub pseudonym: [u8; 32],
    /// The diminishing-returns-capped weight this row contributes.
    pub weight: u128,
}

/// The caller-supplied opening of one real usage row: everything a user knows
/// about a single work's play activity this epoch, before the prover derives
/// its `weight`/`commitment`/`pseudonym`.
///
/// This is the public entry point's row type — plain integers and byte
/// arrays, no field-element or circuit types leak into it — so callers
/// outside this crate never need to depend on `ark_bn254`/`ark_ff` just to
/// build a proof request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageRowInput {
    /// The 256-bit work identifier this row reports usage for.
    pub work_id: [u8; 32],
    /// Minutes the work was played this epoch.
    pub minutes: u64,
    /// Number of listening sessions this epoch.
    pub plays: u64,
    /// The hiding salt for this row's Poseidon commitment.
    pub salt: [u8; 32],
    /// Per-work price in parts-per-million, feeding the DR-weight formula.
    pub price_ppm: u64,
    /// Regional multiplier in parts-per-million, feeding the DR-weight formula.
    pub region_ppm: u64,
}

/// A complete, verifiable usage proof: the serialized Groth16 proof plus the
/// public context and per-row data the on-chain/off-chain verifier needs to
/// interpret it.
///
/// `digest` is the single value the Groth16 public input actually binds
/// (see [`crate::poseidon::digest`]); `epoch`, `tier`, `k_epoch` and `rows`
/// are NOT re-checked by `verify` — they are the prover's claimed opening of
/// that digest, carried alongside the proof so callers (payout computation,
/// on-chain event logs) do not have to separately transport them. A verifier
/// that cares whether these fields are honest must recompute
/// `crate::poseidon::digest(epoch, tier, k_epoch, rows)` itself and compare
/// it to `digest` before trusting them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofBundle {
    /// The Groth16 proof, `ark-serialize` compressed-encoded.
    pub proof: Vec<u8>,
    /// The single public input the proof binds: `Poseidon(epoch, tier, k_epoch, rows)`.
    pub digest: [u8; 32],
    /// The epoch this proof covers (claimed opening of `digest`, unchecked by `verify`).
    pub epoch: u64,
    /// The subscription tier identifier (claimed opening of `digest`, unchecked by `verify`).
    pub tier: [u8; 32],
    /// The per-epoch pseudonym key used to derive every row's `pseudonym` (claimed
    /// opening of `digest`, unchecked by `verify` — this is the user's secret and
    /// callers must handle it accordingly).
    pub k_epoch: [u8; 32],
    /// All `MAX_WORKS` rows in circuit order (active rows first, canonical
    /// padding after) — the claimed opening of `digest`'s per-row inputs,
    /// unchecked by `verify`. `rows[0]` is the first active input's public row.
    pub rows: Vec<PublicRow>,
}

/// Errors `prove` can return. `verify` never returns an error: any
/// deserialization or verification failure collapses to `false`, since a
/// verifier only needs a yes/no answer.
///
/// The crate has no `thiserror` dependency, so this implements `Display`
/// (and `std::error::Error`) by hand rather than pulling one in for a single
/// error type.
#[derive(Debug)]
pub enum ProveError {
    /// `inputs.len() > MAX_WORKS`: more real usage rows than a single proof
    /// can carry. Callers with more works must split across multiple proofs.
    TooManyWorks,
    /// The Groth16 prover itself failed (e.g. the fixed-shape circuit was
    /// somehow unsatisfiable). Wraps the underlying `SynthesisError`.
    Synthesis(ark_relations::r1cs::SynthesisError),
    /// Serializing the generated proof to bytes failed.
    Serialize(ark_serialize::SerializationError),
}

impl std::fmt::Display for ProveError {
    /// Render a human-readable description of the failure, delegating to the
    /// wrapped error's own `Display` where one exists.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProveError::TooManyWorks => {
                write!(
                    f,
                    "too many usage rows: exceeds MAX_WORKS ({})",
                    crate::MAX_WORKS
                )
            }
            ProveError::Synthesis(e) => write!(f, "groth16 proving failed: {e}"),
            ProveError::Serialize(e) => write!(f, "proof serialization failed: {e}"),
        }
    }
}

impl std::error::Error for ProveError {}

/// The fixed seed the prover's randomness is derived from.
///
/// Groth16 proving randomness (the `r`/`s` blinding factors) only needs to be
/// unpredictable to keep proofs zero-knowledge against an observer who does
/// NOT already know the witness; it carries none of the soundness weight
/// that `setup`'s toxic waste does. Fixing it (distinct from
/// `setup::devnet_setup`'s own seed) buys byte-identical, reproducible test
/// fixtures and deterministic CI timing at zero security cost for this
/// devnet crate. A production prover run by an untrusted party should still
/// draw this from real entropy so two proofs of the same statement are
/// unlinkable; that swap is a one-line change when this code is promoted
/// past devnet.
const PROVE_SEED: [u8; 32] = [1u8; 32];

/// Build the canonical inactive (padding) row for a given `k_epoch`: the
/// all-zero opening's commitment, and that commitment's pseudonym under this
/// `k_epoch`. Every proof pads its active rows out to `MAX_WORKS` with copies
/// of this row, so the digest and circuit shape stay fixed regardless of how
/// many real rows a user submits.
///
/// Mirrors `circuit::padded_commitment`/`circuit::padded_pseudonym` (the
/// latter is `#[cfg(test)]`-only in that module), recomputed here from the
/// public native helpers so `prove` does not depend on `circuit`'s test-only
/// items — the FROZEN DIGEST CONTRACT requires this padding to use the exact
/// same `k_epoch` passed to `prove`.
fn padded_row(k_epoch: &[u8; 32]) -> RowWitness {
    let commitment = crate::poseidon::commitment(&[0u8; 32], 0, 0, &[0u8; 32]);
    let pseudonym = crate::poseidon::pseudonym(k_epoch, &commitment);
    RowWitness {
        active: false,
        work_id: [0u8; 32],
        minutes: 0,
        plays: 0,
        salt: [0u8; 32],
        price_ppm: 0,
        region_ppm: 0,
        weight: 0,
        commitment,
        pseudonym,
    }
}

/// Build a real (active) `RowWitness` from a caller's `UsageRowInput`,
/// deriving `weight`/`commitment`/`pseudonym` with the exact same native
/// helpers the circuit's own recomputation uses, so the witness is always
/// satisfiable.
fn active_row(input: &UsageRowInput, k_epoch: &[u8; 32], k_ppm: u64) -> RowWitness {
    let commitment =
        crate::poseidon::commitment(&input.work_id, input.minutes, input.plays, &input.salt);
    let pseudonym = crate::poseidon::pseudonym(k_epoch, &commitment);
    let weight = crate::dr::weight_of(
        input.minutes,
        input.plays,
        input.price_ppm,
        input.region_ppm,
        k_ppm,
    );
    RowWitness {
        active: true,
        work_id: input.work_id,
        minutes: input.minutes,
        plays: input.plays,
        salt: input.salt,
        price_ppm: input.price_ppm,
        region_ppm: input.region_ppm,
        weight,
        commitment,
        pseudonym,
    }
}

/// Project a `RowWitness` into its public form: the fields that enter the
/// digest preimage (plus the commitment, carried for later reference) but
/// none of the private opening (`minutes`, `plays`, `salt`, ppm factors).
fn public_row_of(row: &RowWitness) -> PublicRow {
    PublicRow {
        work_id: row.work_id,
        commitment: row.commitment,
        pseudonym: row.pseudonym,
        weight: row.weight,
    }
}

/// Prove that `inputs` (a user's real per-epoch usage rows) is well-formed,
/// range-bounded, DR-capped, epoch-bound and per-work unique, returning a
/// [`ProofBundle`] whose `digest` is the only value the Groth16 proof binds.
///
/// Rows are padded with canonical inactive rows (via [`padded_row`], using
/// this SAME `k_epoch`) out to exactly `MAX_WORKS`, matching the fixed shape
/// `pk` was generated for. The digest is computed natively over all
/// `MAX_WORKS` public rows in circuit order — active rows first, then
/// padding — via `crate::poseidon::digest`, so it agrees bit-for-bit with
/// what `UsageCircuit::generate_constraints` recomputes in-circuit.
///
/// # Errors
/// Returns `ProveError::TooManyWorks` if `inputs.len() > MAX_WORKS`;
/// `ProveError::Synthesis` if Groth16 proving itself fails; `ProveError::Serialize`
/// if the resulting proof cannot be serialized.
pub fn prove(
    pk: &ProvingKey<Bn254>,
    epoch: u64,
    tier: &[u8; 32],
    k_epoch: &[u8; 32],
    k_ppm: u64,
    inputs: &[UsageRowInput],
) -> Result<ProofBundle, ProveError> {
    if inputs.len() > MAX_WORKS {
        return Err(ProveError::TooManyWorks);
    }

    // Active rows first, then canonical padding — the circuit's shape and the
    // digest preimage both depend on this exact ordering.
    let mut rows: Vec<RowWitness> = inputs
        .iter()
        .map(|input| active_row(input, k_epoch, k_ppm))
        .collect();
    while rows.len() < MAX_WORKS {
        rows.push(padded_row(k_epoch));
    }

    // The native digest over every row's public projection; this is the
    // single value the Groth16 public input will bind.
    let public_rows: Vec<PublicRow> = rows.iter().map(public_row_of).collect();
    let digest = crate::poseidon::digest(epoch, tier, k_epoch, &public_rows);

    let circuit = UsageCircuit {
        epoch,
        tier: *tier,
        k_epoch: *k_epoch,
        k_ppm,
        rows,
        digest,
    };

    // Fixed-seed RNG: see `PROVE_SEED`'s doc comment for why this is safe and
    // desirable for a devnet prover.
    let mut rng = StdRng::from_seed(PROVE_SEED);
    let proof = Groth16::<Bn254>::prove(pk, circuit, &mut rng).map_err(ProveError::Synthesis)?;

    let mut proof_bytes = Vec::new();
    proof
        .serialize_compressed(&mut proof_bytes)
        .map_err(ProveError::Serialize)?;

    Ok(ProofBundle {
        proof: proof_bytes,
        digest,
        epoch,
        tier: *tier,
        k_epoch: *k_epoch,
        rows: public_rows,
    })
}

/// Recompute the public digest from only the ACTIVE (real) rows of a submission,
/// padding to `MAX_WORKS` with the canonical inactive row internally.
///
/// This centralizes the padding convention so off-chain verifiers (e.g. the
/// settlement job) that receive only a user's real rows — the pseudonyms,
/// work ids and weights carried in the `ConsumptionSubmitted` event — can
/// reproduce the exact `crate::poseidon::digest` a prover committed to, without
/// re-implementing the "active first, canonical padding after" rule. The padding
/// rows are built with [`padded_row`] under this SAME `k_epoch`, matching what
/// [`prove`] does, so the recomputed digest agrees bit-for-bit with an honest
/// bundle's `digest`.
///
/// # Errors
/// Returns `ProveError::TooManyWorks` if `active.len() > MAX_WORKS`.
pub fn digest_from_active(
    epoch: u64,
    tier: &[u8; 32],
    k_epoch: &[u8; 32],
    active: &[PublicRow],
) -> Result<[u8; 32], ProveError> {
    // More real rows than a single proof's fixed shape can carry is a hard error,
    // mirroring `prove`'s own bound on `inputs`.
    if active.len() > MAX_WORKS {
        return Err(ProveError::TooManyWorks);
    }

    // The canonical padded row's public projection, derived under this `k_epoch`
    // exactly as `prove` builds its padding — so the digest preimage matches.
    let pad = public_row_of(&padded_row(k_epoch));

    // Active rows first, then canonical padding out to the fixed `MAX_WORKS`
    // shape the digest (and the verifying key) are defined over.
    let mut rows: Vec<PublicRow> = active.to_vec();
    while rows.len() < MAX_WORKS {
        rows.push(pad.clone());
    }

    Ok(crate::poseidon::digest(epoch, tier, k_epoch, &rows))
}

/// Verify that `proof` is a valid Groth16 proof, under `vk`, of the
/// single-public-input statement `digest`.
///
/// Deserializes `proof` (compressed `ark-serialize` encoding) and checks it
/// against the public input `[fr_from_bytes32(digest)]` — the exact
/// preimage-independent value `UsageCircuit::generate_constraints` binds.
/// Any failure to deserialize the proof, or a `Groth16::verify` error,
/// collapses to `false`: a verifier only ever needs a boolean answer, never
/// the reason for a "no".
pub fn verify(vk: &VerifyingKey<Bn254>, digest: &[u8; 32], proof: &[u8]) -> bool {
    let Ok(proof) = Proof::<Bn254>::deserialize_compressed(proof) else {
        return false;
    };
    let public_input = [fr_from_bytes32(digest)];
    Groth16::<Bn254>::verify(vk, &public_input, &proof).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: an honest `prove` verifies against its own digest and the
    /// devnet verifying key; the same proof fails against a tampered digest
    /// (the digest is the only thing the public input binds); and the
    /// bundle's reported weight for the single active row matches the native
    /// `weight_of` computation exactly.
    ///
    /// `devnet_setup` is called exactly once (it takes on the order of a
    /// minute) and the resulting `(pk, vk)` pair is reused for both the
    /// `prove` and `verify` calls below.
    #[test]
    fn prove_verify_round_trip() {
        let (pk, vk) = crate::setup::devnet_setup();
        let inputs = vec![UsageRowInput {
            work_id: [1u8; 32],
            minutes: 60,
            plays: 2,
            salt: [9u8; 32],
            price_ppm: 1_000_000,
            region_ppm: 1_000_000,
        }];
        let bundle = prove(&pk, 7, &[0xAB; 32], &[0x5c; 32], 1_000_000, &inputs).unwrap();
        assert!(verify(&vk, &bundle.digest, &bundle.proof));
        // The digest binds the outputs; a different digest must fail.
        assert!(!verify(&vk, &[0u8; 32], &bundle.proof));
        // The proven weight equals the native weight.
        assert_eq!(
            bundle.rows[0].weight,
            crate::dr::weight_of(60, 2, 1_000_000, 1_000_000, 1_000_000)
        );
    }

    /// `digest_from_active` over just the active rows a fresh `prove` produced
    /// reproduces that bundle's `digest` — i.e. the helper's internal padding
    /// matches `prove`'s padding exactly. Uses two active rows so the active
    /// slice is a strict prefix of the padded `MAX_WORKS` rows.
    #[test]
    fn digest_from_active_matches_prove() {
        let (pk, _vk) = crate::setup::devnet_setup();
        let epoch = 11u64;
        let tier = [0x7c; 32];
        let k_epoch = [0x33; 32];
        let inputs = vec![
            UsageRowInput {
                work_id: [1u8; 32],
                minutes: 60,
                plays: 2,
                salt: [9u8; 32],
                price_ppm: 1_000_000,
                region_ppm: 1_000_000,
            },
            UsageRowInput {
                work_id: [2u8; 32],
                minutes: 30,
                plays: 1,
                salt: [8u8; 32],
                price_ppm: 900_000,
                region_ppm: 1_000_000,
            },
        ];
        let bundle = prove(&pk, epoch, &tier, &k_epoch, 1_000_000, &inputs).unwrap();

        // The active rows are the first `inputs.len()` of the padded bundle rows.
        let active = &bundle.rows[..inputs.len()];
        let recomputed = digest_from_active(epoch, &tier, &k_epoch, active).unwrap();
        assert_eq!(recomputed, bundle.digest);
    }

    /// More active rows than `MAX_WORKS` is rejected, just like `prove`.
    #[test]
    fn digest_from_active_rejects_overflow() {
        let too_many = vec![
            PublicRow {
                work_id: [0u8; 32],
                commitment: [0u8; 32],
                pseudonym: [0u8; 32],
                weight: 0,
            };
            MAX_WORKS + 1
        ];
        assert!(matches!(
            digest_from_active(1, &[0u8; 32], &[0u8; 32], &too_many),
            Err(ProveError::TooManyWorks)
        ));
    }
}
