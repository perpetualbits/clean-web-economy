// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEEscrow} from "./interfaces/ICWEEscrow.sol";
import {ICWERegistry} from "./interfaces/ICWERegistry.sol";
import {IArbiter} from "./interfaces/IArbiter.sol";
import {ReentrancyGuard} from "./utils/ReentrancyGuard.sol";

/// @title CWEEscrow
/// @notice Holds fingerprint-matched (Tier 2) credit behind a challenge window,
///         then split-pays it to the winning work's registered payees.
/// @dev Money flow: the aggregator `commit`s fingerprint-matched credit for a
///      `(epoch, work)` pair, funded from the same pool `CWETiers`/`CWEPayouts`
///      draw from (via `receive()`). Within `CHALLENGE_WINDOW` epochs, anyone may
///      `challenge` the escrow with a competing work; the `IArbiter` decides by
///      registration priority (see `EarliestRegistrationArbiter`), and a winning
///      challenger causes the escrow to reassign. After the window, `release`
///      pays the (possibly reassigned) work's payees per the registry's splits.
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

    /// @notice The arbitration seam consulted to resolve challenges.
    IArbiter public immutable arbiter;

    /// @dev The record stored per `(epoch, work)` escrow.
    struct Escrow {
        uint256 amount; // escrowed credit, in wei
        uint256 releaseEpoch; // epoch at/after which release is permitted
        bool committed; // whether `commit` has been called for this pair
        bool released; // whether the escrow has already been paid out
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
    /// @notice Emitted when a challenge reassigns an escrow to a competing work.
    event EscrowChallenged(
        uint256 indexed epochId, bytes32 indexed escrowedWork, bytes32 indexed challengerWork
    );
    /// @notice Emitted when an escrow is released and split-paid.
    event EscrowReleased(uint256 indexed epochId, bytes32 indexed workId, uint256 amount);

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
    /// @dev Reverts when the arbiter does not favour the challenger.
    error ChallengeFailed();
    /// @dev Reverts when the work being released is not in the registry.
    error WorkNotRegistered(bytes32 workId);
    /// @dev Reverts when a payee transfer fails.
    error PayoutFailed(address payee);

    /// @param registry_ The work registry (registration priority and splits source).
    /// @param aggregator_ The address permitted to commit fingerprint-matched escrows.
    /// @param arbiter_ The arbitration seam consulted on challenges.
    constructor(ICWERegistry registry_, address aggregator_, IArbiter arbiter_) {
        registry = registry_;
        aggregator = aggregator_;
        arbiter = arbiter_;
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
    ///      pair. After adding the amount to the outstanding liability, the pool
    ///      must still cover it — otherwise the escrow is committing credit that
    ///      cannot be paid.
    function commit(uint256 epochId, bytes32 workId, uint256 amount) external {
        if (msg.sender != aggregator) revert NotAggregator();

        Escrow storage e = _escrows[epochId][workId];
        if (e.committed) revert AlreadyCommitted(epochId, workId);

        uint256 releaseEpoch = epochId + CHALLENGE_WINDOW;
        e.amount = amount;
        e.releaseEpoch = releaseEpoch;
        e.committed = true;

        // Grow the outstanding liability and assert the pool can honour it.
        liability += amount;
        if (address(this).balance < liability) {
            revert Insolvent(address(this).balance, liability);
        }

        emit EscrowCommitted(epochId, workId, amount, releaseEpoch);
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Anyone may challenge, provided the escrow is live and its window has
    ///      not closed. The arbiter is asked to resolve the pair; a challenger it
    ///      favours causes the escrow to move to `(epochId, challengerWork)` with
    ///      its amount and release epoch intact, while the old slot is cleared.
    ///      Any credit already sitting in the challenger's own slot (from a
    ///      separate, unrelated commit) is preserved and added to.
    function challenge(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork) external {
        Escrow storage from = _escrows[epochId][escrowedWork];
        if (!from.committed) revert NotEscrowed(epochId, escrowedWork);
        if (from.released) revert AlreadyReleased(epochId, escrowedWork);
        if (currentEpoch() >= from.releaseEpoch) revert WindowClosed(epochId, escrowedWork);

        // Consult the arbitration seam; only a challenger it names as winner
        // may take the escrow (earliest registration wins in the Phase 1 stub).
        bytes32 winner = arbiter.resolve(escrowedWork, challengerWork);
        if (winner != challengerWork) revert ChallengeFailed();

        uint256 amount = from.amount;
        uint256 releaseEpoch = from.releaseEpoch;
        delete _escrows[epochId][escrowedWork];

        Escrow storage to = _escrows[epochId][challengerWork];
        to.amount += amount;
        to.releaseEpoch = releaseEpoch;
        to.committed = true;

        emit EscrowChallenged(epochId, escrowedWork, challengerWork);
    }

    /// @inheritdoc ICWEEscrow
    /// @dev Verifies the escrow is committed, unreleased, and past its release
    ///      epoch, loads the (possibly reassigned) work's payees, marks it
    ///      released and reduces liability (effects) before paying out
    ///      (interactions), and is `nonReentrant`. Rounding dust folds into the
    ///      final payee so the whole escrowed amount is always dispersed.
    function release(uint256 epochId, bytes32 workId) external nonReentrant {
        Escrow storage e = _escrows[epochId][workId];
        if (!e.committed) revert NotEscrowed(epochId, workId);
        if (e.released) revert AlreadyReleased(epochId, workId);
        if (currentEpoch() < e.releaseEpoch) revert TooEarly(epochId, workId);

        // Load payees/splits; a work must be registered to receive a payout.
        address payable[] memory payees = registry.payeesOf(workId);
        uint96[] memory splits = registry.splitsOf(workId);
        if (payees.length == 0) revert WorkNotRegistered(workId);

        // Effects: mark released and reduce liability BEFORE any external call.
        uint256 amount = e.amount;
        e.released = true;
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
}
