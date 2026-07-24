// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title CredentialTypes
/// @notice Canonical credential-type tags, defined once so issuers, the identity
///         contract, and every gating contract agree on the exact bytes32 values.
library CredentialTypes {
    /// @notice A verified content creator, permitted to register works.
    bytes32 internal constant VERIFIED_CREATOR = keccak256("cwe.credential.verified-creator");
    /// @notice An allowlisted juror, permitted to vote in arbitration disputes.
    bytes32 internal constant JUROR = keccak256("cwe.credential.juror");
}
