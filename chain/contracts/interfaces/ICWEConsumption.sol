// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWEConsumption
/// @notice Where users submit their per-epoch usage commitments.
/// @dev Keyed by `msg.sender`; one submission per user per epoch. The submitted
///      proof is forwarded to an `IProofVerifier` (accept-all in Phase 1).
interface ICWEConsumption {
    /// @notice Submit this epoch's usage commitments for the caller.
    /// @param tierId The tier the caller is subscribed to (recorded with the event).
    /// @param workCommitments The per-work usage commitments (keccak256 hashes).
    /// @param proof The usage proof bytes, forwarded to the proof verifier.
    function submitConsumption(
        bytes32 tierId,
        bytes32[] calldata workCommitments,
        bytes calldata proof
    ) external;

    /// @notice The epoch id derived from the current block timestamp.
    /// @return The current epoch number.
    function currentEpoch() external view returns (uint256);

    /// @notice Whether a user has already submitted for a given epoch.
    /// @param epoch The epoch id.
    /// @param user The user address.
    /// @return True iff a submission has been recorded.
    function hasSubmitted(uint256 epoch, address user) external view returns (bool);
}
