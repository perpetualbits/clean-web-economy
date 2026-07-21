// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWERegistry
/// @notice Registry of works and how each work's payout is split among payees.
/// @dev Splits are expressed in parts-per-million (ppm) and must sum to 1_000_000
///      for a work. `CWEPayouts` reads `payeesOf`/`splitsOf` to disburse credit.
interface ICWERegistry {
    /// @notice Register (or, for the original registrant, update) a work.
    /// @param workId The work's unique identifier.
    /// @param contentId The identifier of the underlying content (provenance).
    /// @param payees The addresses that receive the work's payouts.
    /// @param splits Each payee's share in ppm; must sum to 1_000_000.
    /// @param consentSigs Per-payee EIP-191 signatures over `consentDigest`,
    ///        proving each payee agreed to their exact share of this work.
    /// @param pricePerMin The creator's price per minute (informational on-chain).
    /// @param regionRule An opaque tag describing regional pricing rules.
    function registerWork(
        bytes32 workId,
        bytes32 contentId,
        address payable[] calldata payees,
        uint96[] calldata splits,
        bytes[] calldata consentSigs,
        uint256 pricePerMin,
        bytes32 regionRule
    ) external;

    /// @notice The digest each payee signs to consent to their share of a work.
    /// @param workId The work's unique identifier.
    /// @param contentId The identifier of the underlying content.
    /// @param payee The payee consenting to their share.
    /// @param share The payee's share in ppm.
    /// @return The digest a payee must EIP-191-sign to consent.
    function consentDigest(bytes32 workId, bytes32 contentId, address payee, uint96 share)
        external
        pure
        returns (bytes32);

    /// @notice The payee addresses for a work.
    /// @param workId The work identifier.
    /// @return The list of payees, in the order their splits are defined.
    function payeesOf(bytes32 workId) external view returns (address payable[] memory);

    /// @notice The ppm splits for a work, aligned with `payeesOf`.
    /// @param workId The work identifier.
    /// @return The list of ppm shares.
    function splitsOf(bytes32 workId) external view returns (uint96[] memory);

    /// @notice Whether a work has been registered.
    /// @param workId The work identifier.
    /// @return True iff the work exists in the registry.
    function isRegistered(bytes32 workId) external view returns (bool);

    /// @notice The address that first registered a work.
    /// @param workId The work identifier.
    /// @return The registrant address (zero if unregistered).
    function registrantOf(bytes32 workId) external view returns (address);

    /// @notice The opaque region-rule tag for a work.
    /// @param workId The work identifier.
    /// @return The regionRule tag.
    function regionRuleOf(bytes32 workId) external view returns (bytes32);

    /// @notice The content id a work was registered with.
    /// @param workId The work identifier.
    /// @return The contentId bytes32 identifier.
    function contentIdOf(bytes32 workId) external view returns (bytes32);

    /// @notice The timestamp a work was first registered at.
    /// @param workId The work identifier.
    /// @return The registration block timestamp.
    function registeredAtOf(bytes32 workId) external view returns (uint256);
}
