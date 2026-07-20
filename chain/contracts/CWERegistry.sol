// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWERegistry} from "./interfaces/ICWERegistry.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWERegistry
/// @notice Registry mapping each work to its payees and their payout splits.
/// @dev Hardens the Phase 0 draft with: a verified-creator allowlist (a stand-in
///      for real SSI/VC identity, which is Phase 3+), strict split validation
///      (shares must sum to 1_000_000 ppm), and update rules (only the original
///      registrant may re-register a work).
contract CWERegistry is ICWERegistry, Ownable {
    /// @notice The ppm denominator: every work's splits must sum to exactly this.
    uint96 public constant PPM_TOTAL = 1_000_000;

    /// @dev The full record stored per work.
    struct Work {
        address payable[] payees; // who gets paid
        uint96[] splits; // each payee's share in ppm, aligned with `payees`
        uint256 pricePerMin; // creator price per minute (informational on-chain)
        bytes32 regionRule; // opaque regional-pricing tag
        address registrant; // who first registered the work (only they may update)
        bool exists; // distinguishes a registered work from a zero-value slot
    }

    /// @dev workId => stored record.
    mapping(bytes32 => Work) private _works;

    /// @notice Whether an address is an allowlisted (verified) creator.
    mapping(address => bool) public isVerifiedCreator;

    /// @notice Emitted when a work is registered for the first time.
    event WorkRegistered(bytes32 indexed workId, address indexed registrant);
    /// @notice Emitted when an existing work is updated by its registrant.
    event WorkUpdated(bytes32 indexed workId, address indexed registrant);
    /// @notice Emitted when the owner adds or removes a verified creator.
    event VerifiedCreatorSet(address indexed creator, bool verified);

    /// @dev Reverts when a non-verified address tries to register a work.
    error NotVerifiedCreator();
    /// @dev Reverts when payees and splits differ in length or are empty.
    error BadArrayLengths();
    /// @dev Reverts when a payee address is the zero address.
    error ZeroPayee();
    /// @dev Reverts when a split share is zero (a payee that would never be paid).
    error ZeroSplit();
    /// @dev Reverts when the splits do not sum to exactly PPM_TOTAL.
    error SplitsNotFull(uint256 sum);
    /// @dev Reverts when someone other than the original registrant updates a work.
    error NotRegistrant();

    /// @param initialOwner The address allowed to manage the creator allowlist.
    constructor(address initialOwner) Ownable(initialOwner) {}

    /// @notice Add or remove a verified creator (the Phase 1 identity stand-in).
    /// @param creator The creator address.
    /// @param verified True to allow registration, false to revoke.
    function setVerifiedCreator(address creator, bool verified) external onlyOwner {
        isVerifiedCreator[creator] = verified;
        emit VerifiedCreatorSet(creator, verified);
    }

    /// @inheritdoc ICWERegistry
    /// @dev Registration and update share this entry point. A first call records
    ///      the caller as the registrant; later calls must come from that same
    ///      registrant. All calls must pass split validation.
    function registerWork(
        bytes32 workId,
        address payable[] calldata payees,
        uint96[] calldata splits,
        uint256 pricePerMin,
        bytes32 regionRule
    ) external {
        // Only allowlisted creators may register or update works.
        if (!isVerifiedCreator[msg.sender]) revert NotVerifiedCreator();
        // Validate the payee/split arrays before touching storage.
        _validateSplits(payees, splits);

        Work storage work = _works[workId];
        bool isNew = !work.exists;
        if (isNew) {
            // First registration: the caller becomes the immutable registrant.
            work.registrant = msg.sender;
            work.exists = true;
        } else if (work.registrant != msg.sender) {
            // Updates are restricted to whoever first registered the work.
            revert NotRegistrant();
        }

        // Overwrite the mutable fields (arrays are replaced wholesale).
        work.payees = payees;
        work.splits = splits;
        work.pricePerMin = pricePerMin;
        work.regionRule = regionRule;

        // Emit the event that matches whether this created or updated the work.
        if (isNew) {
            emit WorkRegistered(workId, msg.sender);
        } else {
            emit WorkUpdated(workId, msg.sender);
        }
    }

    /// @inheritdoc ICWERegistry
    function payeesOf(bytes32 workId) external view returns (address payable[] memory) {
        return _works[workId].payees;
    }

    /// @inheritdoc ICWERegistry
    function splitsOf(bytes32 workId) external view returns (uint96[] memory) {
        return _works[workId].splits;
    }

    /// @inheritdoc ICWERegistry
    function isRegistered(bytes32 workId) external view returns (bool) {
        return _works[workId].exists;
    }

    /// @notice The price-per-minute a work was registered with (informational).
    /// @param workId The work identifier.
    /// @return The price per minute.
    function pricePerMinOf(bytes32 workId) external view returns (uint256) {
        return _works[workId].pricePerMin;
    }

    /// @dev Enforce the split rules: equal non-empty lengths, no zero payee, no
    ///      zero share, and shares summing to exactly PPM_TOTAL. Reverting here
    ///      guarantees `CWEPayouts` can always disburse the full credited amount.
    function _validateSplits(address payable[] calldata payees, uint96[] calldata splits)
        private
        pure
    {
        // Arrays must be the same non-zero length, one split per payee.
        if (payees.length == 0 || payees.length != splits.length) revert BadArrayLengths();

        uint256 sum = 0;
        for (uint256 i = 0; i < payees.length; i++) {
            // A zero payee could burn funds; a zero split is a payee that never pays.
            if (payees[i] == address(0)) revert ZeroPayee();
            if (splits[i] == 0) revert ZeroSplit();
            sum += splits[i];
        }
        // The shares must exactly partition the payout (no dust, no over-allocation).
        if (sum != PPM_TOTAL) revert SplitsNotFull(sum);
    }
}
