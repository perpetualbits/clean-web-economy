// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title IArbiter
/// @notice The arbitration seam consulted when two works compete for the same
///         fingerprint-matched escrow. A dispute that registration timestamps
///         alone cannot settle is routed through an implementation of this
///         interface; the Phase 1 stub (`EarliestRegistrationArbiter`) decides
///         by earliest registration, and a Phase 2.3 jury replaces it later
///         without changing `CWEEscrow`'s challenge logic.
interface IArbiter {
    /// @notice Decide which of two competing works should hold a disputed escrow.
    /// @param workA The currently escrowed work.
    /// @param workB The challenger's work.
    /// @return winner The work id that should hold (or receive) the escrow.
    function resolve(bytes32 workA, bytes32 workB) external view returns (bytes32 winner);
}
