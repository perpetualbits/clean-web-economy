// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Ownable} from "./utils/Ownable.sol";

/// @title CWEEpochBeacon
/// @notice Publishes a per-epoch key `K_epoch` that other contracts/off-chain
///         provers read via `keyFor` to bind a zk usage proof to a specific
///         epoch.
/// @dev **This MVP publishes a FIXED, NON-RANDOM per-epoch key: the owner picks
///      and sets `K_epoch` directly, so it carries no unpredictability or
///      liveness guarantee.** A real randomness beacon (VRF or drand-style,
///      see `docs/specs/epoch_beacon_specification.md`) that makes `K_epoch`
///      unpredictable ahead of the epoch it governs is a deferred graduation
///      behind this same `keyFor`/`setKey` surface.
contract CWEEpochBeacon is Ownable {
    /// @dev epoch => published key. Zero means "never published".
    mapping(uint256 => bytes32) private _keys;

    /// @notice Emitted when the owner publishes (or overwrites) an epoch's key.
    /// @param epoch The epoch the key governs.
    /// @param key The published key value.
    event EpochKeySet(uint256 indexed epoch, bytes32 key);

    /// @param owner_ The address that curates published epoch keys.
    constructor(address owner_) Ownable(owner_) {}

    /// @notice The published key for `epoch`, or `bytes32(0)` if unset.
    /// @param epoch The epoch to look up.
    function keyFor(uint256 epoch) external view returns (bytes32) {
        return _keys[epoch];
    }

    /// @notice Publish (or overwrite) the key for `epoch`. Owner-only.
    /// @param epoch The epoch the key governs.
    /// @param key The key value to publish.
    function setKey(uint256 epoch, bytes32 key) external onlyOwner {
        _keys[epoch] = key;
        emit EpochKeySet(epoch, key);
    }
}
