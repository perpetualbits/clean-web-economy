//! The R1CS constraint system for the usage-proof circuit.

use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};

use crate::field::Fr;
use crate::poseidon::poseidon_config;

/// The in-circuit Poseidon hash — absorb `inputs`, squeeze one element — using the
/// exact `poseidon_config()` the native hash uses, guaranteeing agreement.
// Only exercised by `gadget_matches_native` for now; the full usage-proof
// circuit wires this in as its commitment/pseudonym sub-gadget in a later task.
#[allow(dead_code)]
pub(crate) fn poseidon_hash_gadget(
    cs: ConstraintSystemRef<Fr>,
    inputs: &[FpVar<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    let mut sponge = PoseidonSpongeVar::new(cs, &poseidon_config());
    sponge.absorb(&inputs.to_vec())?; // absorb the whole input vector once, matching the native sponge
    Ok(sponge.squeeze_field_elements(1)?[0].clone())
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
}
