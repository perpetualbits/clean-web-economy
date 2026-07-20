// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title MerkleProof
/// @notice Verifies Merkle inclusion proofs over keccak256 with sorted pairs.
/// @dev The off-chain settlement job (WP5, Rust) builds the epoch credit tree and
///      MUST use the identical construction for proofs to verify here:
///
///        * leaf   = keccak256(abi.encodePacked(workId, amount))   (see CWEPayouts)
///        * parent = keccak256(concat(sorted(left, right)))
///
///      Sorting each pair before hashing means a proof is just a list of sibling
///      hashes with no left/right flags — the verifier orders each pair itself.
library MerkleProof {
    /// @notice Recompute the root implied by `leaf` and `proof` and compare to `root`.
    /// @param proof The sibling hashes from the leaf up to the root.
    /// @param root The expected Merkle root committed on-chain for the epoch.
    /// @param leaf The leaf being proven to be part of the tree.
    /// @return True iff `leaf` combined with `proof` reproduces `root`.
    function verify(bytes32[] memory proof, bytes32 root, bytes32 leaf)
        internal
        pure
        returns (bool)
    {
        // Walk from the leaf up, folding in one sibling per level.
        bytes32 computed = leaf;
        for (uint256 i = 0; i < proof.length; i++) {
            computed = _hashPair(computed, proof[i]);
        }
        // The proof is valid exactly when it reconstructs the committed root.
        return computed == root;
    }

    /// @dev Hash two nodes after ordering them, so the parent is independent of
    ///      which child was "left". Ordering is by numeric value of the hashes.
    function _hashPair(bytes32 a, bytes32 b) private pure returns (bytes32) {
        return a < b ? _efficientHash(a, b) : _efficientHash(b, a);
    }

    /// @dev keccak256 of two tightly-packed 32-byte words, done in assembly to
    ///      avoid the memory allocation of `abi.encodePacked`. Scratch space
    ///      (memory 0x00–0x40) is safe to clobber for this transient computation.
    function _efficientHash(bytes32 a, bytes32 b) private pure returns (bytes32 value) {
        assembly {
            mstore(0x00, a) // first word
            mstore(0x20, b) // second word
            value := keccak256(0x00, 0x40) // hash the 64 packed bytes
        }
    }
}
