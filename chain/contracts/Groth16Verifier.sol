// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

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
contract Groth16Verifier {
    /// @dev BN254 base field modulus `q`, used to negate a G1 point on-curve.
    uint256 internal constant Q =
        21888242871839275222246405745257275088696311157297823662689037894645226208583;

    // --- Verifying key constants (DEVNET-ONLY) ------------------------------
    // alpha (G1): (x, y)
    uint256 internal constant ALPHA_X = 0x157b2518c66f0c7bbb243412d4301e4dec385f9d85a1ec0bd95e1465ec81d794;
    uint256 internal constant ALPHA_Y = 0x0e5438e77767a28e5d713b278a9574f40c17077252ac2036ae753ddb307d4d64;
    // beta (G2): [x.c1, x.c0], [y.c1, y.c0]
    uint256 internal constant BETA_X1 = 0x1ec41127718bef0143fc3348822c50035fbe77b8ca3d7c6d30ea17c23f041a3d;
    uint256 internal constant BETA_X0 = 0x24d8cada615309663a422ce7f9de8f37e42208c8bb3aed74a85d0eb57403af51;
    uint256 internal constant BETA_Y1 = 0x1acbea5935c09fbd46b0b17b20c13771704e7fd86e25265f727748d7b07e79d9;
    uint256 internal constant BETA_Y0 = 0x01fc50e7b32e53445c6221f5ad0f7dd774c79f66d8f3b06b136385eb18aec08a;
    // gamma (G2)
    uint256 internal constant GAMMA_X1 = 0x2aa7841feed90fb28b1152fb98a75af337c71da92fe88c643dca0dc654667d89;
    uint256 internal constant GAMMA_X0 = 0x1d66d13eef1c36a36259eff6bd26fed1de4b404452f8ba21d1ffef24741d49bd;
    uint256 internal constant GAMMA_Y1 = 0x2039f91c6abe2db461053b7c9d72c08b54f89fa219f87ad18b068e4584d29496;
    uint256 internal constant GAMMA_Y0 = 0x101ab8d3a45c16814332870ce11f5d42100ed37d60be7e8f59c25d96d4c0855f;
    // delta (G2)
    uint256 internal constant DELTA_X1 = 0x15b99e3ad0c91f3f7ac924d97dbe1eb0ddccd012f2d67ecb930f5e15f995d25c;
    uint256 internal constant DELTA_X0 = 0x1ef75365c0ce2f2f2fd8177a6f5b1e314f63202e4741dec93bdc40892505f5be;
    uint256 internal constant DELTA_Y1 = 0x10e3417df27bf1349a34eb77b49064ae97d2569605eec3e36ff0e6365f2f17f7;
    uint256 internal constant DELTA_Y0 = 0x0b93ca95ef520f06aa94fd1f271a2960d71c0c1e41090a5d3de6fcfc50cd8fc3;
    // IC[0], IC[1] (G1) — IC[0] is the constant term, IC[1] scales the single
    // public input (the digest).
    uint256 internal constant IC0_X = 0x09649e11095017f181ff5e1a3986be7b916a90b7cb7c23167b3d5eb69f5605ec;
    uint256 internal constant IC0_Y = 0x01ce3b9e9d70e00c8fe9fda8ede1bcf3ed591b74f21a89bfd0ceb9f1bee6961e;
    uint256 internal constant IC1_X = 0x0a9c38f5362e40c74400ef888095e170da30da8639f5651c909d45d0f74f2b8f;
    uint256 internal constant IC1_Y = 0x0a3a867b62fd507e8ecd65735952d1a8ea6c8bbc7bb4eced77d8dbf620490f59;

    /// @notice Verify a Groth16 proof of the usage circuit for one public input.
    /// @param digest The single public input: the Poseidon usage digest, as a
    ///        `uint256`. It is a canonical BN254 scalar (< the scalar field
    ///        order), so its integer value equals the in-circuit field element.
    /// @param proof ABI-encoded `(uint256[2] a, uint256[2][2] b, uint256[2] c)`
    ///        — the proof points A (G1), B (G2, `[c1, c0]` limb order) and C
    ///        (G1), exactly as written to `chain/test/fixtures/zk_proof.json`.
    /// @return True iff the proof satisfies the Groth16 pairing equation.
    function verify(bytes32 digest, bytes calldata proof) public view returns (bool) {
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
    }

    /// @dev Elliptic-curve point addition on BN254 via the `ecAdd` precompile
    ///      (address 0x06). Reverts if the precompile call fails.
    function _ecAdd(uint256 ax, uint256 ay, uint256 bx, uint256 by)
        internal
        view
        returns (uint256[2] memory r)
    {
        uint256[4] memory input = [ax, ay, bx, by];
        bool ok;
        assembly {
            ok := staticcall(gas(), 0x06, input, 0x80, r, 0x40)
        }
        require(ok, "ecAdd failed");
    }

    /// @dev Elliptic-curve scalar multiplication on BN254 via the `ecMul`
    ///      precompile (address 0x07). Reverts if the precompile call fails.
    function _ecMul(uint256 px, uint256 py, uint256 s)
        internal
        view
        returns (uint256[2] memory r)
    {
        uint256[3] memory input = [px, py, s];
        bool ok;
        assembly {
            ok := staticcall(gas(), 0x07, input, 0x60, r, 0x40)
        }
        require(ok, "ecMul failed");
    }

    /// @dev Optimal-ate pairing product check on BN254 via the `ecPairing`
    ///      precompile (address 0x08). The input is four (G1, G2) pairs (24
    ///      words); the precompile returns 1 iff the product of pairings is the
    ///      identity in the target group. Reverts if the call fails.
    function _pairing(uint256[24] memory input) internal view returns (bool) {
        uint256[1] memory out;
        bool ok;
        assembly {
            ok := staticcall(gas(), 0x08, input, 0x300, out, 0x20)
        }
        require(ok, "pairing failed");
        return out[0] == 1;
    }
}
