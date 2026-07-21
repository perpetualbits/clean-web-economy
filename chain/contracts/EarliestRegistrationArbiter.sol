// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {IArbiter} from "./interfaces/IArbiter.sol";
import {ICWERegistry} from "./interfaces/ICWERegistry.sol";

/// @title EarliestRegistrationArbiter
/// @notice The Phase 1 arbitration stub: whichever work was registered earlier
///         in `CWERegistry` wins the dispute. A future Phase 2.3 jury contract
///         implements `IArbiter` with a real adjudication process and can be
///         swapped in without changing `CWEEscrow`.
/// @dev Deterministic and gameable only by registering earlier, which is exactly
///      the priority signal `CWEEscrow.challenge` intends to reward.
contract EarliestRegistrationArbiter is IArbiter {
    /// @notice The registry consulted for each work's registration timestamp.
    ICWERegistry public immutable registry;

    /// @param registry_ The work registry providing `registeredAtOf`.
    constructor(ICWERegistry registry_) {
        registry = registry_;
    }

    /// @inheritdoc IArbiter
    /// @dev Ties (including two unregistered works, both timestamp zero) resolve
    ///      to `workA`, so an existing escrow holder is never dislodged without a
    ///      strictly earlier registration.
    function resolve(bytes32 workA, bytes32 workB) external view returns (bytes32 winner) {
        uint256 timeA = registry.registeredAtOf(workA);
        uint256 timeB = registry.registeredAtOf(workB);
        return timeB < timeA ? workB : workA;
    }
}
