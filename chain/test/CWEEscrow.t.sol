// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEEscrow} from "../contracts/CWEEscrow.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {EarliestRegistrationArbiter} from "../contracts/EarliestRegistrationArbiter.sol";

/// @notice A payee that tries to re-enter `release` when it receives ETH, used
///         to prove the reentrancy guard blocks nested releases.
contract ReentrantEscrowPayee {
    CWEEscrow private immutable escrow;
    uint256 private epochId;
    bytes32 private workId;
    bool private armed;

    constructor(CWEEscrow escrow_) {
        escrow = escrow_;
    }

    /// @dev Store the parameters of a re-entrant release and arm the attack.
    function arm(uint256 epochId_, bytes32 workId_) external {
        epochId = epochId_;
        workId = workId_;
        armed = true;
    }

    /// @dev On receiving ETH, attempt exactly one re-entrant release. The guard
    ///      makes this call revert, which propagates back as a failed transfer.
    receive() external payable {
        if (armed) {
            armed = false; // only try once
            escrow.release(epochId, workId);
        }
    }
}

/// @title CWEEscrowTest
/// @notice Unit tests for the fingerprint-match escrow: commit, challenge
///         reassignment by registration priority, release after the challenge
///         window, double-release prevention, solvency, and reentrancy safety.
contract CWEEscrowTest is Test {
    CWEEscrow internal escrow;
    CWERegistry internal registry;
    EarliestRegistrationArbiter internal arbiter;

    address internal owner = makeAddr("owner");
    address internal creator = makeAddr("creator");
    address internal aggregator = makeAddr("aggregator");
    address internal challenger = makeAddr("challenger");

    address payable internal payeeA1;
    address payable internal payeeA2;
    uint256 internal payeeA1Key;
    uint256 internal payeeA2Key;

    address payable internal payeeB1;
    address payable internal payeeB2;
    uint256 internal payeeB1Key;
    uint256 internal payeeB2Key;

    bytes32 internal constant CONTENT_A = keccak256("content-A");
    bytes32 internal constant CONTENT_B = keccak256("content-B");
    uint256 internal constant EPOCH = 0;

    /// @notice Deploy the registry, arbiter, and escrow, and mint keyed payees
    ///         for two distinct works so consent signatures can be produced.
    function setUp() public {
        vm.startPrank(owner);
        registry = new CWERegistry(owner);
        registry.setVerifiedCreator(creator, true);
        vm.stopPrank();

        arbiter = new EarliestRegistrationArbiter(registry);
        escrow = new CWEEscrow(registry, aggregator, arbiter);

        (address a1, uint256 ak1) = makeAddrAndKey("payeeA1");
        (address a2, uint256 ak2) = makeAddrAndKey("payeeA2");
        payeeA1 = payable(a1);
        payeeA2 = payable(a2);
        payeeA1Key = ak1;
        payeeA2Key = ak2;

        (address b1, uint256 bk1) = makeAddrAndKey("payeeB1");
        (address b2, uint256 bk2) = makeAddrAndKey("payeeB2");
        payeeB1 = payable(b1);
        payeeB2 = payable(b2);
        payeeB1Key = bk1;
        payeeB2Key = bk2;
    }

    /// @dev EIP-191 personal-sign of the consent digest by key `k`, mirroring
    ///      `CWERegistryTest._consent` so consents validate identically.
    function _consent(uint256 k, bytes32 w, bytes32 c, address payee, uint96 share)
        internal
        view
        returns (bytes memory)
    {
        bytes32 digest = registry.consentDigest(w, c, payee, share);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(k, eth);
        return abi.encodePacked(r, s, v);
    }

    /// @dev Register `workId` with the "A" payee pair (60/40 split).
    function _registerA(bytes32 workId) internal {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payeeA1;
        payees[1] = payeeA2;
        uint96[] memory splits = new uint96[](2);
        splits[0] = 600_000;
        splits[1] = 400_000;
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(payeeA1Key, workId, CONTENT_A, payeeA1, splits[0]);
        sigs[1] = _consent(payeeA2Key, workId, CONTENT_A, payeeA2, splits[1]);
        vm.prank(creator);
        registry.registerWork(workId, CONTENT_A, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev Register `workId` with the "B" payee pair (50/50 split).
    function _registerB(bytes32 workId) internal {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payeeB1;
        payees[1] = payeeB2;
        uint96[] memory splits = new uint96[](2);
        splits[0] = 500_000;
        splits[1] = 500_000;
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(payeeB1Key, workId, CONTENT_B, payeeB1, splits[0]);
        sigs[1] = _consent(payeeB2Key, workId, CONTENT_B, payeeB2, splits[1]);
        vm.prank(creator);
        registry.registerWork(workId, CONTENT_B, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev Register `workId` with the "A" payee pair (60/40 split) under an
    ///      explicit `contentId`, so two distinct work ids can share content.
    function _registerAWithContent(bytes32 workId, bytes32 contentId) internal {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payeeA1;
        payees[1] = payeeA2;
        uint96[] memory splits = new uint96[](2);
        splits[0] = 600_000;
        splits[1] = 400_000;
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(payeeA1Key, workId, contentId, payeeA1, splits[0]);
        sigs[1] = _consent(payeeA2Key, workId, contentId, payeeA2, splits[1]);
        vm.prank(creator);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev Register `workId` with the "B" payee pair (50/50 split) under an
    ///      explicit `contentId`, so two distinct work ids can share content.
    function _registerBWithContent(bytes32 workId, bytes32 contentId) internal {
        address payable[] memory payees = new address payable[](2);
        payees[0] = payeeB1;
        payees[1] = payeeB2;
        uint96[] memory splits = new uint96[](2);
        splits[0] = 500_000;
        splits[1] = 500_000;
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(payeeB1Key, workId, contentId, payeeB1, splits[0]);
        sigs[1] = _consent(payeeB2Key, workId, contentId, payeeB2, splits[1]);
        vm.prank(creator);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @notice Only the aggregator may commit an escrow.
    function test_commit_onlyAggregator() public {
        vm.expectRevert(CWEEscrow.NotAggregator.selector);
        escrow.commit(EPOCH, keccak256("work"), 1 ether);
    }

    /// @notice Committing more credit than the pool holds is rejected.
    function test_commit_insolvent_reverts() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        vm.deal(address(escrow), 1 ether);
        vm.prank(aggregator);
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.Insolvent.selector, 1 ether, 2 ether));
        escrow.commit(EPOCH, workId, 2 ether);
    }

    /// @notice The same `(epoch, work)` pair cannot be committed twice.
    function test_commit_twice_reverts() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        vm.deal(address(escrow), 2 ether);
        vm.startPrank(aggregator);
        escrow.commit(EPOCH, workId, 1 ether);
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.AlreadyCommitted.selector, EPOCH, workId));
        escrow.commit(EPOCH, workId, 1 ether);
        vm.stopPrank();
    }

    /// @notice Committing an unregistered work reverts, closing the fund-lock
    ///         gap where an unregistered incumbent could never be released or
    ///         (previously) dislodged by a challenge.
    function test_commit_unregisteredWork_reverts() public {
        bytes32 workId = keccak256("work-unregistered");
        vm.deal(address(escrow), 1 ether);
        vm.prank(aggregator);
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.WorkNotRegistered.selector, workId));
        escrow.commit(EPOCH, workId, 1 ether);
    }

    /// @notice A committed escrow cannot be released before its challenge window.
    function test_release_tooEarly_reverts() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        vm.deal(address(escrow), 1 ether);
        vm.prank(aggregator);
        escrow.commit(EPOCH, workId, 1 ether);

        // Still within epoch 0; releaseEpoch is 1.
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.TooEarly.selector, EPOCH, workId));
        escrow.release(EPOCH, workId);
    }

    /// @notice After the challenge window elapses, release pays payees per split.
    function test_release_paysPayeesPerSplit_afterWindow() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, workId, amount);

        // Warp past the end of epoch 0 into epoch 1, at/after releaseEpoch.
        vm.warp(30 days);
        escrow.release(EPOCH, workId);

        assertEq(payeeA1.balance, 0.6 ether);
        assertEq(payeeA2.balance, 0.4 ether);
        assertEq(payeeA1.balance + payeeA2.balance, amount);
        assertTrue(escrow.isReleased(EPOCH, workId));
        assertEq(escrow.liability(), 0);

        // The escrow's amount is zeroed on release, so a released escrow is no
        // longer reported as outstanding credit.
        assertEq(escrow.escrowOf(EPOCH, workId), 0);
    }

    /// @notice The same escrow cannot be released twice.
    function test_release_doubleRelease_reverts() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, workId, amount);

        vm.warp(30 days);
        escrow.release(EPOCH, workId);

        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.AlreadyReleased.selector, EPOCH, workId));
        escrow.release(EPOCH, workId);
    }

    /// @notice A challenger with a strictly earlier registration reassigns the
    ///         escrow; releasing then pays the challenger's payees. Both works
    ///         share a content id, since a challenge only concerns the same
    ///         content.
    function test_challenge_earlierRegistration_reassigns() public {
        bytes32 challengerWork = keccak256("work-challenger");
        bytes32 escrowedWork = keccak256("work-escrowed");

        // The challenger's work is registered first (earlier priority)...
        vm.warp(1000);
        _registerAWithContent(challengerWork, CONTENT_A);
        // ...the fingerprint-matched (escrowed) work, over the SAME content,
        // registers later.
        vm.warp(2000);
        _registerBWithContent(escrowedWork, CONTENT_A);

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, escrowedWork, amount);

        // Still within the challenge window (epoch 0).
        vm.prank(challenger);
        escrow.challenge(EPOCH, escrowedWork, challengerWork);

        // The escrow moved fully from the old work to the challenger's work.
        assertEq(escrow.escrowOf(EPOCH, escrowedWork), 0);
        assertEq(escrow.escrowOf(EPOCH, challengerWork), amount);
        assertEq(escrow.releaseEpochOf(EPOCH, challengerWork), EPOCH + 1);

        // The old slot is cleared, so it can no longer be released.
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.NotEscrowed.selector, EPOCH, escrowedWork));
        escrow.release(EPOCH, escrowedWork);

        // Past the window, releasing the challenger's work pays the "A" payees
        // (the challenger's registered payee set).
        vm.warp(30 days);
        escrow.release(EPOCH, challengerWork);
        assertEq(payeeA1.balance, 0.6 ether);
        assertEq(payeeA2.balance, 0.4 ether);
        assertTrue(escrow.isReleased(EPOCH, challengerWork));
    }

    /// @notice The challenge window runs from commit time, not the usage epoch,
    ///         so a settlement that lags the usage epoch (the normal case — an
    ///         epoch can only be settled once it has closed) still leaves a full
    ///         window open. This reproduces the production timing that a
    ///         same-epoch commit hides: usage in epoch 0, but settlement/commit
    ///         several epochs later.
    function test_commit_windowRunsFromCommitNotUsageEpoch() public {
        bytes32 challengerWork = keccak256("work-challenger");
        bytes32 escrowedWork = keccak256("work-escrowed");

        // Both works registered in the usage epoch (epoch 0), real owner first.
        vm.warp(1000);
        _registerAWithContent(challengerWork, CONTENT_A);
        vm.warp(2000);
        _registerBWithContent(escrowedWork, CONTENT_A);

        // Settlement runs five epochs after the usage epoch it is settling.
        uint256 lateEpoch = 5;
        vm.warp(lateEpoch * 30 days + 1);
        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, escrowedWork, amount); // EPOCH == 0, the usage epoch

        // The release epoch is one window past the COMMIT epoch (6), not past the
        // usage epoch (which would be 1 — already elapsed, a zero-length window).
        assertEq(escrow.releaseEpochOf(EPOCH, escrowedWork), lateEpoch + 1);

        // The window is genuinely open: release is too early right after commit...
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.TooEarly.selector, EPOCH, escrowedWork));
        escrow.release(EPOCH, escrowedWork);

        // ...and the earlier-registered real owner can still challenge and win.
        vm.prank(challenger);
        escrow.challenge(EPOCH, escrowedWork, challengerWork);
        assertEq(escrow.escrowOf(EPOCH, escrowedWork), 0);
        assertEq(escrow.escrowOf(EPOCH, challengerWork), amount);

        // After the window elapses, the reassigned escrow pays the real owner.
        vm.warp((lateEpoch + 1) * 30 days);
        escrow.release(EPOCH, challengerWork);
        assertEq(payeeA1.balance, 0.6 ether);
        assertEq(payeeA2.balance, 0.4 ether);
    }

    /// @notice A challenger registered LATER than the escrowed work fails, even
    ///         though it shares the escrowed work's content id.
    function test_challenge_laterRegistration_reverts() public {
        bytes32 escrowedWork = keccak256("work-escrowed");
        bytes32 challengerWork = keccak256("work-challenger");

        // The escrowed work registers first (earlier priority)...
        vm.warp(1000);
        _registerAWithContent(escrowedWork, CONTENT_A);
        // ...the would-be challenger, over the SAME content, registers later,
        // so it cannot win.
        vm.warp(2000);
        _registerBWithContent(challengerWork, CONTENT_A);

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, escrowedWork, amount);

        vm.expectRevert(CWEEscrow.ChallengeFailed.selector);
        escrow.challenge(EPOCH, escrowedWork, challengerWork);

        // The escrow is untouched.
        assertEq(escrow.escrowOf(EPOCH, escrowedWork), amount);
    }

    /// @notice A challenge whose challenger work has a DIFFERENT content id
    ///         than the escrowed work reverts, closing the fund-misdirection
    ///         gap where an unrelated earlier-registered work could steal an
    ///         escrow over different content.
    function test_challenge_contentMismatch_reverts() public {
        bytes32 escrowedWork = keccak256("work-escrowed");
        bytes32 challengerWork = keccak256("work-challenger");

        // The challenger's work is registered earlier, but over DIFFERENT
        // content than the escrowed work.
        vm.warp(1000);
        _registerA(challengerWork); // CONTENT_A
        vm.warp(2000);
        _registerB(escrowedWork); // CONTENT_B

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, escrowedWork, amount);

        vm.expectRevert(
            abi.encodeWithSelector(CWEEscrow.ContentMismatch.selector, CONTENT_B, CONTENT_A)
        );
        escrow.challenge(EPOCH, escrowedWork, challengerWork);

        // The escrow is untouched.
        assertEq(escrow.escrowOf(EPOCH, escrowedWork), amount);
    }

    /// @notice Challenging an escrow with itself as the challenger reverts
    ///         instead of silently no-oping and emitting a spurious event.
    function test_challenge_selfChallenge_reverts() public {
        bytes32 workId = keccak256("work-A");
        vm.warp(1000);
        _registerA(workId);

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, workId, amount);

        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.SelfChallenge.selector, workId));
        escrow.challenge(EPOCH, workId, workId);
    }

    /// @notice The arbiter must never let an unregistered work win: an
    ///         unregistered incumbent (registration timestamp zero) must not
    ///         be able to out-rank a registered challenger, or an unregistered
    ///         escrow could never be dislodged or released.
    function test_arbiter_unregisteredLoses_toRegisteredWork() public {
        bytes32 unregisteredWork = keccak256("work-unregistered");
        bytes32 registeredWork = keccak256("work-registered");

        vm.warp(1000);
        _registerA(registeredWork);

        // Unregistered vs registered, in both argument orders: the registered
        // work always wins.
        assertEq(arbiter.resolve(unregisteredWork, registeredWork), registeredWork);
        assertEq(arbiter.resolve(registeredWork, unregisteredWork), registeredWork);
    }

    /// @notice A payee that re-enters `release` is blocked by the guard, causing
    ///         the whole release to revert (no funds move, nothing is marked).
    function test_release_reentrancy_blocked() public {
        bytes32 workId = keccak256("work-attack");

        // A contract address has no private key, so no signature can ever
        // ecrecover to it; place the attacker's bytecode at an address whose
        // key we do know, so a genuine consent signature validates while the
        // reentrancy trigger still fires on that address.
        (address attackerAddr, uint256 attackerKey) = makeAddrAndKey("attacker");
        vm.etch(attackerAddr, address(new ReentrantEscrowPayee(escrow)).code);
        ReentrantEscrowPayee attacker = ReentrantEscrowPayee(payable(attackerAddr));

        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(attackerAddr);
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        bytes32 contentId = keccak256("content-attack");
        bytes[] memory sigs = new bytes[](1);
        sigs[0] = _consent(attackerKey, workId, contentId, attackerAddr, splits[0]);
        vm.warp(1000);
        vm.prank(creator);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));

        uint256 amount = 1 ether;
        vm.deal(address(escrow), amount);
        vm.prank(aggregator);
        escrow.commit(EPOCH, workId, amount);

        vm.warp(30 days);
        attacker.arm(EPOCH, workId);

        // The re-entrant call reverts inside the guard, surfacing as a failed
        // transfer to the attacker.
        vm.expectRevert(abi.encodeWithSelector(CWEEscrow.PayoutFailed.selector, address(attacker)));
        escrow.release(EPOCH, workId);

        // Nothing was paid out and the escrow remains unreleased.
        assertEq(address(attacker).balance, 0);
        assertFalse(escrow.isReleased(EPOCH, workId));
    }
}
