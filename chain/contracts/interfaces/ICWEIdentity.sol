// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWEIdentity
/// @notice The credential seam consulted by contracts that gate an action on a
///         verifiable credential (a verified creator, an allowlisted juror, ...).
///         It replaces the per-contract owner allowlists with one queryable
///         source of truth that carries expiry and revocation.
interface ICWEIdentity {
    /// @notice Whether `subject` holds a currently-valid credential of `credType`.
    /// @dev True iff the credential exists, is not revoked, is not past its
    ///      expiry, and was issued by an address that is still a trusted issuer.
    function isValid(address subject, bytes32 credType) external view returns (bool);
}
