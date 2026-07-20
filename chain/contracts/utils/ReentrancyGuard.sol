// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ReentrancyGuard
/// @notice Prevents a function from being re-entered before it completes.
/// @dev A minimal, self-contained copy of the standard status-flag guard (as in
///      OpenZeppelin's `ReentrancyGuard`). It is essential for `CWEPayouts.withdraw`,
///      which sends ETH to external payee addresses — a payee could be a contract
///      that calls back into `withdraw` before the first call finishes. The guard
///      makes any such re-entrant call revert.
abstract contract ReentrancyGuard {
    // Two sentinel values are used instead of a bool because flipping a storage
    // slot between two non-zero values is cheaper than toggling to/from zero on
    // every call, and it makes the "entered" state unambiguous.
    uint256 private constant NOT_ENTERED = 1;
    uint256 private constant ENTERED = 2;

    /// @dev The current guard state; starts NOT_ENTERED.
    uint256 private _status;

    /// @dev Reverts when a guarded function is re-entered.
    error ReentrantCall();

    /// @dev Initialise the guard to the not-entered state at construction.
    constructor() {
        _status = NOT_ENTERED;
    }

    /// @notice Marks the wrapped function as non-reentrant.
    /// @dev On entry the status must be NOT_ENTERED; it is flipped to ENTERED for
    ///      the duration of the call and restored afterwards. A nested call sees
    ///      ENTERED and reverts.
    modifier nonReentrant() {
        if (_status == ENTERED) revert ReentrantCall();
        _status = ENTERED; // lock
        _;
        _status = NOT_ENTERED; // unlock once the body has fully executed
    }
}
