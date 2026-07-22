// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {IJury} from "./interfaces/IJury.sol";
import {IArbiter} from "./interfaces/IArbiter.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWEJury
/// @notice A trusted committee that resolves fingerprint-escrow ownership
///         disputes by majority vote. It is the Phase 2.3 replacement for the
///         instant earliest-registration rule: `CWEEscrow` opens a dispute on
///         challenge, allowlisted jurors vote over a window, and the tallied
///         verdict moves the escrow.
/// @dev This is deliberately a *trusted* stub — the owner appoints jurors — that
///      fills the unavoidable human-judgment layer. A future staked, open court
///      (commit-reveal voting, slashing) can replace it behind the same `IJury`
///      seam without touching the escrow's money logic. A tie or a silent
///      committee falls back to the existing `EarliestRegistrationArbiter`, so
///      the H1 safety default is preserved.
contract CWEJury is IJury, Ownable {
    /// @notice How long a dispute stays open for voting. A 3-week floor: jurors
    ///         need time to coordinate across weekends and gather evidence, so a
    ///         shorter window would only ever force the fallback.
    uint256 public constant VOTING_WINDOW = 21 days;

    /// @notice The fallback consulted on a tie or a silent committee (earliest
    ///         registration wins), preserving the H1 default.
    IArbiter public immutable fallbackArbiter;

    /// @notice The only contract permitted to open disputes (the escrow).
    address public escrow;

    /// @notice Whether an address is an allowlisted juror.
    mapping(address => bool) public isJuror;

    /// @dev Monotonic id source; ids start at 1 so 0 always means "no dispute".
    uint256 private _nextDisputeId;

    /// @dev A single dispute's state. Holds a per-juror voted flag, so it is only
    ///      ever accessed through storage references (never copied wholesale).
    struct Dispute {
        bytes32 workA; // the escrowed (incumbent) work
        bytes32 workB; // the challenger's work
        uint256 voteEnd; // timestamp at/after which finalize is allowed
        uint256 votesA;
        uint256 votesB;
        bool finalized;
        bytes32 verdict;
        mapping(address => bool) hasVoted;
    }

    /// @dev disputeId => dispute.
    mapping(uint256 => Dispute) private _disputes;

    /// @notice Emitted when the authorised escrow is set.
    event EscrowSet(address indexed escrow);
    /// @notice Emitted when a juror is added to or removed from the allowlist.
    event JurorSet(address indexed juror, bool allowed);
    /// @notice Emitted when the escrow opens a dispute.
    event DisputeOpened(
        uint256 indexed disputeId, bytes32 indexed workA, bytes32 indexed workB, uint256 voteEnd
    );
    /// @notice Emitted on each juror vote.
    event Voted(uint256 indexed disputeId, address indexed juror, bytes32 forWork);
    /// @notice Emitted when a dispute is tallied.
    event DisputeFinalized(uint256 indexed disputeId, bytes32 verdict, uint256 votesA, uint256 votesB);

    /// @dev Reverts when the escrow address is already set (settable once).
    error EscrowAlreadySet();
    /// @dev Reverts when a non-escrow address calls `openDispute`.
    error NotEscrow();
    /// @dev Reverts when a non-juror tries to vote.
    error NotJuror();
    /// @dev Reverts when acting on a dispute id that was never opened.
    error NoDispute(uint256 disputeId);
    /// @dev Reverts when acting on an already-finalized dispute.
    error AlreadyFinalized(uint256 disputeId);
    /// @dev Reverts when voting after the window has closed.
    error VotingClosed(uint256 disputeId);
    /// @dev Reverts when finalizing before the window has closed.
    error VotingOpen(uint256 disputeId);
    /// @dev Reverts when a juror votes twice on one dispute.
    error AlreadyVoted(uint256 disputeId);
    /// @dev Reverts when voting for a work that is not a party to the dispute.
    error NotAParty(uint256 disputeId, bytes32 work);
    /// @dev Reverts when reading a verdict that is not yet finalized.
    error NotFinalized(uint256 disputeId);

    /// @param initialOwner The address that appoints jurors and the escrow.
    /// @param fallbackArbiter_ The earliest-registration fallback for ties/silence.
    constructor(address initialOwner, IArbiter fallbackArbiter_) Ownable(initialOwner) {
        fallbackArbiter = fallbackArbiter_;
    }

    /// @notice Authorise the escrow that may open disputes. Settable once, so the
    ///         escrow⇄jury deploy cycle resolves without a mutable back-door.
    function setEscrow(address escrow_) external onlyOwner {
        if (escrow != address(0)) revert EscrowAlreadySet();
        escrow = escrow_;
        emit EscrowSet(escrow_);
    }

    /// @notice Add or remove an allowlisted juror (mirrors `setVerifiedCreator`).
    function setJuror(address juror, bool allowed) external onlyOwner {
        isJuror[juror] = allowed;
        emit JurorSet(juror, allowed);
    }

    /// @inheritdoc IJury
    function openDispute(bytes32 workA, bytes32 workB) external returns (uint256 disputeId) {
        if (msg.sender != escrow) revert NotEscrow();
        // Ids start at 1 so a zero disputeId unambiguously means "none".
        disputeId = ++_nextDisputeId;
        Dispute storage d = _disputes[disputeId];
        d.workA = workA;
        d.workB = workB;
        d.voteEnd = block.timestamp + VOTING_WINDOW;
        emit DisputeOpened(disputeId, workA, workB, d.voteEnd);
    }

    /// @notice Cast a juror's single vote for one of the two disputed works.
    function vote(uint256 disputeId, bytes32 forWork) external {
        if (!isJuror[msg.sender]) revert NotJuror();
        Dispute storage d = _disputes[disputeId];
        if (d.voteEnd == 0) revert NoDispute(disputeId);
        if (d.finalized) revert AlreadyFinalized(disputeId);
        if (block.timestamp >= d.voteEnd) revert VotingClosed(disputeId);
        if (d.hasVoted[msg.sender]) revert AlreadyVoted(disputeId);
        if (forWork != d.workA && forWork != d.workB) revert NotAParty(disputeId, forWork);
        d.hasVoted[msg.sender] = true; // one vote per juror per dispute
        if (forWork == d.workA) d.votesA++;
        else d.votesB++;
        emit Voted(disputeId, msg.sender, forWork);
    }

    /// @notice Tally a dispute after its voting window; anyone may call, so a
    ///         silent committee can never freeze the escrow indefinitely.
    function finalize(uint256 disputeId) external {
        Dispute storage d = _disputes[disputeId];
        if (d.voteEnd == 0) revert NoDispute(disputeId);
        if (d.finalized) revert AlreadyFinalized(disputeId);
        if (block.timestamp < d.voteEnd) revert VotingOpen(disputeId);
        // Strict majority wins; a tie or a zero-vote dispute defers to earliest
        // registration, preserving the H1 default.
        bytes32 verdict;
        if (d.votesA > d.votesB) verdict = d.workA;
        else if (d.votesB > d.votesA) verdict = d.workB;
        else verdict = fallbackArbiter.resolve(d.workA, d.workB);
        d.verdict = verdict;
        d.finalized = true;
        emit DisputeFinalized(disputeId, verdict, d.votesA, d.votesB);
    }

    /// @inheritdoc IJury
    function isResolved(uint256 disputeId) external view returns (bool) {
        return _disputes[disputeId].finalized;
    }

    /// @inheritdoc IJury
    function verdictOf(uint256 disputeId) external view returns (bytes32) {
        Dispute storage d = _disputes[disputeId];
        if (!d.finalized) revert NotFinalized(disputeId);
        return d.verdict;
    }

    /// @notice The timestamp a dispute's voting window closes (0 if no dispute).
    function voteEndOf(uint256 disputeId) external view returns (uint256) {
        return _disputes[disputeId].voteEnd;
    }

    /// @notice The current vote tally `(votesA, votesB)` for a dispute.
    function tallyOf(uint256 disputeId) external view returns (uint256 votesA, uint256 votesB) {
        Dispute storage d = _disputes[disputeId];
        return (d.votesA, d.votesB);
    }
}
