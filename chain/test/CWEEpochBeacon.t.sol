// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEEpochBeacon} from "../contracts/CWEEpochBeacon.sol";
import {Ownable} from "../contracts/utils/Ownable.sol";

/// @title CWEEpochBeaconTest
/// @notice Unit tests for the fixed-key epoch beacon: unset epochs read as zero,
///         the owner can publish a key that reads back, non-owners are rejected,
///         and publishing emits `EpochKeySet`.
contract CWEEpochBeaconTest is Test {
    CWEEpochBeacon internal beacon;
    address internal owner = makeAddr("owner");
    address internal alice = makeAddr("alice");

    /// @notice Deploy the beacon with a known, labeled owner.
    function setUp() public {
        beacon = new CWEEpochBeacon(owner);
    }

    /// @notice An epoch that was never published reads back as the zero key.
    function test_keyFor_unsetEpoch_isZero() public view {
        assertEq(beacon.keyFor(42), bytes32(0));
    }

    /// @notice The owner can publish a key for an epoch and read it back.
    function test_setKey_ownerCanSetAndRead() public {
        bytes32 key = keccak256("epoch-7-key");
        vm.prank(owner);
        beacon.setKey(7, key);
        assertEq(beacon.keyFor(7), key);
    }

    /// @notice A non-owner calling `setKey` reverts.
    function test_setKey_onlyOwner_reverts() public {
        vm.prank(alice);
        vm.expectRevert(Ownable.NotOwner.selector);
        beacon.setKey(1, keccak256("nope"));
    }

    /// @notice Publishing a key emits `EpochKeySet` with the epoch and key.
    function test_setKey_emitsEpochKeySet() public {
        bytes32 key = keccak256("epoch-3-key");
        vm.expectEmit(true, false, false, true, address(beacon));
        emit CWEEpochBeacon.EpochKeySet(3, key);
        vm.prank(owner);
        beacon.setKey(3, key);
    }
}
