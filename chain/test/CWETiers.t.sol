// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWETiers} from "../contracts/CWETiers.sol";
import {Ownable} from "../contracts/utils/Ownable.sol";

/// @title CWETiersTest
/// @notice Unit tests for the tier table and subscription payment flow.
contract CWETiersTest is Test {
    CWETiers internal tiers;
    address internal owner = makeAddr("owner");
    address internal user = makeAddr("user");
    address payable internal pool = payable(makeAddr("pool"));

    /// @dev A stable tier id used throughout the tests.
    bytes32 internal constant LIGHT = keccak256("light");
    uint256 internal constant FEE = 1 ether;

    /// @notice Deploy the contract as `owner`, set a fee, and point at the pool.
    function setUp() public {
        vm.startPrank(owner);
        tiers = new CWETiers(owner);
        tiers.setFee(LIGHT, FEE);
        tiers.setPayoutPool(pool);
        vm.stopPrank();
    }

    /// @notice `feeOf` returns what the owner set.
    function test_feeOf_returnsConfiguredFee() public view {
        assertEq(tiers.feeOf(LIGHT), FEE);
    }

    /// @notice A correct-fee subscription records the tier and forwards funds.
    function test_subscribe_recordsTierAndForwardsFee() public {
        vm.deal(user, FEE);
        vm.prank(user);
        tiers.subscribe{value: FEE}(LIGHT);

        assertEq(tiers.activeTier(user), LIGHT);
        assertEq(pool.balance, FEE); // fee forwarded to the pool
        assertEq(address(tiers).balance, 0); // nothing stuck in the tier contract
    }

    /// @notice Paying the wrong amount is rejected.
    function test_subscribe_wrongFee_reverts() public {
        vm.deal(user, FEE);
        vm.prank(user);
        vm.expectRevert(abi.encodeWithSelector(CWETiers.WrongFee.selector, FEE - 1, FEE));
        tiers.subscribe{value: FEE - 1}(LIGHT);
    }

    /// @notice Subscribing to a tier with no fee configured is rejected.
    function test_subscribe_unknownTier_reverts() public {
        bytes32 unknown = keccak256("does-not-exist");
        vm.deal(user, FEE);
        vm.prank(user);
        vm.expectRevert(abi.encodeWithSelector(CWETiers.UnknownTier.selector, unknown));
        tiers.subscribe{value: FEE}(unknown);
    }

    /// @notice Subscribing before the pool is set is rejected (funds would stick).
    function test_subscribe_poolUnset_reverts() public {
        CWETiers fresh = new CWETiers(owner);
        vm.prank(owner);
        fresh.setFee(LIGHT, FEE);
        vm.deal(user, FEE);
        vm.prank(user);
        vm.expectRevert(CWETiers.PayoutPoolUnset.selector);
        fresh.subscribe{value: FEE}(LIGHT);
    }

    /// @notice Only the owner may set fees.
    function test_setFee_onlyOwner() public {
        vm.prank(user);
        vm.expectRevert(Ownable.NotOwner.selector);
        tiers.setFee(LIGHT, 2 ether);
    }
}
