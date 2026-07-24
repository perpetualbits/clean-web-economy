//! EVM proof encoding for the on-chain `Groth16Verifier`.
//!
//! [`crate::prove::prove`] returns its proof in arkworks' compressed
//! `ark-serialize` encoding — compact, but NOT what the Solidity
//! `Groth16Verifier.verify(bytes32 digest, bytes proof)` decodes. That contract
//! runs `abi.decode(proof, (uint256[2] a, uint256[2][2] b, uint256[2] c))`,
//! feeding the points to the EVM `alt_bn128` pairing precompile. This module is
//! the single bridge between the two encodings, so any submitter (the settlement
//! `zk_submit` bin, tests, future tooling) can turn a [`crate::prove::ProofBundle`]'s
//! proof bytes into the exact calldata the verifier expects.
//!
//! **The encoding here MUST stay identical to `bin/export_keys.rs` (the fixture
//! exporter) and `bin/gen_verifier.rs` (the VK baker).** All three convert a
//! field element to its canonical (non-Montgomery) big-endian integer, and all
//! three reverse the `Fq2` limb order for G2 points (`[c1, c0]` — imaginary part
//! first) as the precompile requires. Getting that reversal wrong is the classic
//! BN254-on-EVM bug.

use ark_bn254::{Bn254, Fq, G1Affine, G2Affine};
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::Proof;
use ark_serialize::CanonicalDeserialize;

/// Encode one base-field element `Fq` as canonical big-endian 32 bytes — the
/// integer value the EVM reads as a `uint256`.
///
/// arkworks stores field elements in Montgomery form internally; `into_bigint`
/// converts back to the canonical integer and `to_bytes_be` renders it
/// big-endian. The result is left-padded to a full 32-byte word.
fn fq_word(f: &Fq) -> [u8; 32] {
    let be = f.into_bigint().to_bytes_be(); // canonical (non-Montgomery) big-endian
    let mut buf = [0u8; 32];
    buf[32 - be.len()..].copy_from_slice(&be); // left-pad to a full word
    buf
}

/// Append a G1 point's affine `(x, y)` coordinates, each a 32-byte word, to `out`.
///
/// Both coordinates are plain base-field elements, so each goes through
/// [`fq_word`] unchanged. The devnet proof points are never the point at
/// infinity, so that degenerate case is not represented.
fn push_g1(p: &G1Affine, out: &mut Vec<u8>) {
    out.extend_from_slice(&fq_word(&p.x));
    out.extend_from_slice(&fq_word(&p.y));
}

/// Append a G2 point in the precompile's expected limb order
/// `x.c1, x.c0, y.c1, y.c0` (four 32-byte words) to `out`.
///
/// A G2 coordinate is an `Fq2` value `c0 + c1·u`; the `alt_bn128` pairing
/// precompile expects each `Fq2` with its imaginary limb (`c1`) FIRST — the
/// reverse of arkworks' `(c0, c1)` field order.
fn push_g2(p: &G2Affine, out: &mut Vec<u8>) {
    out.extend_from_slice(&fq_word(&p.x.c1)); // x: imaginary limb first
    out.extend_from_slice(&fq_word(&p.x.c0));
    out.extend_from_slice(&fq_word(&p.y.c1)); // y: imaginary limb first
    out.extend_from_slice(&fq_word(&p.y.c0));
}

/// Convert a compressed `ark-serialize` Groth16 proof (as carried in
/// [`crate::prove::ProofBundle::proof`]) into the 256-byte ABI-encoded calldata
/// the on-chain `Groth16Verifier` decodes into `(uint256[2] a, uint256[2][2] b,
/// uint256[2] c)`.
///
/// The tuple and its members are all STATIC ABI types (fixed-size arrays of
/// `uint256`), so their encoding is simply the eight words concatenated in
/// order — `a.x, a.y, b.x.c1, b.x.c0, b.y.c1, b.y.c0, c.x, c.y` — with no length
/// or offset headers. That 256-byte blob is exactly what `abi.decode` reads.
///
/// Returns `None` if `proof` is not a valid compressed BN254 proof.
pub fn proof_to_evm_calldata(proof: &[u8]) -> Option<Vec<u8>> {
    // Re-decode the compressed proof bytes into the actual curve points.
    let proof = Proof::<Bn254>::deserialize_compressed(proof).ok()?;
    let mut out = Vec::with_capacity(256);
    push_g1(&proof.a, &mut out); // a  (G1) → 2 words
    push_g2(&proof.b, &mut out); // b  (G2) → 4 words
    push_g1(&proof.c, &mut out); // c  (G1) → 2 words
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prove::{prove, UsageRowInput};
    use crate::setup::devnet_setup;

    /// The encoded calldata is exactly 256 bytes (eight 32-byte words) and its
    /// first two words reproduce the proof's `A` point encoded via [`fq_word`] —
    /// i.e. the layout matches what `export_keys`/`gen_verifier` assume.
    #[test]
    fn calldata_is_eight_words_a_first() {
        let (pk, _vk) = devnet_setup();
        let input = UsageRowInput {
            work_id: [1u8; 32],
            minutes: 60,
            plays: 2,
            salt: [9u8; 32],
            price_ppm: 1_000_000,
            region_ppm: 1_000_000,
        };
        let bundle = prove(&pk, 7, &[0xABu8; 32], &[0x5cu8; 32], 1_000_000, &[input]).unwrap();
        let calldata = proof_to_evm_calldata(&bundle.proof).unwrap();
        assert_eq!(calldata.len(), 256, "8 static uint256 words");

        // The first word must be the proof's A.x, matching the standalone re-decode.
        let proof = Proof::<Bn254>::deserialize_compressed(bundle.proof.as_slice()).unwrap();
        assert_eq!(&calldata[..32], &fq_word(&proof.a.x));
    }

    /// Garbage bytes are rejected rather than panicking.
    #[test]
    fn rejects_invalid_proof_bytes() {
        assert!(proof_to_evm_calldata(&[0u8; 8]).is_none());
    }
}
