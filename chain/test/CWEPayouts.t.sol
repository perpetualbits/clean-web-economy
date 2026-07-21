// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEPayouts} from "../contracts/CWEPayouts.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";

/// @notice A payee that tries to re-enter `withdraw` when it receives ETH, used
///         to prove the reentrancy guard blocks nested withdrawals.
contract ReentrantPayee {
    CWEPayouts private immutable payouts;
    uint256 private epoch;
    bytes32 private workId;
    uint256 private amount;
    bytes32[] private proof;
    bool private armed;

    constructor(CWEPayouts payouts_) {
        payouts = payouts_;
    }

    /// @dev Store the parameters of a re-entrant withdrawal and arm the attack.
    function arm(uint256 epoch_, bytes32 workId_, uint256 amount_, bytes32[] calldata proof_)
        external
    {
        epoch = epoch_;
        workId = workId_;
        amount = amount_;
        proof = proof_;
        armed = true;
    }

    /// @dev On receiving ETH, attempt exactly one re-entrant withdraw. The guard
    ///      makes this call revert, which propagates back as a failed transfer.
    receive() external payable {
        if (armed) {
            armed = false; // only try once
            payouts.withdraw(epoch, workId, amount, proof);
        }
    }
}

/// @title CWEPayoutsTest
/// @notice Unit tests for epoch commitment, Merkle-proven withdrawal, split-pay,
///         double-withdraw prevention, solvency, and reentrancy safety.
contract CWEPayoutsTest is Test {
    CWEPayouts internal payouts;
    CWERegistry internal registry;
    address internal owner = makeAddr("owner");
    address internal creator = makeAddr("creator");
    address internal aggregator = makeAddr("aggregator");

    address payable internal payee1;
    address payable internal payee2;
    uint256 internal payee1Key;
    uint256 internal payee2Key;

    bytes32 internal constant WORK_A = keccak256("work-A");
    bytes32 internal constant WORK_B = keccak256("work-B");
    bytes32 internal constant CONTENT_A = keccak256("content-A");
    bytes32 internal constant CONTENT_B = keccak256("content-B");
    uint256 internal constant EPOCH = 7;

    /// @notice Deploy registry + payouts, register WORK_A with a 60/40 split.
    function setUp() public {
        vm.startPrank(owner);
        registry = new CWERegistry(owner);
        registry.setVerifiedCreator(creator, true);
        vm.stopPrank();

        payouts = new CWEPayouts(registry, aggregator);

        (address p1, uint256 k1) = makeAddrAndKey("payee1");
        (address p2, uint256 k2) = makeAddrAndKey("payee2");
        payee1 = payable(p1);
        payee2 = payable(p2);
        payee1Key = k1;
        payee2Key = k2;

        address payable[] memory payees = new address payable[](2);
        payees[0] = payee1;
        payees[1] = payee2;
        uint96[] memory splits = new uint96[](2);
        splits[0] = 600_000; // 60%
        splits[1] = 400_000; // 40%
        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(payee1Key, WORK_A, CONTENT_A, payee1, splits[0]);
        sigs[1] = _consent(payee2Key, WORK_A, CONTENT_A, payee2, splits[1]);
        vm.prank(creator);
        registry.registerWork(WORK_A, CONTENT_A, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev EIP-191 personal-sign of the consent digest by key `k`, mirroring
    ///      `CWERegistryTest._consent` so both suites build valid consents identically.
    function _consent(uint256 k, bytes32 w, bytes32 c, address payee, uint96 share)
        internal view returns (bytes memory)
    {
        bytes32 digest = registry.consentDigest(w, c, payee, share);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(k, eth);
        return abi.encodePacked(r, s, v);
    }

    /// @dev Reproduce the contract's sorted-pair parent hash for building proofs.
    function _hashPair(bytes32 a, bytes32 b) internal pure returns (bytes32) {
        return a < b ? keccak256(abi.encodePacked(a, b)) : keccak256(abi.encodePacked(b, a));
    }

    /// @notice A committed credit can be withdrawn and is split 60/40, in full.
    function test_withdraw_splitsPayoutInFull() public {
        uint256 amountA = 1 ether;
        uint256 amountB = 0.5 ether;

        // Two-leaf tree: WORK_A and WORK_B. Proof for A is just B's leaf.
        bytes32 leafA = payouts.leafHash(WORK_A, amountA);
        bytes32 leafB = payouts.leafHash(WORK_B, amountB);
        bytes32 root = _hashPair(leafA, leafB);

        // Fund the pool and commit the epoch as the aggregator.
        vm.deal(address(payouts), amountA + amountB);
        vm.prank(aggregator);
        payouts.commitEpoch(EPOCH, root, amountA + amountB);

        bytes32[] memory proof = new bytes32[](1);
        proof[0] = leafB;
        payouts.withdraw(EPOCH, WORK_A, amountA, proof);

        // 60/40 split of 1 ether, fully dispersed.
        assertEq(payee1.balance, 0.6 ether);
        assertEq(payee2.balance, 0.4 ether);
        assertEq(payee1.balance + payee2.balance, amountA);
        assertTrue(payouts.isWithdrawn(EPOCH, WORK_A));
    }

    /// @notice The same credit cannot be withdrawn twice.
    function test_withdraw_doubleWithdraw_reverts() public {
        uint256 amountA = 1 ether;
        bytes32 leafA = payouts.leafHash(WORK_A, amountA);
        // Single-leaf tree: root == leaf, empty proof.
        vm.deal(address(payouts), amountA);
        vm.prank(aggregator);
        payouts.commitEpoch(EPOCH, leafA, amountA);

        bytes32[] memory empty = new bytes32[](0);
        payouts.withdraw(EPOCH, WORK_A, amountA, empty);

        vm.expectRevert(
            abi.encodeWithSelector(CWEPayouts.AlreadyWithdrawn.selector, EPOCH, WORK_A)
        );
        payouts.withdraw(EPOCH, WORK_A, amountA, empty);
    }

    /// @notice A proof that does not match the committed root is rejected.
    function test_withdraw_badProof_reverts() public {
        uint256 amountA = 1 ether;
        bytes32 leafA = payouts.leafHash(WORK_A, amountA);
        vm.deal(address(payouts), amountA);
        vm.prank(aggregator);
        payouts.commitEpoch(EPOCH, leafA, amountA);

        // Wrong amount => wrong leaf => proof fails against the committed root.
        bytes32[] memory empty = new bytes32[](0);
        vm.expectRevert(CWEPayouts.BadProof.selector);
        payouts.withdraw(EPOCH, WORK_A, amountA + 1, empty);
    }

    /// @notice Only the aggregator may commit an epoch.
    function test_commit_onlyAggregator() public {
        vm.expectRevert(CWEPayouts.NotAggregator.selector);
        payouts.commitEpoch(EPOCH, bytes32(0), 0);
    }

    /// @notice An epoch cannot be committed twice.
    function test_commit_twice_reverts() public {
        vm.deal(address(payouts), 1 ether);
        vm.startPrank(aggregator);
        payouts.commitEpoch(EPOCH, bytes32(0), 1 ether);
        vm.expectRevert(abi.encodeWithSelector(CWEPayouts.EpochAlreadyCommitted.selector, EPOCH));
        payouts.commitEpoch(EPOCH, bytes32(0), 0);
        vm.stopPrank();
    }

    /// @notice Committing more credit than the pool holds is rejected.
    function test_commit_insolvent_reverts() public {
        vm.deal(address(payouts), 1 ether);
        vm.prank(aggregator);
        vm.expectRevert(
            abi.encodeWithSelector(CWEPayouts.Insolvent.selector, 1 ether, 2 ether)
        );
        payouts.commitEpoch(EPOCH, bytes32(0), 2 ether);
    }

    /// @notice A payee that re-enters `withdraw` is blocked by the guard, causing
    ///         the whole withdrawal to revert (no funds move, nothing is marked).
    function test_withdraw_reentrancy_blocked() public {
        // Register WORK_B with a single payee that attacks on receive. A
        // contract address has no private key, so no signature can ever
        // ecrecover to it; place the attacker's bytecode (via vm.etch) at an
        // address whose key we do know, so a genuine consent signature
        // validates while the reentrancy trigger still fires on that address.
        (address attackerAddr, uint256 attackerKey) = makeAddrAndKey("attacker");
        vm.etch(attackerAddr, address(new ReentrantPayee(payouts)).code);
        ReentrantPayee attacker = ReentrantPayee(payable(attackerAddr));
        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(attackerAddr);
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        bytes[] memory sigs = new bytes[](1);
        sigs[0] = _consent(attackerKey, WORK_B, CONTENT_B, attackerAddr, splits[0]);
        vm.prank(creator);
        registry.registerWork(WORK_B, CONTENT_B, payees, splits, sigs, 1000, bytes32("EU"));

        uint256 amountB = 1 ether;
        bytes32 leafB = payouts.leafHash(WORK_B, amountB);
        vm.deal(address(payouts), amountB);
        vm.prank(aggregator);
        payouts.commitEpoch(EPOCH, leafB, amountB);

        bytes32[] memory empty = new bytes32[](0);
        attacker.arm(EPOCH, WORK_B, amountB, empty);

        // The re-entrant call reverts inside the guard, surfacing as a failed
        // transfer to the attacker.
        vm.expectRevert(abi.encodeWithSelector(CWEPayouts.PayoutFailed.selector, address(attacker)));
        payouts.withdraw(EPOCH, WORK_B, amountB, empty);

        // Nothing was paid out and the credit remains unwithdrawn.
        assertEq(address(attacker).balance, 0);
        assertFalse(payouts.isWithdrawn(EPOCH, WORK_B));
    }
}
