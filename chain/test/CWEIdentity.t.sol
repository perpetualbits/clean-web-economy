// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEIdentity} from "../contracts/CWEIdentity.sol";

contract CWEIdentityTest is Test {
    CWEIdentity internal id;
    address internal owner = makeAddr("owner");
    address internal issuer = makeAddr("issuer");
    address internal alice = makeAddr("alice");
    bytes32 internal constant T = keccak256("cwe.credential.verified-creator");

    function setUp() public {
        id = new CWEIdentity(owner);
        vm.prank(owner);
        id.setIssuer(issuer, true);
        vm.warp(1000); // a sane non-zero clock
    }

    /// @notice A trusted issuer's attestation makes a subject valid until expiry.
    function test_attest_makesValid() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        assertTrue(id.isValid(alice, T));
    }

    /// @notice Only a trusted issuer may attest.
    function test_attest_onlyIssuer() public {
        vm.expectRevert(CWEIdentity.NotIssuer.selector);
        id.attest(alice, T, uint64(block.timestamp + 100));
    }

    /// @notice Attesting a past (or present) expiry is rejected.
    function test_attest_pastExpiry_reverts() public {
        vm.prank(issuer);
        vm.expectRevert(CWEIdentity.BadExpiry.selector);
        id.attest(alice, T, uint64(block.timestamp)); // not strictly in the future
    }

    /// @notice A revoked credential is invalid.
    function test_revoke_invalidates() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        vm.prank(issuer);
        id.revoke(alice, T);
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Revoking a non-existent credential reverts.
    function test_revoke_missing_reverts() public {
        vm.prank(issuer);
        vm.expectRevert(CWEIdentity.NoCredential.selector);
        id.revoke(alice, T);
    }

    /// @notice Validity ends exactly at expiry: valid while now < expiresAt.
    function test_expiry_boundary() public {
        uint64 exp = uint64(block.timestamp + 100);
        vm.prank(issuer);
        id.attest(alice, T, exp);
        vm.warp(exp - 1);
        assertTrue(id.isValid(alice, T));
        vm.warp(exp); // now == expiresAt → invalid
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Removing the issuer invalidates all their credentials.
    function test_untrustedIssuer_invalidates() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        assertTrue(id.isValid(alice, T));
        vm.prank(owner);
        id.setIssuer(issuer, false);
        assertFalse(id.isValid(alice, T));
    }

    /// @notice A re-attest renews (new expiry, cleared revocation).
    function test_reattest_renews() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 10));
        vm.prank(issuer);
        id.revoke(alice, T);
        assertFalse(id.isValid(alice, T));
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100)); // renew
        assertTrue(id.isValid(alice, T));
    }

    /// @notice A never-attested subject is invalid.
    function test_unknown_isInvalid() public view {
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Only the owner may curate issuers.
    function test_setIssuer_onlyOwner() public {
        vm.expectRevert(); // Ownable.NotOwner
        id.setIssuer(alice, true);
    }
}
