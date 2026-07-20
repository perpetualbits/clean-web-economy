// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEConsumption} from "../contracts/CWEConsumption.sol";
import {AcceptAllVerifier} from "../contracts/AcceptAllVerifier.sol";
import {IProofVerifier} from "../contracts/interfaces/IProofVerifier.sol";

/// @notice A verifier that rejects every proof, used to exercise the reject path.
contract RejectingVerifier is IProofVerifier {
    function verify(bytes32[] calldata, bytes calldata) external pure returns (bool) {
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

    /// @notice A first submission is recorded for the current epoch.
    function test_submit_recordsSubmission() public {
        uint256 epoch = consumption.currentEpoch();
        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), "");
        assertTrue(consumption.hasSubmitted(epoch, user));
    }

    /// @notice A second submission in the same epoch is rejected.
    function test_submit_doubleSubmit_reverts() public {
        vm.startPrank(user);
        consumption.submitConsumption(TIER, _commitments(), "");
        uint256 epoch = consumption.currentEpoch();
        vm.expectRevert(
            abi.encodeWithSelector(CWEConsumption.AlreadySubmitted.selector, epoch, user)
        );
        consumption.submitConsumption(TIER, _commitments(), "");
        vm.stopPrank();
    }

    /// @notice After advancing to the next epoch, the same user may submit again.
    function test_submit_newEpoch_allowsResubmission() public {
        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), "");

        // Jump forward one full epoch window.
        vm.warp(block.timestamp + consumption.EPOCH_LENGTH());
        vm.prank(user);
        consumption.submitConsumption(TIER, _commitments(), "");
        assertTrue(consumption.hasSubmitted(consumption.currentEpoch(), user));
    }

    /// @notice An empty commitments array is rejected.
    function test_submit_noCommitments_reverts() public {
        vm.prank(user);
        vm.expectRevert(CWEConsumption.NoCommitments.selector);
        consumption.submitConsumption(TIER, new bytes32[](0), "");
    }

    /// @notice A rejecting verifier makes submission fail.
    function test_submit_proofRejected_reverts() public {
        CWEConsumption rejecting = new CWEConsumption(new RejectingVerifier());
        vm.prank(user);
        vm.expectRevert(CWEConsumption.ProofRejected.selector);
        rejecting.submitConsumption(TIER, _commitments(), "");
    }
}
