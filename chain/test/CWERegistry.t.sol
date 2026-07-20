// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";

/// @title CWERegistryTest
/// @notice Unit tests for work registration, split validation, and update rules.
contract CWERegistryTest is Test {
    CWERegistry internal registry;
    address internal owner = makeAddr("owner");
    address internal creator = makeAddr("creator");
    address internal other = makeAddr("other");

    bytes32 internal constant WORK = keccak256("work-A");
    address payable internal payee1 = payable(makeAddr("payee1"));
    address payable internal payee2 = payable(makeAddr("payee2"));

    /// @notice Deploy as owner and allowlist `creator`.
    function setUp() public {
        vm.prank(owner);
        registry = new CWERegistry(owner);
        vm.prank(owner);
        registry.setVerifiedCreator(creator, true);
    }

    /// @dev Build a two-payee split (60% / 40% in ppm).
    function _splitArrays()
        internal
        view
        returns (address payable[] memory payees, uint96[] memory splits)
    {
        payees = new address payable[](2);
        payees[0] = payee1;
        payees[1] = payee2;
        splits = new uint96[](2);
        splits[0] = 600_000;
        splits[1] = 400_000;
    }

    /// @notice A verified creator can register a valid work.
    function test_register_validWork() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        vm.prank(creator);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));

        assertTrue(registry.isRegistered(WORK));
        assertEq(registry.payeesOf(WORK).length, 2);
        assertEq(registry.splitsOf(WORK)[0], 600_000);
        assertEq(registry.pricePerMinOf(WORK), 1000);
    }

    /// @notice Non-verified addresses cannot register.
    function test_register_notVerified_reverts() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        vm.prank(other);
        vm.expectRevert(CWERegistry.NotVerifiedCreator.selector);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));
    }

    /// @notice Splits that do not sum to 1_000_000 ppm are rejected.
    function test_register_splitsNotFull_reverts() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        splits[1] = 300_000; // sum is now 900_000, not 1_000_000
        vm.prank(creator);
        vm.expectRevert(abi.encodeWithSelector(CWERegistry.SplitsNotFull.selector, 900_000));
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));
    }

    /// @notice Mismatched payee/split array lengths are rejected.
    function test_register_badLengths_reverts() public {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payee1;
        payees[1] = payee2;
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        vm.prank(creator);
        vm.expectRevert(CWERegistry.BadArrayLengths.selector);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));
    }

    /// @notice A zero payee address is rejected.
    function test_register_zeroPayee_reverts() public {
        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(address(0));
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        vm.prank(creator);
        vm.expectRevert(CWERegistry.ZeroPayee.selector);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));
    }

    /// @notice Only the original registrant may update a work.
    function test_update_onlyRegistrant() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        vm.prank(creator);
        registry.registerWork(WORK, payees, splits, 1000, bytes32("EU"));

        // Allowlist `other`, but they still cannot update someone else's work.
        vm.prank(owner);
        registry.setVerifiedCreator(other, true);
        vm.prank(other);
        vm.expectRevert(CWERegistry.NotRegistrant.selector);
        registry.registerWork(WORK, payees, splits, 2000, bytes32("US"));

        // The original registrant can update it.
        vm.prank(creator);
        registry.registerWork(WORK, payees, splits, 2000, bytes32("US"));
        assertEq(registry.pricePerMinOf(WORK), 2000);
    }
}
