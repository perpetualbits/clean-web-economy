// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWETiers} from "./interfaces/ICWETiers.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWETiers
/// @notice Holds the subscription tier table and takes users' monthly payments.
/// @dev A tier is a `bytes32` id mapped to a fee in wei. When a user subscribes,
///      the fee is forwarded to the payout pool (`CWEPayouts`) so the money that
///      backs creator withdrawals is collected automatically. Fees are owner-settable.
contract CWETiers is ICWETiers, Ownable {
    /// @dev tierId => monthly fee in wei. A fee of zero marks a non-existent tier.
    mapping(bytes32 => uint256) private _fees;

    /// @inheritdoc ICWETiers
    /// @dev user => the tier id they last subscribed to (zero if never).
    mapping(address => bytes32) public activeTier;

    /// @notice The pool that receives subscription fees (the `CWEPayouts` address).
    address payable public payoutPool;

    /// @notice Emitted when the owner sets or changes a tier's fee.
    event FeeSet(bytes32 indexed tierId, uint256 fee);
    /// @notice Emitted when the owner sets the payout pool address.
    event PayoutPoolSet(address indexed pool);
    /// @notice Emitted when a user subscribes to a tier.
    event Subscribed(address indexed user, bytes32 indexed tierId, uint256 fee);

    /// @dev Reverts when subscribing to a tier that has no fee configured.
    error UnknownTier(bytes32 tierId);
    /// @dev Reverts when the paid value does not exactly equal the tier fee.
    error WrongFee(uint256 sent, uint256 required);
    /// @dev Reverts when subscribing before the payout pool has been configured.
    error PayoutPoolUnset();
    /// @dev Reverts when forwarding the fee to the payout pool fails.
    error FeeForwardFailed();

    /// @param initialOwner The address allowed to set fees and the payout pool.
    constructor(address initialOwner) Ownable(initialOwner) {}

    /// @notice Set (or change) the fee for a tier.
    /// @param tierId The tier identifier.
    /// @param fee The new fee in wei.
    function setFee(bytes32 tierId, uint256 fee) external onlyOwner {
        _fees[tierId] = fee;
        emit FeeSet(tierId, fee);
    }

    /// @notice Point subscription revenue at the payout pool.
    /// @param pool The `CWEPayouts` contract address.
    function setPayoutPool(address payable pool) external onlyOwner {
        payoutPool = pool;
        emit PayoutPoolSet(pool);
    }

    /// @inheritdoc ICWETiers
    function feeOf(bytes32 tierId) external view returns (uint256) {
        return _fees[tierId];
    }

    /// @inheritdoc ICWETiers
    /// @dev Requires an exact-fee payment and a configured pool. The fee is
    ///      forwarded to the pool immediately so payout funding tracks revenue.
    function subscribe(bytes32 tierId) external payable {
        uint256 fee = _fees[tierId];
        // A zero fee means the tier does not exist; reject rather than accept free subs.
        if (fee == 0) revert UnknownTier(tierId);
        // Require exact payment so the pool is funded by precisely one tier fee.
        if (msg.value != fee) revert WrongFee(msg.value, fee);
        // The pool must be set, otherwise the fee would be stuck in this contract.
        if (payoutPool == address(0)) revert PayoutPoolUnset();

        // Record the subscription before moving funds (checks-effects-interactions).
        activeTier[msg.sender] = tierId;
        emit Subscribed(msg.sender, tierId, fee);

        // Forward the fee to the payout pool; bubble up a clear error on failure.
        (bool ok,) = payoutPool.call{value: fee}("");
        if (!ok) revert FeeForwardFailed();
    }
}
