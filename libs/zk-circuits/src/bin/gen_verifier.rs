//! Solidity Groth16 verifier codegen for the CWE usage-proof circuit.
//!
//! Loads the devnet verifying key saved by `export_keys.rs` and emits
//! `chain/contracts/Groth16Verifier.sol`: a standard BN254 Groth16 pairing
//! verifier that hardcodes the verifying-key constants and checks a proof via
//! the EVM `alt_bn128` precompiles (`ecAdd` 0x06, `ecMul` 0x07, `ecPairing`
//! 0x08).
//!
//! The VK is LOADED (not re-derived) so it is byte-identical to the key
//! `export_keys.rs` baked into the proof fixture — the two artifacts must agree
//! on the verifying key AND on the field-element encoding (canonical big-endian
//! `uint256`, with G2 `Fq2` limbs reversed to `[c1, c0]`). See the matching
//! encoders in `export_keys.rs`.

use ark_bn254::{Fq, G1Affine, G2Affine};
use ark_ff::{BigInteger, PrimeField};

use cwe_zk_circuits::setup::load_vk;

/// Encode one base-field element `Fq` as a 32-byte big-endian, `0x`-prefixed
/// hex `uint256` literal — identical to `export_keys.rs`'s `fq_hex`.
///
/// `into_bigint()` undoes arkworks' internal Montgomery form to recover the
/// canonical integer; `to_bytes_be()` renders it big-endian; the value is
/// left-padded to a full 32-byte word.
fn fq_hex(f: &Fq) -> String {
    let be = f.into_bigint().to_bytes_be(); // canonical (non-Montgomery) big-endian
    let mut buf = [0u8; 32];
    buf[32 - be.len()..].copy_from_slice(&be); // left-pad to a full word
    format!("0x{}", hex::encode(buf))
}

/// Emit two `(x, y)` hex `uint256` literals for a G1 point's affine coordinates.
fn g1(p: &G1Affine) -> [String; 2] {
    [fq_hex(&p.x), fq_hex(&p.y)]
}

/// Emit the four hex `uint256` literals of a G2 point in the EVM precompile's
/// expected order: `x.c1, x.c0, y.c1, y.c0` (imaginary `Fq2` limb first).
///
/// This reversal of arkworks' `(c0, c1)` limb order is the BN254-on-EVM gotcha;
/// it is applied identically here and in `export_keys.rs` so the baked VK and
/// the proof fixture speak the same dialect.
fn g2(p: &G2Affine) -> [String; 4] {
    [
        fq_hex(&p.x.c1),
        fq_hex(&p.x.c0),
        fq_hex(&p.y.c1),
        fq_hex(&p.y.c0),
    ]
}

/// Load the devnet VK and write the generated `Groth16Verifier.sol`.
fn main() {
    // Load the SAME key export_keys.rs saved — never re-run devnet_setup here.
    let vk = load_vk("chain/zk/verifying_key.bin").expect("load chain/zk/verifying_key.bin");

    // One public input (the digest) means gamma_abc_g1 has exactly 2 entries.
    assert_eq!(
        vk.gamma_abc_g1.len(),
        2,
        "expected exactly one public input (IC length 2), got IC length {}",
        vk.gamma_abc_g1.len()
    );

    let alpha = g1(&vk.alpha_g1);
    let beta = g2(&vk.beta_g2);
    let gamma = g2(&vk.gamma_g2);
    let delta = g2(&vk.delta_g2);
    let ic0 = g1(&vk.gamma_abc_g1[0]);
    let ic1 = g1(&vk.gamma_abc_g1[1]);

    let sol = render(&alpha, &beta, &gamma, &delta, &ic0, &ic1);

    std::fs::create_dir_all("chain/contracts").expect("create chain/contracts");
    std::fs::write("chain/contracts/Groth16Verifier.sol", &sol).expect("write verifier");
    println!("gen_verifier: wrote chain/contracts/Groth16Verifier.sol");
}

/// Render the full `Groth16Verifier.sol` source with the VK constants inlined.
///
/// The pairing check follows the canonical Groth16 arrangement
/// `e(-A, B) · e(alpha, beta) · e(vk_x, gamma) · e(C, delta) == 1`, evaluated
/// in a single `ecPairing` call, with `vk_x = IC[0] + digest · IC[1]`.
fn render(
    alpha: &[String; 2],
    beta: &[String; 4],
    gamma: &[String; 4],
    delta: &[String; 4],
    ic0: &[String; 2],
    ic1: &[String; 2],
) -> String {
    format!(
        r#"// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {{IProofVerifier}} from "./interfaces/IProofVerifier.sol";

/// @title Groth16Verifier
/// @notice On-chain Groth16 (BN254) verifier for the CWE usage-proof circuit.
/// @dev GENERATED FILE — produced by `libs/zk-circuits/src/bin/gen_verifier.rs`
///      from the devnet verifying key; do not edit by hand. Re-run the
///      generator to regenerate after any circuit or setup change.
///
///      SECURITY: the baked-in verifying key comes from the DEVNET-ONLY,
///      INSECURE trusted setup (a fixed, publicly known RNG seed). Its toxic
///      waste is public, so proofs against this key are trivially forgeable.
///      This contract is for local development and tests ONLY; a production
///      deployment requires a key from a real multi-party-computation ceremony.
///
///      Field elements are encoded as canonical big-endian `uint256` values.
///      G2 points follow the `alt_bn128` precompile layout `[x.c1, x.c0]` /
///      `[y.c1, y.c0]` (imaginary Fq2 limb first). The proof fixture in
///      `chain/test/fixtures/zk_proof.json` uses the identical encoding.
contract Groth16Verifier is IProofVerifier {{
    /// @dev BN254 base field modulus `q`, used to negate a G1 point on-curve.
    uint256 internal constant Q =
        21888242871839275222246405745257275088696311157297823662689037894645226208583;

    // --- Verifying key constants (DEVNET-ONLY) ------------------------------
    // alpha (G1): (x, y)
    uint256 internal constant ALPHA_X = {alpha_x};
    uint256 internal constant ALPHA_Y = {alpha_y};
    // beta (G2): [x.c1, x.c0], [y.c1, y.c0]
    uint256 internal constant BETA_X1 = {beta_x1};
    uint256 internal constant BETA_X0 = {beta_x0};
    uint256 internal constant BETA_Y1 = {beta_y1};
    uint256 internal constant BETA_Y0 = {beta_y0};
    // gamma (G2)
    uint256 internal constant GAMMA_X1 = {gamma_x1};
    uint256 internal constant GAMMA_X0 = {gamma_x0};
    uint256 internal constant GAMMA_Y1 = {gamma_y1};
    uint256 internal constant GAMMA_Y0 = {gamma_y0};
    // delta (G2)
    uint256 internal constant DELTA_X1 = {delta_x1};
    uint256 internal constant DELTA_X0 = {delta_x0};
    uint256 internal constant DELTA_Y1 = {delta_y1};
    uint256 internal constant DELTA_Y0 = {delta_y0};
    // IC[0], IC[1] (G1) — IC[0] is the constant term, IC[1] scales the single
    // public input (the digest).
    uint256 internal constant IC0_X = {ic0_x};
    uint256 internal constant IC0_Y = {ic0_y};
    uint256 internal constant IC1_X = {ic1_x};
    uint256 internal constant IC1_Y = {ic1_y};

    /// @notice Verify a Groth16 proof of the usage circuit for one public input.
    /// @param digest The single public input: the Poseidon usage digest, as a
    ///        `uint256`. It is a canonical BN254 scalar (< the scalar field
    ///        order), so its integer value equals the in-circuit field element.
    /// @param proof ABI-encoded `(uint256[2] a, uint256[2][2] b, uint256[2] c)`
    ///        — the proof points A (G1), B (G2, `[c1, c0]` limb order) and C
    ///        (G1), exactly as written to `chain/test/fixtures/zk_proof.json`.
    /// @return True iff the proof satisfies the Groth16 pairing equation.
    function verify(bytes32 digest, bytes calldata proof) public view override returns (bool) {{
        // Decode the proof into its three curve points.
        (uint256[2] memory a, uint256[2][2] memory b, uint256[2] memory c) =
            abi.decode(proof, (uint256[2], uint256[2][2], uint256[2]));

        // vk_x = IC[0] + digest * IC[1]  (linear combination of the public input).
        uint256[2] memory vkx = _ecMul(IC1_X, IC1_Y, uint256(digest));
        vkx = _ecAdd(vkx[0], vkx[1], IC0_X, IC0_Y);

        // Negate A on the curve: -(x, y) = (x, q - y).
        uint256 negAy = a[1] == 0 ? 0 : Q - (a[1] % Q);

        // Single pairing check:
        //   e(-A, B) * e(alpha, beta) * e(vk_x, gamma) * e(C, delta) == 1
        // Each G2 element is fed [x.c1, x.c0, y.c1, y.c0] as the precompile wants.
        uint256[24] memory input;
        // pair 1: (-A, B)
        input[0] = a[0];
        input[1] = negAy;
        input[2] = b[0][0];
        input[3] = b[0][1];
        input[4] = b[1][0];
        input[5] = b[1][1];
        // pair 2: (alpha, beta)
        input[6] = ALPHA_X;
        input[7] = ALPHA_Y;
        input[8] = BETA_X1;
        input[9] = BETA_X0;
        input[10] = BETA_Y1;
        input[11] = BETA_Y0;
        // pair 3: (vk_x, gamma)
        input[12] = vkx[0];
        input[13] = vkx[1];
        input[14] = GAMMA_X1;
        input[15] = GAMMA_X0;
        input[16] = GAMMA_Y1;
        input[17] = GAMMA_Y0;
        // pair 4: (C, delta)
        input[18] = c[0];
        input[19] = c[1];
        input[20] = DELTA_X1;
        input[21] = DELTA_X0;
        input[22] = DELTA_Y1;
        input[23] = DELTA_Y0;

        return _pairing(input);
    }}

    /// @dev Elliptic-curve point addition on BN254 via the `ecAdd` precompile
    ///      (address 0x06). Reverts if the precompile call fails.
    function _ecAdd(uint256 ax, uint256 ay, uint256 bx, uint256 by)
        internal
        view
        returns (uint256[2] memory r)
    {{
        uint256[4] memory input = [ax, ay, bx, by];
        bool ok;
        assembly {{
            ok := staticcall(gas(), 0x06, input, 0x80, r, 0x40)
        }}
        require(ok, "ecAdd failed");
    }}

    /// @dev Elliptic-curve scalar multiplication on BN254 via the `ecMul`
    ///      precompile (address 0x07). Reverts if the precompile call fails.
    function _ecMul(uint256 px, uint256 py, uint256 s)
        internal
        view
        returns (uint256[2] memory r)
    {{
        uint256[3] memory input = [px, py, s];
        bool ok;
        assembly {{
            ok := staticcall(gas(), 0x07, input, 0x60, r, 0x40)
        }}
        require(ok, "ecMul failed");
    }}

    /// @dev Optimal-ate pairing product check on BN254 via the `ecPairing`
    ///      precompile (address 0x08). The input is four (G1, G2) pairs (24
    ///      words); the precompile returns 1 iff the product of pairings is the
    ///      identity in the target group. Reverts if the call fails.
    function _pairing(uint256[24] memory input) internal view returns (bool) {{
        uint256[1] memory out;
        bool ok;
        assembly {{
            ok := staticcall(gas(), 0x08, input, 0x300, out, 0x20)
        }}
        require(ok, "pairing failed");
        return out[0] == 1;
    }}
}}
"#,
        alpha_x = alpha[0],
        alpha_y = alpha[1],
        beta_x1 = beta[0],
        beta_x0 = beta[1],
        beta_y1 = beta[2],
        beta_y0 = beta[3],
        gamma_x1 = gamma[0],
        gamma_x0 = gamma[1],
        gamma_y1 = gamma[2],
        gamma_y0 = gamma[3],
        delta_x1 = delta[0],
        delta_x0 = delta[1],
        delta_y1 = delta[2],
        delta_y0 = delta[3],
        ic0_x = ic0[0],
        ic0_y = ic0[1],
        ic1_x = ic1[0],
        ic1_y = ic1[1],
    )
}
