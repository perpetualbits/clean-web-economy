// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWETiers
/// @notice Subscription tiers and the monthly fee each one costs.
/// @dev Tiers are identified by a `bytes32` id (e.g. `keccak256("light")`). The
///      fee is what a user pays per epoch and is the amount split among creators.
interface ICWETiers {
    /// @notice The monthly fee for a tier, in wei. Zero means "no such tier".
    /// @param tierId The tier identifier.
    /// @return The fee in wei.
    function feeOf(bytes32 tierId) external view returns (uint256);

    /// @notice The tier a user is currently subscribed to (zero if none).
    /// @param user The subscriber address.
    /// @return The user's active tier id.
    function activeTier(address user) external view returns (bytes32);

    /// @notice Subscribe to a tier by paying exactly its fee.
    /// @param tierId The tier to subscribe to.
    function subscribe(bytes32 tierId) external payable;
}
