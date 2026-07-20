// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWEPayouts
/// @notice The epoch payout ledger: the aggregator commits a Merkle root of
///         per-work credits, and creators withdraw against inclusion proofs.
/// @dev Matches the single-trusted-aggregator model (plan decision D5): the
///      contract does not recompute DAPR, it only verifies withdrawals against
///      the committed root.
interface ICWEPayouts {
    /// @notice Commit the payout Merkle root for an epoch.
    /// @param epochId The epoch being settled.
    /// @param merkleRoot Root of the tree of `(workId, amount)` credit leaves.
    /// @param totalCredits Sum of all leaf amounts (the epoch's distributed total).
    function commitEpoch(uint256 epochId, bytes32 merkleRoot, uint256 totalCredits) external;

    /// @notice Withdraw a work's credit for an epoch, split-paying its payees.
    /// @param epochId The epoch to withdraw from.
    /// @param workId The work whose credit is being withdrawn.
    /// @param amount The credited amount (must match the committed leaf).
    /// @param proof Merkle inclusion proof for the `(workId, amount)` leaf.
    function withdraw(
        uint256 epochId,
        bytes32 workId,
        uint256 amount,
        bytes32[] calldata proof
    ) external;

    /// @notice Whether a work's credit for an epoch has already been withdrawn.
    /// @param epochId The epoch id.
    /// @param workId The work id.
    /// @return True iff already withdrawn.
    function isWithdrawn(uint256 epochId, bytes32 workId) external view returns (bool);
}
