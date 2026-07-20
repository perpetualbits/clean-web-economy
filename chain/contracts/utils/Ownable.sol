// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title Ownable
/// @notice Minimal single-owner access control.
/// @dev A deliberately small, self-contained implementation of the well-known
///      ownership pattern (equivalent to OpenZeppelin's `Ownable`) so the Phase 1
///      contracts carry no external dependency. The owner is the address allowed
///      to perform privileged administrative actions in the derived contract.
abstract contract Ownable {
    /// @notice The current owner. Public so tooling and tests can read it directly.
    address public owner;

    /// @notice Emitted whenever ownership moves from one address to another.
    /// @param previousOwner The address that was the owner before the transfer.
    /// @param newOwner The address that is the owner after the transfer.
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    /// @dev Reverts when a non-owner calls an `onlyOwner` function.
    error NotOwner();
    /// @dev Reverts when ownership would be transferred to the zero address.
    error ZeroAddressOwner();

    /// @notice Restricts a function so only the current owner may call it.
    modifier onlyOwner() {
        // Compare against the caller; any other address is rejected outright.
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    /// @param initialOwner The address that starts out owning the contract.
    /// @dev The zero address is rejected so a contract can never deploy ownerless.
    constructor(address initialOwner) {
        if (initialOwner == address(0)) revert ZeroAddressOwner();
        owner = initialOwner;
        // Announce the initial assignment as a transfer from the zero address,
        // matching the convention indexers expect.
        emit OwnershipTransferred(address(0), initialOwner);
    }

    /// @notice Transfer ownership to a new address.
    /// @param newOwner The address that will become the owner.
    /// @dev Restricted to the current owner; the zero address is rejected to avoid
    ///      accidentally bricking administrative access.
    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddressOwner();
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
    }
}
