// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWERegistry
/// @notice Registry of works and how each work's payout is split among payees.
/// @dev Splits are expressed in parts-per-million (ppm) and must sum to 1_000_000
///      for a work. `CWEPayouts` reads `payeesOf`/`splitsOf` to disburse credit.
interface ICWERegistry {
    /// @notice Register (or, for the original registrant, update) a work.
    /// @param workId The work's unique identifier.
    /// @param payees The addresses that receive the work's payouts.
    /// @param splits Each payee's share in ppm; must sum to 1_000_000.
    /// @param pricePerMin The creator's price per minute (informational on-chain).
    /// @param regionRule An opaque tag describing regional pricing rules.
    function registerWork(
        bytes32 workId,
        address payable[] calldata payees,
        uint96[] calldata splits,
        uint256 pricePerMin,
        bytes32 regionRule
    ) external;

    /// @notice The payee addresses for a work.
    /// @param workId The work identifier.
    /// @return The list of payees, in the order their splits are defined.
    function payeesOf(bytes32 workId) external view returns (address payable[] memory);

    /// @notice The ppm splits for a work, aligned with `payeesOf`.
    /// @param workId The work identifier.
    /// @return The list of ppm shares.
    function splitsOf(bytes32 workId) external view returns (uint96[] memory);

    /// @notice Whether a work has been registered.
    /// @param workId The work identifier.
    /// @return True iff the work exists in the registry.
    function isRegistered(bytes32 workId) external view returns (bool);
}
