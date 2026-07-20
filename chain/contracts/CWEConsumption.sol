// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEConsumption} from "./interfaces/ICWEConsumption.sol";
import {IProofVerifier} from "./interfaces/IProofVerifier.sol";

/// @title CWEConsumption
/// @notice Receives each user's per-epoch usage commitments.
/// @dev Submissions are keyed by `msg.sender` and limited to one per user per
///      epoch. The epoch is derived from the block timestamp (Phase 1 has no
///      beacon; an epoch is a fixed 30-day window). The commitments themselves
///      live in the `ConsumptionSubmitted` event log, which is what the off-chain
///      settlement job (WP5) reads; on-chain we keep only a "has submitted" flag.
contract CWEConsumption is ICWEConsumption {
    /// @notice Length of one epoch in seconds (Phase 1: a 30-day window).
    uint256 public constant EPOCH_LENGTH = 30 days;

    /// @notice The proof verifier the submitted proof is checked against.
    /// @dev Phase 1 wires an accept-all verifier (the ZK seam, decision D2).
    IProofVerifier public immutable verifier;

    /// @dev epoch => user => whether they have already submitted this epoch.
    mapping(uint256 => mapping(address => bool)) private _submitted;

    /// @notice Emitted on every accepted submission; carries the commitments that
    ///         the off-chain aggregator consumes.
    /// @param user The submitting user.
    /// @param epoch The epoch the submission belongs to.
    /// @param tierId The user's tier at submission time.
    /// @param commitments The per-work usage commitments.
    event ConsumptionSubmitted(
        address indexed user, uint256 indexed epoch, bytes32 tierId, bytes32[] commitments
    );

    /// @dev Reverts when a user submits twice in the same epoch.
    error AlreadySubmitted(uint256 epoch, address user);
    /// @dev Reverts when no commitments are provided.
    error NoCommitments();
    /// @dev Reverts when the proof verifier rejects the submitted proof.
    error ProofRejected();

    /// @param verifier_ The proof verifier implementation to use.
    constructor(IProofVerifier verifier_) {
        verifier = verifier_;
    }

    /// @inheritdoc ICWEConsumption
    /// @dev The epoch is a floor-divided timestamp, so all submissions within the
    ///      same 30-day window share an epoch id.
    function currentEpoch() public view returns (uint256) {
        return block.timestamp / EPOCH_LENGTH;
    }

    /// @inheritdoc ICWEConsumption
    function hasSubmitted(uint256 epoch, address user) external view returns (bool) {
        return _submitted[epoch][user];
    }

    /// @inheritdoc ICWEConsumption
    /// @dev Enforces one submission per user per epoch and runs the proof through
    ///      the verifier before recording. The commitments are emitted, not stored.
    function submitConsumption(
        bytes32 tierId,
        bytes32[] calldata workCommitments,
        bytes calldata proof
    ) external {
        // There must be something to account for.
        if (workCommitments.length == 0) revert NoCommitments();

        uint256 epoch = currentEpoch();
        // Reject a second submission from the same user in the same epoch.
        if (_submitted[epoch][msg.sender]) revert AlreadySubmitted(epoch, msg.sender);

        // Run the proof through the verifier seam (accept-all in Phase 1).
        if (!verifier.verify(workCommitments, proof)) revert ProofRejected();

        // Record the submission before emitting (effects before the log).
        _submitted[epoch][msg.sender] = true;
        emit ConsumptionSubmitted(msg.sender, epoch, tierId, workCommitments);
    }
}
