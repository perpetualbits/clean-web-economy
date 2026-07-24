// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEConsumption} from "../contracts/CWEConsumption.sol";
import {AcceptAllVerifier} from "../contracts/AcceptAllVerifier.sol";
import {IProofVerifier} from "../contracts/interfaces/IProofVerifier.sol";

/// @notice A verifier whose result is configurable, used to exercise both the
///         accept and reject paths against the same submission plumbing.
contract MockVerifier is IProofVerifier {
    /// @notice The result `verify` returns for every call.
    bool public result;

    /// @param result_ The value `verify` should return.
    constructor(bool result_) {
        result = result_;
    }

    /// @inheritdoc IProofVerifier
    function verify(bytes32, bytes calldata) external view returns (bool) {
        return result;
    }
}

/// @notice A verifier that rejects every proof, used to exercise the reject path.
contract RejectingVerifier is IProofVerifier {
    function verify(bytes32, bytes calldata) external pure returns (bool) {
        return false;
    }
}

/// @title CWEConsumptionTest
/// @notice Unit tests for per-epoch usage submission and the verifier seam.
contract CWEConsumptionTest is Test {
    CWEConsumption internal consumption;
    address internal user = makeAddr("user");
    bytes32 internal constant TIER = keccak256("light");

    /// @notice Deploy with the Phase 1 accept-all verifier and a sane timestamp.
    function setUp() public {
        consumption = new CWEConsumption(new AcceptAllVerifier());
        // Warp to a realistic time so `currentEpoch` is a large, non-zero number.
        vm.warp(1_700_000_000);
    }

    /// @dev A one-element commitments array for brevity.
    function _commitments() internal pure returns (bytes32[] memory c) {
        c = new bytes32[](1);
        c[0] = keccak256("commit-1");
    }

    /// @dev A one-element pseudonyms array matching `_commitments()` in length.
    function _pseudonyms() internal pure returns (bytes32[] memory p) {
        p = new bytes32[](1);
        p[0] = keccak256("pseudonym-1");
    }

    /// @dev A one-element workIds array matching `_commitments()` in length.
    function _workIds() internal pure returns (bytes32[] memory w) {
        w = new bytes32[](1);
        w[0] = keccak256("work-1");
    }

    /// @dev A one-element weights array matching `_commitments()` in length.
    function _weights() internal pure returns (uint256[] memory w) {
        w = new uint256[](1);
        w[0] = 42;
    }

    /// @dev The digest bound to the fixture arrays above, for `verify` calls.
    function _digest() internal pure returns (bytes32) {
        return keccak256("digest-1");
    }

    /// @notice A first submission is recorded for the current epoch and emits
    ///         the extended event carrying the proven outputs.
    function test_submit_recordsSubmission() public {
        uint256 epoch = consumption.currentEpoch();

        vm.expectEmit(true, true, false, true, address(consumption));
        emit CWEConsumption.ConsumptionSubmitted(user, epoch, TIER, _digest(), _pseudonyms(), _workIds(), _weights());

        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
        assertTrue(consumption.hasSubmitted(epoch, user));
    }

    /// @notice A second submission in the same epoch is rejected.
    function test_submit_doubleSubmit_reverts() public {
        vm.startPrank(user);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
        uint256 epoch = consumption.currentEpoch();
        vm.expectRevert(abi.encodeWithSelector(CWEConsumption.AlreadySubmitted.selector, epoch, user));
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
        vm.stopPrank();
    }

    /// @notice After advancing to the next epoch, the same user may submit again.
    function test_submit_newEpoch_allowsResubmission() public {
        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");

        // Jump forward one full epoch window.
        vm.warp(block.timestamp + consumption.EPOCH_LENGTH());
        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
        assertTrue(consumption.hasSubmitted(consumption.currentEpoch(), user));
    }

    /// @notice An empty commitments array is rejected.
    function test_submit_noCommitments_reverts() public {
        vm.prank(user);
        vm.expectRevert(CWEConsumption.NoCommitments.selector);
        consumption.submitConsumption(
            TIER, new bytes32[](0), new bytes32[](0), new bytes32[](0), new uint256[](0), _digest(), ""
        );
    }

    /// @notice A rejecting verifier makes submission fail.
    function test_submit_proofRejected_reverts() public {
        CWEConsumption rejecting = new CWEConsumption(new RejectingVerifier());
        vm.prank(user);
        vm.expectRevert(CWEConsumption.ProofRejected.selector);
        rejecting.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
    }

    /// @notice A configurable mock verifier accepts when told to.
    function test_submit_mockVerifierAccepts() public {
        CWEConsumption accepting = new CWEConsumption(new MockVerifier(true));
        uint256 epoch = accepting.currentEpoch();
        vm.prank(user);
        accepting.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
        assertTrue(accepting.hasSubmitted(epoch, user));
    }

    /// @notice A configurable mock verifier rejects when told to.
    function test_submit_mockVerifierRejects() public {
        CWEConsumption rejecting = new CWEConsumption(new MockVerifier(false));
        vm.prank(user);
        vm.expectRevert(CWEConsumption.ProofRejected.selector);
        rejecting.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), _weights(), _digest(), "");
    }

    /// @notice Mismatched pseudonyms length reverts with ArityMismatch.
    function test_submit_pseudonymsArityMismatch_reverts() public {
        bytes32[] memory badPseudonyms = new bytes32[](2);
        badPseudonyms[0] = keccak256("pseudonym-1");
        badPseudonyms[1] = keccak256("pseudonym-2");

        vm.prank(user);
        vm.expectRevert(CWEConsumption.ArityMismatch.selector);
        consumption.submitConsumption(TIER, _commitments(), badPseudonyms, _workIds(), _weights(), _digest(), "");
    }

    /// @notice Mismatched workIds length reverts with ArityMismatch.
    function test_submit_workIdsArityMismatch_reverts() public {
        bytes32[] memory badWorkIds = new bytes32[](2);
        badWorkIds[0] = keccak256("work-1");
        badWorkIds[1] = keccak256("work-2");

        vm.prank(user);
        vm.expectRevert(CWEConsumption.ArityMismatch.selector);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), badWorkIds, _weights(), _digest(), "");
    }

    /// @notice Mismatched weights length reverts with ArityMismatch.
    function test_submit_weightsArityMismatch_reverts() public {
        uint256[] memory badWeights = new uint256[](2);
        badWeights[0] = 1;
        badWeights[1] = 2;

        vm.prank(user);
        vm.expectRevert(CWEConsumption.ArityMismatch.selector);
        consumption.submitConsumption(TIER, _commitments(), _pseudonyms(), _workIds(), badWeights, _digest(), "");
    }
}
