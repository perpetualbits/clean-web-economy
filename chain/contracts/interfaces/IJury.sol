// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title IJury
/// @notice The asynchronous arbitration seam `CWEEscrow` consults for a disputed
///         escrow. Unlike the synchronous `IArbiter` (which decides instantly),
///         a jury opens a dispute, jurors vote over a window, and only then is a
///         verdict available — so the escrow opens a dispute on challenge and
///         reads the outcome later via `resolveDispute`.
interface IJury {
    /// @notice Open a dispute between the escrowed work and a challenger. Only the
    ///         authorised escrow may call. Returns a nonzero dispute id.
    function openDispute(bytes32 escrowedWork, bytes32 challengerWork)
        external
        returns (uint256 disputeId);

    /// @notice Whether a dispute has been finalized (a verdict is available).
    function isResolved(uint256 disputeId) external view returns (bool);

    /// @notice The finalized verdict (the winning work id). Reverts if not final.
    function verdictOf(uint256 disputeId) external view returns (bytes32 winner);
}
