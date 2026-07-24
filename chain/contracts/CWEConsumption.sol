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

    /// @notice Emitted on every accepted submission; carries the ZK proof's
    ///         public outputs that the off-chain aggregator (WP5) consumes.
    /// @param user The submitting user.
    /// @param epoch The epoch the submission belongs to.
    /// @param tierId The user's tier at submission time.
    /// @param digest The public-input digest the proof was verified against.
    /// @param pseudonyms The per-work pseudonymous identifiers proven by the circuit.
    /// @param workIds The per-work identifiers the usage is attributed to.
    /// @param weights The per-work usage weights proven by the circuit.
    event ConsumptionSubmitted(
        address indexed user,
        uint256 indexed epoch,
        bytes32 tierId,
        bytes32 digest,
        bytes32[] pseudonyms,
        bytes32[] workIds,
        uint256[] weights
    );

    /// @dev Reverts when a user submits twice in the same epoch.
    error AlreadySubmitted(uint256 epoch, address user);
    /// @dev Reverts when no commitments are provided.
    error NoCommitments();
    /// @dev Reverts when the proof verifier rejects the submitted proof.
    error ProofRejected();
    /// @dev Reverts when the commitments/pseudonyms/workIds/weights arrays
    ///      do not all share the same length.
    error ArityMismatch();

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
    /// @dev Enforces one submission per user per epoch, checks that the parallel
    ///      arrays agree in length, and runs the caller-supplied digest through
    ///      the verifier before recording. The proven outputs are emitted, not
    ///      stored on-chain; the off-chain aggregator reads them from the log.
    function submitConsumption(
        bytes32 tierId,
        bytes32[] calldata commitments,
        bytes32[] calldata pseudonyms,
        bytes32[] calldata workIds,
        uint256[] calldata weights,
        bytes32 digest,
        bytes calldata proof
    ) external {
        // There must be something to account for.
        if (commitments.length == 0) revert NoCommitments();
        // The four parallel arrays must describe the same set of per-work entries.
        if (
            commitments.length != pseudonyms.length || commitments.length != workIds.length
                || commitments.length != weights.length
        ) {
            revert ArityMismatch();
        }

        uint256 epoch = currentEpoch();
        // Reject a second submission from the same user in the same epoch.
        if (_submitted[epoch][msg.sender]) revert AlreadySubmitted(epoch, msg.sender);

        // Run the caller-supplied digest and proof through the verifier seam
        // (accept-all in Phase 1). The digest is the ZK circuit's public-input
        // digest binding `pseudonyms`/`workIds`/`weights`; verifying it here is
        // what checks the proof actually attests to the emitted outputs.
        if (!verifier.verify(digest, proof)) revert ProofRejected();

        // Record the submission before emitting (effects before the log).
        _submitted[epoch][msg.sender] = true;
        emit ConsumptionSubmitted(msg.sender, epoch, tierId, digest, pseudonyms, workIds, weights);
    }
}
