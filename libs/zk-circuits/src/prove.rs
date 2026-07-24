//! Witness generation and Groth16 proof creation for the usage-proof circuit.

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
