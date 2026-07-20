// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title IProofVerifier
/// @notice The seam where real zero-knowledge usage proofs plug in later.
/// @dev Phase 1 uses hash commitments, not circuits (plan decision D2). This
///      interface is the drop-in point: `CWEConsumption` forwards the submitted
///      proof bytes and commitments here, and Phase 1 wires an accept-all
///      implementation. When circuits arrive, only the implementation changes.
///      See `docs/specs/zk_usage_proof_requirements.md`.
interface IProofVerifier {
    /// @notice Verify a usage proof against its public commitments.
    /// @param commitments The per-work usage commitments the proof is about.
    /// @param proof The proof bytes (opaque to the caller).
    /// @return True iff the proof is considered valid.
    function verify(bytes32[] calldata commitments, bytes calldata proof)
        external
        view
        returns (bool);
}
