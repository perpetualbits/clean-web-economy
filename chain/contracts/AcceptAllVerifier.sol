// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {IProofVerifier} from "./interfaces/IProofVerifier.sol";

/// @title AcceptAllVerifier
/// @notice Phase 1 proof verifier that accepts every proof.
/// @dev Phase 1 accounts usage with hash commitments rather than ZK circuits
///      (plan decision D2), so there is nothing to cryptographically verify yet.
///      This implementation exists purely to occupy the `IProofVerifier` seam;
///      swapping it for a real circuit verifier later requires no change to
///      `CWEConsumption`. See `docs/specs/zk_usage_proof_requirements.md`.
contract AcceptAllVerifier is IProofVerifier {
    /// @inheritdoc IProofVerifier
    /// @dev Always returns true. Parameters are unnamed to document that they are
    ///      intentionally ignored in Phase 1.
    function verify(bytes32[] calldata, bytes calldata) external pure returns (bool) {
        return true;
    }
}
