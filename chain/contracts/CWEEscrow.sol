// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEEscrow} from "./interfaces/ICWEEscrow.sol";
import {ICWERegistry} from "./interfaces/ICWERegistry.sol";
import {IJury} from "./interfaces/IJury.sol";
import {ReentrancyGuard} from "./utils/ReentrancyGuard.sol";

/// @title CWEEscrow
/// @notice Holds fingerprint-matched (Tier 2) credit behind a challenge window,
///         then split-pays it to the winning work's registered payees.
/// @dev Money flow: the aggregator `commit`s fingerprint-matched credit for a
///      `(epoch, work)` pair, funded from the same pool `CWETiers`/`CWEPayouts`
///      draw from (via `receive()`). Within `CHALLENGE_WINDOW` epochs, anyone may
///      `challenge` the escrow with a competing work; this OPENS an asynchronous
///      jury dispute (see `IJury`) rather than deciding instantly, and blocks
///      `release` until `resolveDispute` applies the finalized verdict. A
///      winning challenger causes the escrow to reassign; the incumbent keeping
///      it simply clears the dispute. After the window (and once any dispute is
///      resolved), `release` pays the (possibly reassigned) work's payees per
///      the registry's splits.
///
///      Safety properties (see the tests), mirroring `CWEPayouts`:
///        * no double-release — each `(epoch, work)` releases at most once;
///        * full dispersal — a release pays out its entire escrowed amount;
///        * solvency — a commit cannot escrow more than the pool holds;
///        * reentrancy-safe — `release` is guarded and follows checks-effects-interactions.
contract CWEEscrow is ICWEEscrow, ReentrancyGuard {
    /// @notice ppm denominator for splits (matches `CWERegistry.PPM_TOTAL`).
    uint96 private constant PPM_TOTAL = 1_000_000;

    /// @notice Length of one settlement epoch, matching `CWEConsumption`.
    uint256 public constant EPOCH_LENGTH = 30 days;

    /// @notice How many epochs an escrow stays challengeable before it releases.
    uint256 public constant CHALLENGE_WINDOW = 1;

    /// @notice The registry consulted for registration priority and payout splits.
    ICWERegistry public immutable registry;

    /// @notice The only address allowed to commit fingerprint-matched escrows.
    address public immutable aggregator;

    /// @notice The arbitration jury consulted on challenges (async verdict).
    IJury public immutable jury;

    /// @dev The record stored per `(epoch, work)` escrow.
    struct Escrow {
        uint256 amount; // escrowed credit, in wei
        uint256 releaseEpoch; // epoch at/after which release is permitted
        bytes32 contentId; // the content id the escrowed work was registered under
        bool committed; // whether `commit` has been called for this pair
        bool released; // whether the escrow has already been paid out
        uint256 disputeId; // open jury dispute id (0 = none/settled)
        bytes32 challenger; // the challenging work while a dispute is open
    }

    /// @dev epoch => workId => escrow record.
    mapping(uint256 => mapping(bytes32 => Escrow)) private _escrows;

    /// @notice Total committed-but-not-yet-released credit across all epochs.
    /// @dev The pool balance must always cover this; enforced at commit time.
    uint256 public liability;

    /// @notice Emitted when the aggregator escrows fingerprint-matched credit.
    event EscrowCommitted(
        uint256 indexed epochId, bytes32 indexed workId, uint256 amount, uint256 releaseEpoch
    );
    /// @notice Emitted when an escrow is released and split-paid.
    event EscrowReleased(uint256 indexed epochId, bytes32 indexed workId, uint256 amount);
    /// @notice Emitted when a challenge opens a jury dispute over an escrow.
    event DisputeOpened(
        uint256 indexed epochId,
        bytes32 indexed escrowedWork,
        bytes32 indexed challengerWork,
        uint256 disputeId
    );
    /// @notice Emitted when a dispute's verdict is applied to the escrow.
    event DisputeResolved(
        uint256 indexed epochId, bytes32 indexed escrowedWork, bytes32 indexed winner, uint256 disputeId
    );

    /// @dev Reverts when a non-aggregator calls `commit`.
    error NotAggregator();
    /// @dev Reverts when committing a `(epoch, work)` pair that is already committed.
    error AlreadyCommitted(uint256 epochId, bytes32 workId);
    /// @dev Reverts when the pool cannot cover the committed liability.
    error Insolvent(uint256 balance, uint256 liability);
    /// @dev Reverts when acting on a `(epoch, work)` pair that was never committed.
    error NotEscrowed(uint256 epochId, bytes32 workId);
    /// @dev Reverts when acting on an escrow that was already released.
    error AlreadyReleased(uint256 epochId, bytes32 workId);
    /// @dev Reverts when challenging an escrow whose window has already closed.
    error WindowClosed(uint256 epochId, bytes32 workId);
    /// @dev Reverts when releasing before the escrow's challenge window has elapsed.
    error TooEarly(uint256 epochId, bytes32 workId);
    /// @dev Reverts when the work being released is not in the registry.
    error WorkNotRegistered(bytes32 workId);
    /// @dev Reverts when a payee transfer fails.
    error PayoutFailed(address payee);
    /// @dev Reverts when challenging an escrow with a work that is itself
    ///      already the escrow holder.
    error SelfChallenge(bytes32 workId);
    /// @dev Reverts when the challenger's content id does not match the
    ///      escrowed work's content id.
    error ContentMismatch(bytes32 escrowedContentId, bytes32 challengerContentId);
    /// @dev Reverts when challenging an escrow that already has an open dispute.
    error AlreadyDisputed(uint256 epochId, bytes32 workId);
    /// @dev Reverts when resolving a `(epoch, work)` pair with no open dispute.
    error NotDisputed(uint256 epochId, bytes32 workId);
    /// @dev Reverts when resolving before the jury has finalized the dispute.
    error DisputeNotResolved(uint256 epochId, bytes32 workId);
    /// @dev Reverts when releasing an escrow whose dispute is not yet resolved.
    error Disputed(uint256 epochId, bytes32 workId);

    /// @param registry_ The work registry (registration priority and splits source).
    /// @param aggregator_ The address permitted to commit fingerprint-matched escrows.
    /// @param jury_ The arbitration jury consulted on challenges (async verdict).
    constructor(ICWERegistry registry_, address aggregator_, IJury jury_) {
        registry = registry_;
        aggregator = aggregator_;
        jury = jury_;
    }

    /// @notice Accept pool funding (forwarded from the payout pool, or direct
    ///         top-ups). Funds are undifferentiated pool liquidity.
    receive() external payable {}

    /// @notice The current epoch derived from the block timestamp.
    /// @return The current epoch number.
    function currentEpoch() public view returns (uint256) {
        return block.timestamp / EPOCH_LENGTH;
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Only the aggregator may commit, and only once per `(epoch, work)`
    ///      pair. The work must already be registered — this guarantees
    ///      `release` always has payees to pay and, combined with the arbiter's
    ///      handling of unregistered works, means an unregistered incumbent can
    ///      never lock the escrow. The work's content id is recorded so a later
    ///      `challenge` can be bound to the same content.
    ///
    ///      The challenge window runs from the CURRENT (commit) epoch, not from
    ///      `epochId`. `epochId` is the past *usage* epoch — settlement can only
    ///      run once that epoch has closed, so it is always behind
    ///      `currentEpoch()`. Keying the window off `epochId` would place the
    ///      release epoch in the past, giving a zero-length window that no one
    ///      could challenge; keying it off `currentEpoch()` gives every commit a
    ///      full `CHALLENGE_WINDOW` regardless of how far settlement lagged.
    function commit(uint256 epochId, bytes32 workId, uint256 amount) external {
        if (msg.sender != aggregator) revert NotAggregator();
        if (!registry.isRegistered(workId)) revert WorkNotRegistered(workId);

        Escrow storage e = _escrows[epochId][workId];
        if (e.committed) revert AlreadyCommitted(epochId, workId);

        // Window is measured from commit time, so a lagging settlement still
        // leaves a full challenge window open (see the @dev note above).
        uint256 releaseEpoch = currentEpoch() + CHALLENGE_WINDOW;
        e.amount = amount;
        e.releaseEpoch = releaseEpoch;
        e.contentId = registry.contentIdOf(workId);
        e.committed = true;

        // Grow the outstanding liability and assert the pool can honour it.
        liability += amount;
        if (address(this).balance < liability) {
            revert Insolvent(address(this).balance, liability);
        }

        emit EscrowCommitted(epochId, workId, amount, releaseEpoch);
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Anyone may challenge, provided the escrow is live, its window is open,
    ///      and it has no dispute already. The challenger's work must share the
    ///      escrowed work's content id (an exact-content requirement; perceptual
    ///      disputes are out of scope). Rather than deciding here, the escrow OPENS
    ///      an asynchronous jury dispute and blocks release until `resolveDispute`
    ///      applies the verdict.
    function challenge(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork) external {
        if (escrowedWork == challengerWork) revert SelfChallenge(escrowedWork);

        Escrow storage from = _escrows[epochId][escrowedWork];
        if (!from.committed) revert NotEscrowed(epochId, escrowedWork);
        if (from.released) revert AlreadyReleased(epochId, escrowedWork);
        if (currentEpoch() >= from.releaseEpoch) revert WindowClosed(epochId, escrowedWork);
        if (from.disputeId != 0) revert AlreadyDisputed(epochId, escrowedWork); // one per escrow

        bytes32 challengerContentId = registry.contentIdOf(challengerWork);
        if (challengerContentId != from.contentId) {
            revert ContentMismatch(from.contentId, challengerContentId);
        }

        // Open the dispute and remember the challenger; the verdict lands later.
        // The store below necessarily happens after this call, since it needs
        // the id the call returns; `jury` is trusted, immutable, and makes no
        // external calls of its own, so this ordering cannot be exploited to
        // reenter the escrow.
        uint256 disputeId = jury.openDispute(escrowedWork, challengerWork);
        from.disputeId = disputeId;
        from.challenger = challengerWork;
        emit DisputeOpened(epochId, escrowedWork, challengerWork, disputeId);
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Applies a finalized jury verdict to an escrow. Anyone may call once the
    ///      jury has resolved the dispute. If the challenger won, the escrow
    ///      reassigns to it (the same reassignment the H1 challenge did, now
    ///      verdict-gated: amount/releaseEpoch/contentId move, `liability` is
    ///      unchanged); otherwise the incumbent keeps it. Either way the dispute is
    ///      cleared so the winning work becomes releasable.
    function resolveDispute(uint256 epochId, bytes32 escrowedWork) external {
        Escrow storage from = _escrows[epochId][escrowedWork];
        uint256 disputeId = from.disputeId;
        if (disputeId == 0) revert NotDisputed(epochId, escrowedWork);
        if (from.released) revert AlreadyReleased(epochId, escrowedWork);
        if (!jury.isResolved(disputeId)) revert DisputeNotResolved(epochId, escrowedWork);

        bytes32 winner = jury.verdictOf(disputeId);
        bytes32 challengerWork = from.challenger;

        if (winner == challengerWork) {
            // Challenger wins: move the escrow to the challenger's slot intact.
            uint256 amount = from.amount;
            uint256 releaseEpoch = from.releaseEpoch;
            bytes32 contentId = from.contentId;
            delete _escrows[epochId][escrowedWork];

            Escrow storage to = _escrows[epochId][challengerWork];
            to.amount += amount;
            to.releaseEpoch = releaseEpoch;
            to.contentId = contentId;
            to.committed = true;
            // The challenger's OWN escrow for this (epoch, work) may already have
            // been released independently -- release is permissionless and the
            // voting window (21 days) is long enough for that slot's own
            // challenge window to have elapsed while this dispute was pending.
            // If so, its prior `amount` was already zeroed on that release (see
            // `release`), so the `+=` above is exactly the reassigned amount --
            // nothing is double-paid. But a released slot can never be released
            // again (`AlreadyReleased`), so without reopening it here the
            // reassigned money would be frozen in this contract forever. Reset
            // `released` so the reassigned amount can actually reach the
            // dispute-winning challenger's payees; if the slot was never
            // released this is a harmless no-op (it is already false).
            to.released = false;
            // to.disputeId stays 0: the new holder is undisputed and releasable.
            emit DisputeResolved(epochId, escrowedWork, challengerWork, disputeId);
        } else {
            // Incumbent keeps the escrow; clear the dispute so it can release.
            from.disputeId = 0;
            from.challenger = bytes32(0);
            emit DisputeResolved(epochId, escrowedWork, escrowedWork, disputeId);
        }
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Verifies the escrow is committed, unreleased, not under an open
    ///      dispute (see `resolveDispute`), and past its release epoch, loads
    ///      the (possibly reassigned) work's payees, marks it released and
    ///      reduces liability (effects) before paying out (interactions), and
    ///      is `nonReentrant`. Rounding dust folds into the final payee so the
    ///      whole escrowed amount is always dispersed. The escrow's amount is
    ///      zeroed so `escrowOf` correctly reports a released escrow as no
    ///      longer outstanding.
    function release(uint256 epochId, bytes32 workId) external nonReentrant {
        Escrow storage e = _escrows[epochId][workId];
        if (!e.committed) revert NotEscrowed(epochId, workId);
        if (e.released) revert AlreadyReleased(epochId, workId);
        if (e.disputeId != 0) revert Disputed(epochId, workId); // pause while disputed
        if (currentEpoch() < e.releaseEpoch) revert TooEarly(epochId, workId);

        // Load payees/splits; a work must be registered to receive a payout.
        address payable[] memory payees = registry.payeesOf(workId);
        uint96[] memory splits = registry.splitsOf(workId);
        if (payees.length == 0) revert WorkNotRegistered(workId);

        // Effects: mark released, zero the outstanding amount, and reduce
        // liability BEFORE any external call.
        uint256 amount = e.amount;
        e.released = true;
        e.amount = 0;
        liability -= amount;
        emit EscrowReleased(epochId, workId, amount);

        // Interactions: split-pay. All but the last payee get their floored
        // share; the last payee absorbs the rounding remainder so `amount` is
        // fully paid.
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
            // Forward the share; revert the whole release if any transfer fails.
            (bool ok,) = payees[i].call{value: share}("");
            if (!ok) revert PayoutFailed(payees[i]);
        }
    }

    /// @inheritdoc ICWEEscrow
    function escrowOf(uint256 epochId, bytes32 workId) external view returns (uint256) {
        return _escrows[epochId][workId].amount;
    }

    /// @inheritdoc ICWEEscrow
    function releaseEpochOf(uint256 epochId, bytes32 workId) external view returns (uint256) {
        return _escrows[epochId][workId].releaseEpoch;
    }

    /// @inheritdoc ICWEEscrow
    function isReleased(uint256 epochId, bytes32 workId) external view returns (bool) {
        return _escrows[epochId][workId].released;
    }

    /// @notice The open dispute id for a `(epoch, work)` escrow (0 if none).
    function disputeIdOf(uint256 epochId, bytes32 workId) external view returns (uint256) {
        return _escrows[epochId][workId].disputeId;
    }
}
