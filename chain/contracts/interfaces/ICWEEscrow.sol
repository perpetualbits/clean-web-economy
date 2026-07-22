// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWEEscrow
/// @notice Holds fingerprint-matched credit behind a challenge window before it
///         is split-paid to a work's registered payees.
/// @dev Signed (Tier 1) credits pay directly through `CWEPayouts` and never touch
///      this contract; only Tier 2 fingerprint-matched credit is escrowed here,
///      because a fingerprint match is a cautious fallback rather than proof of
///      authorship (see the design doc §5).
interface ICWEEscrow {
    /// @notice Escrow `amount` of fingerprint-matched credit for `workId` in
    ///         `epochId`, opening a challenge window.
    /// @param epochId The settlement epoch the credit was matched in.
    /// @param workId The work the aggregator matched the credit to.
    /// @param amount The credited amount to escrow.
    function commit(uint256 epochId, bytes32 workId, uint256 amount) external;

    /// @notice Challenge an escrowed credit with a competing work, opening a jury
    ///         dispute (the verdict is applied later via `resolveDispute`).
    /// @param epochId The epoch the escrow was committed under.
    /// @param escrowedWork The work currently holding the escrow.
    /// @param challengerWork The competing work claiming the credit instead.
    function challenge(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork) external;

    /// @notice Apply a finalized jury verdict to a disputed escrow — reassigning to
    ///         the challenger if it won, or clearing the dispute if the incumbent did.
    /// @param epochId The epoch the escrow was committed under.
    /// @param escrowedWork The work the dispute was opened against.
    function resolveDispute(uint256 epochId, bytes32 escrowedWork) external;

    /// @notice Release an escrow past its challenge window, split-paying its payees.
    /// @param epochId The epoch the escrow was committed under.
    /// @param workId The work (possibly reassigned by a challenge) to release.
    function release(uint256 epochId, bytes32 workId) external;

    /// @notice The escrowed amount for a `(epoch, work)` pair.
    /// @param epochId The epoch id.
    /// @param workId The work id.
    /// @return The escrowed amount (zero if none, or if reassigned away).
    function escrowOf(uint256 epochId, bytes32 workId) external view returns (uint256);

    /// @notice The epoch at or after which a `(epoch, work)` escrow may release.
    /// @param epochId The epoch id.
    /// @param workId The work id.
    /// @return The release epoch.
    function releaseEpochOf(uint256 epochId, bytes32 workId) external view returns (uint256);

    /// @notice Whether a `(epoch, work)` escrow has already been released.
    /// @param epochId The epoch id.
    /// @param workId The work id.
    /// @return True iff already released.
    function isReleased(uint256 epochId, bytes32 workId) external view returns (bool);
}
