// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";
import {Groth16Verifier} from "../contracts/Groth16Verifier.sol";

/// @title Groth16VerifierTest
/// @notice Real end-to-end gate for the generated BN254 Groth16 verifier: it
///         runs the exported proof fixture through the on-chain pairing check via
///         the `alt_bn128` precompiles.
/// @dev The fixture `test/fixtures/zk_proof.json` is produced by
///      `libs/zk-circuits/src/bin/export_keys.rs` and encodes a proof that the
///      Rust verifier already accepts. This test proves the EVM encoding is
///      consistent with that: the good digest verifies, a tampered digest does
///      not. Both assertions reflect genuine precompile execution.
contract Groth16VerifierTest is Test {
    using stdJson for string;

    /// @dev The verifier under test, deployed fresh for each test.
    Groth16Verifier internal verifier;

    /// @notice Deploy a fresh `Groth16Verifier` before each test.
    function setUp() public {
        verifier = new Groth16Verifier();
    }

    /// @notice The known-good digest and proof from the fixture verify on-chain.
    function test_AcceptsValidProof() public view {
        bytes32 digest = _digest(false);
        bytes memory proof = _proof();
        assertTrue(verifier.verify(digest, proof), "good proof must verify");
    }

    /// @notice The same proof against a tampered digest is rejected on-chain.
    function test_RejectsTamperedDigest() public view {
        bytes32 badDigest = _digest(true);
        bytes memory proof = _proof();
        assertFalse(verifier.verify(badDigest, proof), "tampered digest must fail");
    }

    /// @dev Read the fixture and return either the valid `digest` or, when `bad`
    ///      is true, the `bad_digest` (the digest with its top byte flipped).
    /// @param bad Whether to return the tampered digest instead of the valid one.
    /// @return The requested digest as a `bytes32`.
    function _digest(bool bad) internal view returns (bytes32) {
        string memory json = vm.readFile("test/fixtures/zk_proof.json");
        return vm.parseJsonBytes32(json, bad ? ".bad_digest" : ".digest");
    }

    /// @dev Read the fixture's proof points and ABI-encode them exactly as the
    ///      verifier's `verify` decodes them: `(uint256[2] a, uint256[2][2] b,
    ///      uint256[2] c)`, where `b` keeps the fixture's `[[x.c1,x.c0],
    ///      [y.c1,y.c0]]` limb order (the `alt_bn128` precompile layout).
    /// @return The ABI-encoded proof bytes to pass to `verify`.
    function _proof() internal view returns (bytes memory) {
        string memory json = vm.readFile("test/fixtures/zk_proof.json");

        // G1 point A = (x, y).
        uint256[2] memory a;
        a[0] = vm.parseJsonUint(json, ".a[0]");
        a[1] = vm.parseJsonUint(json, ".a[1]");

        // G2 point B = [[x.c1, x.c0], [y.c1, y.c0]] — parsed element-wise so the
        // nested-array limb order matches the fixture exactly.
        uint256[2][2] memory b;
        b[0][0] = vm.parseJsonUint(json, ".b[0][0]");
        b[0][1] = vm.parseJsonUint(json, ".b[0][1]");
        b[1][0] = vm.parseJsonUint(json, ".b[1][0]");
        b[1][1] = vm.parseJsonUint(json, ".b[1][1]");

        // G1 point C = (x, y).
        uint256[2] memory c;
        c[0] = vm.parseJsonUint(json, ".c[0]");
        c[1] = vm.parseJsonUint(json, ".c[1]");

        return abi.encode(a, b, c);
    }
}
