// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEJury} from "../contracts/CWEJury.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {EarliestRegistrationArbiter} from "../contracts/EarliestRegistrationArbiter.sol";
import {CWEIdentity} from "../contracts/CWEIdentity.sol";
import {CredentialTypes} from "../contracts/CredentialTypes.sol";

/// @title CWEJuryTest
/// @notice Unit tests for dispute opening, juror voting, majority/tie/fallback
///         resolution, and admin controls.
contract CWEJuryTest is Test {
    CWEJury internal jury;
    CWERegistry internal registry;
    EarliestRegistrationArbiter internal arbiter;
    CWEIdentity internal identity;

    address internal owner = makeAddr("owner");
    address internal creator = makeAddr("creator");
    address internal juror1 = makeAddr("juror1");
    address internal juror2 = makeAddr("juror2");
    address internal juror3 = makeAddr("juror3");
    address internal escrow = makeAddr("escrow"); // stand-in escrow caller

    // Two competing works over the same content; workEarly registers first.
    bytes32 internal workEarly = keccak256("work-early");
    bytes32 internal workLate = keccak256("work-late");
    bytes32 internal constant CONTENT = keccak256("content");

    address payable internal payeeE;
    address payable internal payeeL;
    uint256 internal payeeEKey;
    uint256 internal payeeLKey;

    /// @notice Deploy the identity registry, registry, arbiter, and jury; attest
    ///         `creator`'s verified-creator credential and each juror's juror
    ///         credential; register the two competing works.
    function setUp() public {
        vm.startPrank(owner);
        identity = new CWEIdentity(owner);
        identity.setIssuer(owner, true);
        registry = new CWERegistry(owner, identity);
        identity.attest(creator, CredentialTypes.VERIFIED_CREATOR, type(uint64).max);
        vm.stopPrank();
        arbiter = new EarliestRegistrationArbiter(registry);
        jury = new CWEJury(owner, arbiter, identity);
        vm.prank(owner);
        jury.setEscrow(escrow);

        (address e, uint256 ek) = makeAddrAndKey("payeeE");
        (address l, uint256 lk) = makeAddrAndKey("payeeL");
        payeeE = payable(e); payeeEKey = ek; payeeL = payable(l); payeeLKey = lk;

        // workEarly registered first (t=1000), workLate later (t=2000).
        vm.warp(1000); _register(workEarly, payeeE, payeeEKey);
        vm.warp(2000); _register(workLate, payeeL, payeeLKey);

        vm.prank(owner); identity.attest(juror1, CredentialTypes.JUROR, type(uint64).max);
        vm.prank(owner); identity.attest(juror2, CredentialTypes.JUROR, type(uint64).max);
        vm.prank(owner); identity.attest(juror3, CredentialTypes.JUROR, type(uint64).max);
    }

    /// @dev Register `workId` with a single 100% consenting payee over CONTENT.
    function _register(bytes32 workId, address payable payee, uint256 key) internal {
        address payable[] memory payees = new address payable[](1);
        payees[0] = payee;
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        bytes[] memory sigs = new bytes[](1);
        bytes32 digest = registry.consentDigest(workId, CONTENT, payee, splits[0]);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, eth);
        sigs[0] = abi.encodePacked(r, s, v);
        vm.prank(creator);
        registry.registerWork(workId, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev Open a dispute as the authorised escrow.
    function _open() internal returns (uint256) {
        vm.prank(escrow);
        return jury.openDispute(workEarly, workLate);
    }

    /// @notice Only the authorised escrow may open a dispute.
    function test_openDispute_onlyEscrow() public {
        vm.expectRevert(CWEJury.NotEscrow.selector);
        jury.openDispute(workEarly, workLate);
    }

    /// @notice A majority for the later work overrides the earliest-registration
    ///         default (which would have picked workEarly).
    function test_finalize_majorityOverridesFallback() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.prank(juror2); jury.vote(id, workLate);
        vm.prank(juror3); jury.vote(id, workEarly);
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertTrue(jury.isResolved(id));
        assertEq(jury.verdictOf(id), workLate); // committee overrode the timestamp
        // Sanity: the fallback alone would have chosen workEarly.
        assertEq(arbiter.resolve(workEarly, workLate), workEarly);
    }

    /// @notice A zero-vote dispute falls back to earliest registration.
    function test_finalize_noVotes_fallsBackToEarliest() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertEq(jury.verdictOf(id), workEarly); // earliest registration default
    }

    /// @notice A tie falls back to earliest registration.
    function test_finalize_tie_fallsBackToEarliest() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.prank(juror2); jury.vote(id, workEarly);
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertEq(jury.verdictOf(id), workEarly);
    }

    /// @notice A non-juror cannot vote.
    function test_vote_onlyJuror() public {
        uint256 id = _open();
        vm.expectRevert(CWEJury.NotJuror.selector);
        vm.prank(makeAddr("stranger")); jury.vote(id, workLate);
    }

    /// @notice A juror cannot vote twice on the same dispute.
    function test_vote_twice_reverts() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.expectRevert(abi.encodeWithSelector(CWEJury.AlreadyVoted.selector, id));
        vm.prank(juror1); jury.vote(id, workEarly);
    }

    /// @notice A vote for a work that is not a party reverts.
    function test_vote_nonParty_reverts() public {
        uint256 id = _open();
        bytes32 other = keccak256("other");
        vm.expectRevert(abi.encodeWithSelector(CWEJury.NotAParty.selector, id, other));
        vm.prank(juror1); jury.vote(id, other);
    }

    /// @notice Voting after the window closes reverts.
    function test_vote_afterWindow_reverts() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        vm.expectRevert(abi.encodeWithSelector(CWEJury.VotingClosed.selector, id));
        vm.prank(juror1); jury.vote(id, workLate);
    }

    /// @notice Finalizing before the window closes reverts.
    function test_finalize_early_reverts() public {
        uint256 id = _open();
        vm.expectRevert(abi.encodeWithSelector(CWEJury.VotingOpen.selector, id));
        jury.finalize(id);
    }

    /// @notice A dispute cannot be finalized twice.
    function test_finalize_twice_reverts() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        vm.expectRevert(abi.encodeWithSelector(CWEJury.AlreadyFinalized.selector, id));
        jury.finalize(id);
    }

    /// @notice verdictOf reverts before finalize.
    function test_verdictOf_beforeFinalize_reverts() public {
        uint256 id = _open();
        vm.expectRevert(abi.encodeWithSelector(CWEJury.NotFinalized.selector, id));
        jury.verdictOf(id);
    }

    /// @notice Only the owner may set the escrow.
    function test_admin_onlyOwner() public {
        vm.expectRevert(); jury.setEscrow(escrow);
    }

    /// @notice The escrow can be set only once.
    function test_setEscrow_once() public {
        vm.expectRevert(CWEJury.EscrowAlreadySet.selector);
        vm.prank(owner); jury.setEscrow(makeAddr("other"));
    }
}
