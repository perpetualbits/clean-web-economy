// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEPayouts} from "./interfaces/ICWEPayouts.sol";
import {ICWERegistry} from "./interfaces/ICWERegistry.sol";
import {MerkleProof} from "./utils/MerkleProof.sol";
import {ReentrancyGuard} from "./utils/ReentrancyGuard.sol";

/// @title CWEPayouts
/// @notice The epoch payout ledger and pool.
/// @dev Money flow: subscription fees arrive here (from `CWETiers`) and pool up.
///      Each epoch, the trusted off-chain aggregator computes per-work credits,
///      builds a Merkle tree of `(workId, amount)` leaves, and commits its root.
///      Creators then `withdraw`, proving their leaf against the committed root;
///      the credit is split among the work's payees per the registry.
///
///      Safety properties (see the tests):
///        * no double-withdraw — each `(epoch, work)` can be withdrawn once;
///        * full dispersal — a withdrawal pays out its entire credited amount;
///        * solvency — an epoch cannot commit more credit than the pool holds;
///        * reentrancy-safe — `withdraw` is guarded and follows checks-effects-interactions.
contract CWEPayouts is ICWEPayouts, ReentrancyGuard {
    /// @notice ppm denominator for splits (matches `CWERegistry.PPM_TOTAL`).
    uint96 private constant PPM_TOTAL = 1_000_000;

    /// @notice The registry consulted for each work's payees and splits.
    ICWERegistry public immutable registry;

    /// @notice The only address allowed to commit epoch roots (the settlement job).
    address public immutable aggregator;

    /// @notice Committed Merkle root per epoch.
    mapping(uint256 => bytes32) public epochRoot;
    /// @notice Committed total credit per epoch (sum of all leaf amounts).
    mapping(uint256 => uint256) public epochTotal;
    /// @notice Whether an epoch has been committed (roots may be zero legitimately).
    mapping(uint256 => bool) public epochCommitted;

    /// @dev epoch => work => whether its credit has been withdrawn.
    mapping(uint256 => mapping(bytes32 => bool)) private _withdrawn;

    /// @notice Total committed-but-not-yet-withdrawn credit across all epochs.
    /// @dev The pool balance must always cover this; enforced at commit time.
    uint256 public liability;

    /// @notice Emitted when the aggregator commits an epoch's payout root.
    event EpochCommitted(uint256 indexed epochId, bytes32 merkleRoot, uint256 totalCredits);
    /// @notice Emitted when a work's credit is withdrawn and split-paid.
    event Withdrawn(uint256 indexed epochId, bytes32 indexed workId, uint256 amount);

    /// @dev Reverts when a non-aggregator calls `commitEpoch`.
    error NotAggregator();
    /// @dev Reverts when committing an epoch that is already committed.
    error EpochAlreadyCommitted(uint256 epochId);
    /// @dev Reverts when the pool cannot cover the committed liability.
    error Insolvent(uint256 balance, uint256 liability);
    /// @dev Reverts when withdrawing from an epoch that was never committed.
    error EpochNotCommitted(uint256 epochId);
    /// @dev Reverts when a work's credit for an epoch was already withdrawn.
    error AlreadyWithdrawn(uint256 epochId, bytes32 workId);
    /// @dev Reverts when the Merkle proof does not match the committed root.
    error BadProof();
    /// @dev Reverts when the work being withdrawn is not in the registry.
    error WorkNotRegistered(bytes32 workId);
    /// @dev Reverts when a payee transfer fails.
    error PayoutFailed(address payee);

    /// @param registry_ The work registry (payees and splits source).
    /// @param aggregator_ The address permitted to commit epoch roots.
    constructor(ICWERegistry registry_, address aggregator_) {
        registry = registry_;
        aggregator = aggregator_;
    }

    /// @notice Accept pool funding (subscription fees forwarded by `CWETiers`, or
    ///         direct top-ups). Funds are undifferentiated pool liquidity.
    receive() external payable {}

    /// @inheritdoc ICWEPayouts
    /// @dev Only the aggregator may commit, and only once per epoch. After adding
    ///      the new total to the outstanding liability, the pool must still cover
    ///      it — otherwise the epoch is committing credit that cannot be paid.
    function commitEpoch(uint256 epochId, bytes32 merkleRoot, uint256 totalCredits) external {
        if (msg.sender != aggregator) revert NotAggregator();
        if (epochCommitted[epochId]) revert EpochAlreadyCommitted(epochId);

        // Record the epoch's root and total, and mark it committed.
        epochRoot[epochId] = merkleRoot;
        epochTotal[epochId] = totalCredits;
        epochCommitted[epochId] = true;

        // Grow the outstanding liability and assert the pool can honour it.
        liability += totalCredits;
        if (address(this).balance < liability) {
            revert Insolvent(address(this).balance, liability);
        }

        emit EpochCommitted(epochId, merkleRoot, totalCredits);
    }

    /// @inheritdoc ICWEPayouts
    function isWithdrawn(uint256 epochId, bytes32 workId) external view returns (bool) {
        return _withdrawn[epochId][workId];
    }

    /// @notice The leaf hash for a `(workId, amount)` credit.
    /// @dev Exposed so the off-chain job and tests can reproduce the exact leaf
    ///      encoding: keccak256 of the tightly-packed 32-byte work id and 32-byte
    ///      amount. WP5 builds the tree from these leaves.
    /// @param workId The work id.
    /// @param amount The credited amount.
    /// @return The leaf hash.
    function leafHash(bytes32 workId, uint256 amount) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(workId, amount));
    }

    /// @inheritdoc ICWEPayouts
    /// @dev Verifies the leaf against the committed root, marks the work withdrawn
    ///      (effects) before paying out (interactions), and is `nonReentrant`. The
    ///      credit is split among payees per the registry, with any rounding dust
    ///      folded into the final payee so the whole `amount` is always dispersed.
    function withdraw(
        uint256 epochId,
        bytes32 workId,
        uint256 amount,
        bytes32[] calldata proof
    ) external nonReentrant {
        if (!epochCommitted[epochId]) revert EpochNotCommitted(epochId);
        if (_withdrawn[epochId][workId]) revert AlreadyWithdrawn(epochId, workId);

        // The (workId, amount) leaf must be provable against the epoch's root.
        bytes32 leaf = leafHash(workId, amount);
        if (!MerkleProof.verify(proof, epochRoot[epochId], leaf)) revert BadProof();

        // Load payees/splits; a work must be registered to receive a payout.
        address payable[] memory payees = registry.payeesOf(workId);
        uint96[] memory splits = registry.splitsOf(workId);
        if (payees.length == 0) revert WorkNotRegistered(workId);

        // Effects: mark withdrawn and reduce liability BEFORE any external call.
        _withdrawn[epochId][workId] = true;
        liability -= amount;
        emit Withdrawn(epochId, workId, amount);

        // Interactions: split-pay. All but the last payee get their floored share;
        // the last payee absorbs the rounding remainder so `amount` is fully paid.
        uint256 distributed = 0;
        uint256 lastIndex = payees.length - 1;
        for (uint256 i = 0; i < payees.length; i++) {
            uint256 share;
            if (i == lastIndex) {
                // Final payee receives everything not yet distributed (exact total).
                share = amount - distributed;
            } else {
                // Floored proportional share for all earlier payees.
                share = (amount * splits[i]) / PPM_TOTAL;
                distributed += share;
            }
            // Forward the share; revert the whole withdrawal if any transfer fails.
            (bool ok,) = payees[i].call{value: share}("");
            if (!ok) revert PayoutFailed(payees[i]);
        }
    }
}
