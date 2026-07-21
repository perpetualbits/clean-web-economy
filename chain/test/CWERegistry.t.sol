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
    bytes32 internal constant CONTENT = keccak256("content-A");
    address payable internal payee1;
    address payable internal payee2;
    uint256 internal payee1Key;
    uint256 internal payee2Key;

    /// @notice Deploy as owner, allowlist `creator`, and mint keyed payees so
    ///         consent signatures can be produced for the default split.
    function setUp() public {
        vm.prank(owner);
        registry = new CWERegistry(owner);
        vm.prank(owner);
        registry.setVerifiedCreator(creator, true);

        (address p1, uint256 k1) = makeAddrAndKey("payee1");
        (address p2, uint256 k2) = makeAddrAndKey("payee2");
        payee1 = payable(p1);
        payee2 = payable(p2);
        payee1Key = k1;
        payee2Key = k2;
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

    /// @dev Build consent signatures matching `_splitArrays()`'s payees/splits.
    function _defaultConsents(uint96[] memory splits) internal view returns (bytes[] memory sigs) {
        sigs = new bytes[](2);
        sigs[0] = _consent(payee1Key, WORK, CONTENT, payee1, splits[0]);
        sigs[1] = _consent(payee2Key, WORK, CONTENT, payee2, splits[1]);
    }

    /// @notice A verified creator can register a valid work.
    function test_register_validWork() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        bytes[] memory sigs = _defaultConsents(splits);
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));

        assertTrue(registry.isRegistered(WORK));
        assertEq(registry.payeesOf(WORK).length, 2);
        assertEq(registry.splitsOf(WORK)[0], 600_000);
        assertEq(registry.pricePerMinOf(WORK), 1000);
    }

    /// @notice Non-verified addresses cannot register.
    function test_register_notVerified_reverts() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        bytes[] memory sigs = _defaultConsents(splits);
        vm.prank(other);
        vm.expectRevert(CWERegistry.NotVerifiedCreator.selector);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @notice Splits that do not sum to 1_000_000 ppm are rejected.
    function test_register_splitsNotFull_reverts() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        splits[1] = 300_000; // sum is now 900_000, not 1_000_000
        bytes[] memory sigs = _defaultConsents(splits);
        vm.prank(creator);
        vm.expectRevert(abi.encodeWithSelector(CWERegistry.SplitsNotFull.selector, 900_000));
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @notice Mismatched payee/split array lengths are rejected.
    function test_register_badLengths_reverts() public {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payee1;
        payees[1] = payee2;
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        // Length mismatch reverts before consent is checked; sigs may be empty.
        bytes[] memory sigs = new bytes[](0);
        vm.prank(creator);
        vm.expectRevert(CWERegistry.BadArrayLengths.selector);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @notice A zero payee address is rejected.
    function test_register_zeroPayee_reverts() public {
        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(address(0));
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        // Zero-payee reverts before consent is checked; sigs may be empty.
        bytes[] memory sigs = new bytes[](1);
        vm.prank(creator);
        vm.expectRevert(CWERegistry.ZeroPayee.selector);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @notice Only the original registrant may update a work.
    function test_update_onlyRegistrant() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        bytes[] memory sigs = _defaultConsents(splits);
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));

        // Allowlist `other`, but they still cannot update someone else's work.
        vm.prank(owner);
        registry.setVerifiedCreator(other, true);
        vm.prank(other);
        vm.expectRevert(CWERegistry.NotRegistrant.selector);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 2000, bytes32("US"));

        // The original registrant can update it.
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 2000, bytes32("US"));
        assertEq(registry.pricePerMinOf(WORK), 2000);
    }

    /// @notice Updating a work preserves its original registration timestamp, which
    ///         is the priority key the escrow challenge rule relies on.
    function test_update_preservesRegisteredAt() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        bytes[] memory sigs = _defaultConsents(splits);

        vm.warp(1000);
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
        assertEq(registry.registeredAtOf(WORK), 1000);

        // A later update by the registrant must NOT move the priority timestamp.
        vm.warp(5000);
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 2000, bytes32("US"));
        assertEq(registry.registeredAtOf(WORK), 1000, "registeredAt must be first-registration time");
    }

    /// @notice The registrant and region are readable after registration.
    function test_getters_exposeRegistrantAndRegion() public {
        (address payable[] memory payees, uint96[] memory splits) = _splitArrays();
        bytes[] memory sigs = _defaultConsents(splits);
        vm.prank(creator);
        registry.registerWork(WORK, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));

        assertEq(registry.registrantOf(WORK), creator);
        assertEq(registry.regionRuleOf(WORK), bytes32("EU"));
    }

    /// @notice An unregistered work reports the zero registrant.
    function test_registrantOf_unregisteredIsZero() public view {
        assertEq(registry.registrantOf(keccak256("nope")), address(0));
    }

    /// A work registers only when every payee has consented to their share, and it
    /// records the content id and registration timestamp.
    function test_register_withConsent() public {
        (address alice, uint256 aliceK) = makeAddrAndKey("alice");
        (address bob, uint256 bobK) = makeAddrAndKey("bob");
        vm.prank(owner);
        registry.setVerifiedCreator(creator, true);

        bytes32 workId = keccak256("song-A");
        bytes32 contentId = keccak256("content-A");
        address payable[] memory payees = new address payable[](2);
        payees[0] = payable(alice); payees[1] = payable(bob);
        uint96[] memory splits = new uint96[](2);
        splits[0] = 700_000; splits[1] = 300_000;

        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(aliceK, workId, contentId, alice, splits[0]);
        sigs[1] = _consent(bobK, workId, contentId, bob, splits[1]);

        vm.warp(1000);
        vm.prank(creator);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));

        assertEq(registry.contentIdOf(workId), contentId);
        assertEq(registry.registeredAtOf(workId), 1000);
    }

    /// A missing/forged consent signature is rejected.
    function test_register_badConsent_reverts() public {
        (address alice, ) = makeAddrAndKey("alice");
        (, uint256 malloryK) = makeAddrAndKey("mallory");
        vm.prank(owner); registry.setVerifiedCreator(creator, true);

        bytes32 workId = keccak256("song-B"); bytes32 contentId = keccak256("content-B");
        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(alice);
        uint96[] memory splits = new uint96[](1); splits[0] = 1_000_000;
        bytes[] memory sigs = new bytes[](1);
        // Signed by mallory, not alice.
        sigs[0] = _consent(malloryK, workId, contentId, alice, splits[0]);

        vm.prank(creator);
        vm.expectRevert(CWERegistry.BadConsent.selector);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// Helper: EIP-191 personal-sign of the consent digest by key `k`.
    function _consent(uint256 k, bytes32 w, bytes32 c, address payee, uint96 share)
        internal view returns (bytes memory)
    {
        bytes32 digest = registry.consentDigest(w, c, payee, share);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(k, eth);
        return abi.encodePacked(r, s, v);
    }
}
