// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEIdentity} from "./interfaces/ICWEIdentity.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWEIdentity
/// @notice A minimal verifiable-credential registry. A rotatable set of trusted
///         issuers `attest` revocable, expiring credentials to subjects; any
///         contract or caller verifies one with `isValid`. It is the Phase-6
///         replacement for per-contract owner allowlists (`setVerifiedCreator`,
///         `setJuror`): the same gate, now portable, expiring, revocable, and
///         verifiable by anyone.
/// @dev Deliberately a *trusted-issuer* stub — the owner curates issuers. Real
///      eID/SSI (proof-of-personhood, OIDC, W3C DID/JSON-LD, holder-carried VCs)
///      graduate this behind the `ICWEIdentity` seam without touching the
///      gating contracts. Removing an issuer invalidates every credential they
///      granted, so a compromised issuer is contained by one `setIssuer` call.
contract CWEIdentity is ICWEIdentity, Ownable {
    /// @notice The rotatable set of addresses permitted to attest/revoke.
    mapping(address => bool) public isIssuer;

    /// @dev A single credential record.
    struct Credential {
        address issuer;   // who attested it (checked still-trusted in isValid)
        uint64 issuedAt;  // attestation time
        uint64 expiresAt; // valid while block.timestamp < expiresAt
        bool revoked;     // issuer-set revocation flag
        bool exists;      // distinguishes "never attested" from a zeroed record
    }

    /// @dev keccak256(subject, credType) => credential.
    mapping(bytes32 => Credential) private _credentials;

    /// @notice Emitted when the owner adds or removes a trusted issuer.
    event IssuerSet(address indexed issuer, bool trusted);
    /// @notice Emitted when an issuer attests a credential.
    event Attested(address indexed subject, bytes32 indexed credType, address indexed issuer, uint64 expiresAt);
    /// @notice Emitted when an issuer revokes a credential.
    event Revoked(address indexed subject, bytes32 indexed credType, address indexed issuer);

    /// @dev Reverts when a non-issuer calls `attest`/`revoke`.
    error NotIssuer();
    /// @dev Reverts when `attest` is given an expiry that is not in the future.
    error BadExpiry();
    /// @dev Reverts when `revoke` targets a credential that was never attested.
    error NoCredential();

    /// @dev Restricts a function to a currently-trusted issuer.
    modifier onlyIssuer() {
        if (!isIssuer[msg.sender]) revert NotIssuer();
        _;
    }

    /// @param initialOwner The address that curates the issuer set.
    constructor(address initialOwner) Ownable(initialOwner) {}

    /// @notice Add or remove a trusted issuer.
    function setIssuer(address issuer, bool trusted) external onlyOwner {
        isIssuer[issuer] = trusted;
        emit IssuerSet(issuer, trusted);
    }

    /// @notice Attest a credential of `credType` to `subject`, valid until
    ///         `expiresAt`. Re-attesting overwrites (a renewal). Issuer-only.
    function attest(address subject, bytes32 credType, uint64 expiresAt) external onlyIssuer {
        if (expiresAt <= block.timestamp) revert BadExpiry();
        _credentials[_key(subject, credType)] = Credential({
            issuer: msg.sender,
            issuedAt: uint64(block.timestamp),
            expiresAt: expiresAt,
            revoked: false,
            exists: true
        });
        emit Attested(subject, credType, msg.sender, expiresAt);
    }

    /// @notice Revoke a subject's credential of `credType`. Issuer-only.
    function revoke(address subject, bytes32 credType) external onlyIssuer {
        Credential storage c = _credentials[_key(subject, credType)];
        if (!c.exists) revert NoCredential();
        c.revoked = true;
        emit Revoked(subject, credType, msg.sender);
    }

    /// @inheritdoc ICWEIdentity
    function isValid(address subject, bytes32 credType) external view returns (bool) {
        Credential storage c = _credentials[_key(subject, credType)];
        // All four must hold: attested, live, unexpired, and from a still-trusted issuer.
        return c.exists && !c.revoked && block.timestamp < c.expiresAt && isIssuer[c.issuer];
    }

    /// @notice The raw credential record for `(subject, credType)` (for tooling/tests).
    function credentialOf(address subject, bytes32 credType)
        external
        view
        returns (address issuer, uint64 issuedAt, uint64 expiresAt, bool revoked, bool exists)
    {
        Credential storage c = _credentials[_key(subject, credType)];
        return (c.issuer, c.issuedAt, c.expiresAt, c.revoked, c.exists);
    }

    /// @dev The storage key binding a subject to a credential type.
    function _key(address subject, bytes32 credType) private pure returns (bytes32) {
        return keccak256(abi.encode(subject, credType));
    }
}
