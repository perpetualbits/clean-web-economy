# Arbitration Jury (Phase 2.3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the escrow's instant "earliest-registration" dispute rule with a real **committee of jurors** who vote on a contested ownership case, whose verdict moves the escrowed money — falling back to earliest-registration only when the committee is silent.

**Architecture:** A new `CWEJury` contract (owner-managed allowlist + async dispute lifecycle) behind a minimal `IJury` seam. `CWEEscrow` is reworked from a synchronous challenge (instant verdict) to asynchronous (challenge opens a dispute and blocks release; a new `resolveDispute` applies the verdict). The existing `EarliestRegistrationArbiter` is kept and reused as the jury's fallback.

**Tech Stack:** Solidity (Foundry/Anvil) under `chain/`; bash demos under `ops/`. No Rust changes (the `services/arbitration/` operator tool is deferred).

**Design spec:** `docs/superpowers/specs/2026-07-22-arbitration-jury-design.md`.

## Global Constraints

- **Solidity for contracts** (EVM/Foundry), bash for demos — no new Rust this cycle.
- **No attribution to any coding agent, assistant, or automated tool** anywhere — code, comments, docs, commit messages, branch/PR text. Hard rule.
- **Every function/contract has a NatSpec doc block** (`///`/`/** */`); every non-trivial line gets an inline comment **only where it adds understanding**, never noise.
- **This modifies audited money code (`CWEEscrow`).** Preserve its H1 safety invariants: no double-release; a reassignment conserves `liability` and preserves `amount`/`releaseEpoch`/`contentId`; funds can never be permanently frozen; the challenge window is measured from commit (H1 fix); reentrancy-guarded release.
- **`VOTING_WINDOW = 21 days`** (a floor — jurors need time to coordinate and gather evidence).
- **Fallback preserved:** a tie or a zero-vote dispute must resolve via `EarliestRegistrationArbiter` (the H1 default), so a silent committee never changes the honest outcome.
- `forge build` / `forge test` stay green; the full workspace gate (`cargo fmt`/`clippy`/`test`) and every existing demo stay green.

---

## File Structure

- Create: `chain/contracts/interfaces/IJury.sol` — the escrow-facing seam.
- Create: `chain/contracts/CWEJury.sol` — the committee + dispute lifecycle.
- Create: `chain/test/CWEJury.t.sol` — jury unit tests.
- Modify: `chain/contracts/CWEEscrow.sol` — async challenge, `resolveDispute`, release-blocked-while-disputed, `arbiter`→`jury`.
- Modify: `chain/contracts/interfaces/ICWEEscrow.sol` — add `resolveDispute`, update `challenge` doc.
- Modify: `chain/test/CWEEscrow.t.sol` — update to the async flow + new tests.
- Modify: `chain/script/Deploy.s.sol` — deploy the jury, wire it into the escrow, `setEscrow`, persist its address.
- Create: `ops/demo/run_arbitration_demo.sh` — the committee-overrides demo.
- Modify: `ops/demo/run_ownership_demo.sh` — update to the async dispute path.
- Modify: `ops/Makefile`, `.github/workflows/ci.yml` — `arbitration-demo` target + `arbitration-e2e` job.

Existing patterns to mirror: `Ownable(initialOwner)` + `onlyOwner` (`chain/contracts/utils/Ownable.sol`); the `setVerifiedCreator(addr,bool) onlyOwner` allowlist (`CWERegistry.sol`); `EarliestRegistrationArbiter(ICWERegistry)` (`chain/contracts/EarliestRegistrationArbiter.sol`); the escrow test helpers (`chain/test/CWEEscrow.t.sol` — `_registerAWithContent`, `_consent`).

---

## Task 1: `IJury` seam + `CWEJury` committee contract

**Files:**
- Create: `chain/contracts/interfaces/IJury.sol`, `chain/contracts/CWEJury.sol`
- Test: `chain/test/CWEJury.t.sol`

**Interfaces:**
- Produces (consumed by Task 2): `IJury { openDispute(bytes32,bytes32) returns (uint256); isResolved(uint256) view returns (bool); verdictOf(uint256) view returns (bytes32); }`.
- `CWEJury` public surface: `VOTING_WINDOW` (21 days), `fallbackArbiter`, `escrow`, `isJuror`, `setEscrow(address)`, `setJuror(address,bool)`, `openDispute`, `vote(uint256,bytes32)`, `finalize(uint256)`, `isResolved`, `verdictOf`, `voteEndOf(uint256)`, `tallyOf(uint256)`.

- [ ] **Step 1: Write `IJury.sol`**

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title IJury
/// @notice The asynchronous arbitration seam `CWEEscrow` consults for a disputed
///         escrow. Unlike the synchronous `IArbiter` (which decides instantly),
///         a jury opens a dispute, jurors vote over a window, and only then is a
///         verdict available — so the escrow opens a dispute on challenge and
///         reads the outcome later via `resolveDispute`.
interface IJury {
    /// @notice Open a dispute between the escrowed work and a challenger. Only the
    ///         authorised escrow may call. Returns a nonzero dispute id.
    function openDispute(bytes32 escrowedWork, bytes32 challengerWork)
        external
        returns (uint256 disputeId);

    /// @notice Whether a dispute has been finalized (a verdict is available).
    function isResolved(uint256 disputeId) external view returns (bool);

    /// @notice The finalized verdict (the winning work id). Reverts if not final.
    function verdictOf(uint256 disputeId) external view returns (bytes32 winner);
}
```

- [ ] **Step 2: Write the failing jury tests**

Create `chain/test/CWEJury.t.sol`. It deploys a `CWERegistry`, an `EarliestRegistrationArbiter` over it, and a `CWEJury(owner, arbiter)`; registers two works sharing a content id at distinct times (so the fallback has a real earliest-registration answer); sets the test contract as the `escrow` so it can call `openDispute`. Include these tests (real assertions, not stubs):

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEJury} from "../contracts/CWEJury.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {EarliestRegistrationArbiter} from "../contracts/EarliestRegistrationArbiter.sol";

contract CWEJuryTest is Test {
    CWEJury internal jury;
    CWERegistry internal registry;
    EarliestRegistrationArbiter internal arbiter;

    address internal owner = makeAddr("owner");
    address internal creator = makeAddr("creator");
    address internal juror1 = makeAddr("juror1");
    address internal juror2 = makeAddr("juror2");
    address internal juror3 = makeAddr("juror3");
    address internal escrow = makeAddr("escrow"); // stand-in escrow caller

    // Two competing works over the same content; workEarly registers first.
    bytes32 internal workEarly = keccak256("work-early");
    bytes32 internal workLate = keccak256("work-late");
    bytes32 internal constant CONTENT = keccak256("content");

    address payable internal payeeE;
    address payable internal payeeL;
    uint256 internal payeeEKey;
    uint256 internal payeeLKey;

    function setUp() public {
        vm.startPrank(owner);
        registry = new CWERegistry(owner);
        registry.setVerifiedCreator(creator, true);
        vm.stopPrank();
        arbiter = new EarliestRegistrationArbiter(registry);
        jury = new CWEJury(owner, arbiter);
        vm.prank(owner);
        jury.setEscrow(escrow);

        (address e, uint256 ek) = makeAddrAndKey("payeeE");
        (address l, uint256 lk) = makeAddrAndKey("payeeL");
        payeeE = payable(e); payeeEKey = ek; payeeL = payable(l); payeeLKey = lk;

        // workEarly registered first (t=1000), workLate later (t=2000).
        vm.warp(1000); _register(workEarly, payeeE, payeeEKey);
        vm.warp(2000); _register(workLate, payeeL, payeeLKey);

        vm.prank(owner); jury.setJuror(juror1, true);
        vm.prank(owner); jury.setJuror(juror2, true);
        vm.prank(owner); jury.setJuror(juror3, true);
    }

    /// @dev Register `workId` with a single 100% consenting payee over CONTENT.
    function _register(bytes32 workId, address payable payee, uint256 key) internal {
        address payable[] memory payees = new address payable[](1);
        payees[0] = payee;
        uint96[] memory splits = new uint96[](1);
        splits[0] = 1_000_000;
        bytes[] memory sigs = new bytes[](1);
        bytes32 digest = registry.consentDigest(workId, CONTENT, payee, splits[0]);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, eth);
        sigs[0] = abi.encodePacked(r, s, v);
        vm.prank(creator);
        registry.registerWork(workId, CONTENT, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// @dev Open a dispute as the authorised escrow.
    function _open() internal returns (uint256) {
        vm.prank(escrow);
        return jury.openDispute(workEarly, workLate);
    }

    /// @notice Only the authorised escrow may open a dispute.
    function test_openDispute_onlyEscrow() public {
        vm.expectRevert(CWEJury.NotEscrow.selector);
        jury.openDispute(workEarly, workLate);
    }

    /// @notice A majority for the later work overrides the earliest-registration
    ///         default (which would have picked workEarly).
    function test_finalize_majorityOverridesFallback() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.prank(juror2); jury.vote(id, workLate);
        vm.prank(juror3); jury.vote(id, workEarly);
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertTrue(jury.isResolved(id));
        assertEq(jury.verdictOf(id), workLate); // committee overrode the timestamp
        // Sanity: the fallback alone would have chosen workEarly.
        assertEq(arbiter.resolve(workEarly, workLate), workEarly);
    }

    /// @notice A zero-vote dispute falls back to earliest registration.
    function test_finalize_noVotes_fallsBackToEarliest() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertEq(jury.verdictOf(id), workEarly); // earliest registration default
    }

    /// @notice A tie falls back to earliest registration.
    function test_finalize_tie_fallsBackToEarliest() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.prank(juror2); jury.vote(id, workEarly);
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        assertEq(jury.verdictOf(id), workEarly);
    }

    /// @notice A non-juror cannot vote.
    function test_vote_onlyJuror() public {
        uint256 id = _open();
        vm.expectRevert(CWEJury.NotJuror.selector);
        vm.prank(makeAddr("stranger")); jury.vote(id, workLate);
    }

    /// @notice A juror cannot vote twice on the same dispute.
    function test_vote_twice_reverts() public {
        uint256 id = _open();
        vm.prank(juror1); jury.vote(id, workLate);
        vm.expectRevert(abi.encodeWithSelector(CWEJury.AlreadyVoted.selector, id));
        vm.prank(juror1); jury.vote(id, workEarly);
    }

    /// @notice A vote for a work that is not a party reverts.
    function test_vote_nonParty_reverts() public {
        uint256 id = _open();
        bytes32 other = keccak256("other");
        vm.expectRevert(abi.encodeWithSelector(CWEJury.NotAParty.selector, id, other));
        vm.prank(juror1); jury.vote(id, other);
    }

    /// @notice Voting after the window closes reverts.
    function test_vote_afterWindow_reverts() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        vm.expectRevert(abi.encodeWithSelector(CWEJury.VotingClosed.selector, id));
        vm.prank(juror1); jury.vote(id, workLate);
    }

    /// @notice Finalizing before the window closes reverts.
    function test_finalize_early_reverts() public {
        uint256 id = _open();
        vm.expectRevert(abi.encodeWithSelector(CWEJury.VotingOpen.selector, id));
        jury.finalize(id);
    }

    /// @notice A dispute cannot be finalized twice.
    function test_finalize_twice_reverts() public {
        uint256 id = _open();
        vm.warp(block.timestamp + jury.VOTING_WINDOW());
        jury.finalize(id);
        vm.expectRevert(abi.encodeWithSelector(CWEJury.AlreadyFinalized.selector, id));
        jury.finalize(id);
    }

    /// @notice verdictOf reverts before finalize.
    function test_verdictOf_beforeFinalize_reverts() public {
        uint256 id = _open();
        vm.expectRevert(abi.encodeWithSelector(CWEJury.NotFinalized.selector, id));
        jury.verdictOf(id);
    }

    /// @notice Only the owner may set jurors / the escrow.
    function test_admin_onlyOwner() public {
        vm.expectRevert(); jury.setJuror(juror1, true);
        vm.expectRevert(); jury.setEscrow(escrow);
    }

    /// @notice The escrow can be set only once.
    function test_setEscrow_once() public {
        vm.expectRevert(CWEJury.EscrowAlreadySet.selector);
        vm.prank(owner); jury.setEscrow(makeAddr("other"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd chain && forge test --match-contract CWEJuryTest 2>&1 | tail -5`
Expected: FAIL — `CWEJury` not found / does not compile.

- [ ] **Step 4: Implement `CWEJury.sol`**

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {IJury} from "./interfaces/IJury.sol";
import {IArbiter} from "./interfaces/IArbiter.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWEJury
/// @notice A trusted committee that resolves fingerprint-escrow ownership
///         disputes by majority vote. It is the Phase 2.3 replacement for the
///         instant earliest-registration rule: `CWEEscrow` opens a dispute on
///         challenge, allowlisted jurors vote over a window, and the tallied
///         verdict moves the escrow.
/// @dev This is deliberately a *trusted* stub — the owner appoints jurors — that
///      fills the unavoidable human-judgment layer. A future staked, open court
///      (commit-reveal voting, slashing) can replace it behind the same `IJury`
///      seam without touching the escrow's money logic. A tie or a silent
///      committee falls back to the existing `EarliestRegistrationArbiter`, so
///      the H1 safety default is preserved.
contract CWEJury is IJury, Ownable {
    /// @notice How long a dispute stays open for voting. A 3-week floor: jurors
    ///         need time to coordinate across weekends and gather evidence, so a
    ///         shorter window would only ever force the fallback.
    uint256 public constant VOTING_WINDOW = 21 days;

    /// @notice The fallback consulted on a tie or a silent committee (earliest
    ///         registration wins), preserving the H1 default.
    IArbiter public immutable fallbackArbiter;

    /// @notice The only contract permitted to open disputes (the escrow).
    address public escrow;

    /// @notice Whether an address is an allowlisted juror.
    mapping(address => bool) public isJuror;

    /// @dev Monotonic id source; ids start at 1 so 0 always means "no dispute".
    uint256 private _nextDisputeId;

    /// @dev A single dispute's state. Holds a per-juror voted flag, so it is only
    ///      ever accessed through storage references (never copied wholesale).
    struct Dispute {
        bytes32 workA; // the escrowed (incumbent) work
        bytes32 workB; // the challenger's work
        uint256 voteEnd; // timestamp at/after which finalize is allowed
        uint256 votesA;
        uint256 votesB;
        bool finalized;
        bytes32 verdict;
        mapping(address => bool) hasVoted;
    }

    /// @dev disputeId => dispute.
    mapping(uint256 => Dispute) private _disputes;

    /// @notice Emitted when the authorised escrow is set.
    event EscrowSet(address indexed escrow);
    /// @notice Emitted when a juror is added to or removed from the allowlist.
    event JurorSet(address indexed juror, bool allowed);
    /// @notice Emitted when the escrow opens a dispute.
    event DisputeOpened(
        uint256 indexed disputeId, bytes32 indexed workA, bytes32 indexed workB, uint256 voteEnd
    );
    /// @notice Emitted on each juror vote.
    event Voted(uint256 indexed disputeId, address indexed juror, bytes32 forWork);
    /// @notice Emitted when a dispute is tallied.
    event DisputeFinalized(uint256 indexed disputeId, bytes32 verdict, uint256 votesA, uint256 votesB);

    /// @dev Reverts when the escrow address is already set (settable once).
    error EscrowAlreadySet();
    /// @dev Reverts when a non-escrow address calls `openDispute`.
    error NotEscrow();
    /// @dev Reverts when a non-juror tries to vote.
    error NotJuror();
    /// @dev Reverts when acting on a dispute id that was never opened.
    error NoDispute(uint256 disputeId);
    /// @dev Reverts when acting on an already-finalized dispute.
    error AlreadyFinalized(uint256 disputeId);
    /// @dev Reverts when voting after the window has closed.
    error VotingClosed(uint256 disputeId);
    /// @dev Reverts when finalizing before the window has closed.
    error VotingOpen(uint256 disputeId);
    /// @dev Reverts when a juror votes twice on one dispute.
    error AlreadyVoted(uint256 disputeId);
    /// @dev Reverts when voting for a work that is not a party to the dispute.
    error NotAParty(uint256 disputeId, bytes32 work);
    /// @dev Reverts when reading a verdict that is not yet finalized.
    error NotFinalized(uint256 disputeId);

    /// @param initialOwner The address that appoints jurors and the escrow.
    /// @param fallbackArbiter_ The earliest-registration fallback for ties/silence.
    constructor(address initialOwner, IArbiter fallbackArbiter_) Ownable(initialOwner) {
        fallbackArbiter = fallbackArbiter_;
    }

    /// @notice Authorise the escrow that may open disputes. Settable once, so the
    ///         escrow⇄jury deploy cycle resolves without a mutable back-door.
    function setEscrow(address escrow_) external onlyOwner {
        if (escrow != address(0)) revert EscrowAlreadySet();
        escrow = escrow_;
        emit EscrowSet(escrow_);
    }

    /// @notice Add or remove an allowlisted juror (mirrors `setVerifiedCreator`).
    function setJuror(address juror, bool allowed) external onlyOwner {
        isJuror[juror] = allowed;
        emit JurorSet(juror, allowed);
    }

    /// @inheritdoc IJury
    function openDispute(bytes32 workA, bytes32 workB) external returns (uint256 disputeId) {
        if (msg.sender != escrow) revert NotEscrow();
        // Ids start at 1 so a zero disputeId unambiguously means "none".
        disputeId = ++_nextDisputeId;
        Dispute storage d = _disputes[disputeId];
        d.workA = workA;
        d.workB = workB;
        d.voteEnd = block.timestamp + VOTING_WINDOW;
        emit DisputeOpened(disputeId, workA, workB, d.voteEnd);
    }

    /// @notice Cast a juror's single vote for one of the two disputed works.
    function vote(uint256 disputeId, bytes32 forWork) external {
        if (!isJuror[msg.sender]) revert NotJuror();
        Dispute storage d = _disputes[disputeId];
        if (d.voteEnd == 0) revert NoDispute(disputeId);
        if (d.finalized) revert AlreadyFinalized(disputeId);
        if (block.timestamp >= d.voteEnd) revert VotingClosed(disputeId);
        if (d.hasVoted[msg.sender]) revert AlreadyVoted(disputeId);
        if (forWork != d.workA && forWork != d.workB) revert NotAParty(disputeId, forWork);
        d.hasVoted[msg.sender] = true; // one vote per juror per dispute
        if (forWork == d.workA) d.votesA++;
        else d.votesB++;
        emit Voted(disputeId, msg.sender, forWork);
    }

    /// @notice Tally a dispute after its voting window; anyone may call, so a
    ///         silent committee can never freeze the escrow indefinitely.
    function finalize(uint256 disputeId) external {
        Dispute storage d = _disputes[disputeId];
        if (d.voteEnd == 0) revert NoDispute(disputeId);
        if (d.finalized) revert AlreadyFinalized(disputeId);
        if (block.timestamp < d.voteEnd) revert VotingOpen(disputeId);
        // Strict majority wins; a tie or a zero-vote dispute defers to earliest
        // registration, preserving the H1 default.
        bytes32 verdict;
        if (d.votesA > d.votesB) verdict = d.workA;
        else if (d.votesB > d.votesA) verdict = d.workB;
        else verdict = fallbackArbiter.resolve(d.workA, d.workB);
        d.verdict = verdict;
        d.finalized = true;
        emit DisputeFinalized(disputeId, verdict, d.votesA, d.votesB);
    }

    /// @inheritdoc IJury
    function isResolved(uint256 disputeId) external view returns (bool) {
        return _disputes[disputeId].finalized;
    }

    /// @inheritdoc IJury
    function verdictOf(uint256 disputeId) external view returns (bytes32) {
        Dispute storage d = _disputes[disputeId];
        if (!d.finalized) revert NotFinalized(disputeId);
        return d.verdict;
    }

    /// @notice The timestamp a dispute's voting window closes (0 if no dispute).
    function voteEndOf(uint256 disputeId) external view returns (uint256) {
        return _disputes[disputeId].voteEnd;
    }

    /// @notice The current vote tally `(votesA, votesB)` for a dispute.
    function tallyOf(uint256 disputeId) external view returns (uint256 votesA, uint256 votesB) {
        Dispute storage d = _disputes[disputeId];
        return (d.votesA, d.votesB);
    }
}
```

- [ ] **Step 5: Run the jury tests to green**

Run: `cd chain && forge test --match-contract CWEJuryTest -vvv 2>&1 | tail -20`
Expected: all `CWEJuryTest` tests PASS.

- [ ] **Step 6: Commit**

```bash
git add chain/contracts/interfaces/IJury.sol chain/contracts/CWEJury.sol chain/test/CWEJury.t.sol
git commit -m "Add CWEJury committee contract and IJury seam"
```

---

## Task 2: `CWEEscrow` async rework

**Files:**
- Modify: `chain/contracts/CWEEscrow.sol`, `chain/contracts/interfaces/ICWEEscrow.sol`
- Test: `chain/test/CWEEscrow.t.sol` (update to async flow + new tests)

**Interfaces:**
- Consumes: `IJury` (Task 1).
- Produces: `CWEEscrow` now takes `IJury jury_` in its constructor; adds `resolveDispute(uint256 epochId, bytes32 escrowedWork)`; `escrowOf`/`releaseEpochOf`/`isReleased` unchanged.

- [ ] **Step 1: Change the constructor and imports**

In `CWEEscrow.sol`: replace the arbiter import/field with the jury.
- Import: replace `import {IArbiter} from "./interfaces/IArbiter.sol";` with `import {IJury} from "./interfaces/IJury.sol";`.
- Field: replace `IArbiter public immutable arbiter;` with `IJury public immutable jury;`.
- Constructor: change the third parameter and body:

```solidity
/// @param registry_ The work registry (registration priority and splits source).
/// @param aggregator_ The address permitted to commit fingerprint-matched escrows.
/// @param jury_ The arbitration jury consulted on challenges (async verdict).
constructor(ICWERegistry registry_, address aggregator_, IJury jury_) {
    registry = registry_;
    aggregator = aggregator_;
    jury = jury_;
}
```

- Remove the now-unused `error ChallengeFailed();` (the jury decides, not the escrow).

- [ ] **Step 2: Extend the `Escrow` struct with dispute tracking**

```solidity
struct Escrow {
    uint256 amount; // escrowed credit, in wei
    uint256 releaseEpoch; // epoch at/after which release is permitted
    bytes32 contentId; // the content id the escrowed work was registered under
    bool committed; // whether `commit` has been called for this pair
    bool released; // whether the escrow has already been paid out
    uint256 disputeId; // open jury dispute id (0 = none/settled)
    bytes32 challenger; // the challenging work while a dispute is open
}
```

- [ ] **Step 3: Add new events/errors**

```solidity
/// @notice Emitted when a challenge opens a jury dispute over an escrow.
event DisputeOpened(
    uint256 indexed epochId, bytes32 indexed escrowedWork, bytes32 indexed challengerWork, uint256 disputeId
);
/// @notice Emitted when a dispute's verdict is applied to the escrow.
event DisputeResolved(
    uint256 indexed epochId, bytes32 indexed escrowedWork, bytes32 indexed winner, uint256 disputeId
);

/// @dev Reverts when challenging an escrow that already has an open dispute.
error AlreadyDisputed(uint256 epochId, bytes32 workId);
/// @dev Reverts when resolving a `(epoch, work)` pair with no open dispute.
error NotDisputed(uint256 epochId, bytes32 workId);
/// @dev Reverts when resolving before the jury has finalized the dispute.
error DisputeNotResolved(uint256 epochId, bytes32 workId);
/// @dev Reverts when releasing an escrow whose dispute is not yet resolved.
error Disputed(uint256 epochId, bytes32 workId);
```

- [ ] **Step 4: Rewrite `challenge` to open a dispute (no instant reassign)**

```solidity
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
    uint256 disputeId = jury.openDispute(escrowedWork, challengerWork);
    from.disputeId = disputeId;
    from.challenger = challengerWork;
    emit DisputeOpened(epochId, escrowedWork, challengerWork, disputeId);
}
```

- [ ] **Step 5: Add `resolveDispute`**

```solidity
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
        // to.disputeId stays 0: the new holder is undisputed and releasable.
        emit DisputeResolved(epochId, escrowedWork, challengerWork, disputeId);
    } else {
        // Incumbent keeps the escrow; clear the dispute so it can release.
        from.disputeId = 0;
        from.challenger = bytes32(0);
        emit DisputeResolved(epochId, escrowedWork, escrowedWork, disputeId);
    }
}
```

- [ ] **Step 6: Block `release` while a dispute is open**

In `release`, add the dispute guard right after the `released` check:

```solidity
function release(uint256 epochId, bytes32 workId) external nonReentrant {
    Escrow storage e = _escrows[epochId][workId];
    if (!e.committed) revert NotEscrowed(epochId, workId);
    if (e.released) revert AlreadyReleased(epochId, workId);
    if (e.disputeId != 0) revert Disputed(epochId, workId); // NEW: pause while disputed
    if (currentEpoch() < e.releaseEpoch) revert TooEarly(epochId, workId);
    // ... unchanged payee load, effects, and split-pay interactions ...
}
```

- [ ] **Step 7: Update `ICWEEscrow.sol`**

Add the new function and update the `challenge` doc:

```solidity
/// @notice Challenge an escrowed credit with a competing work, opening a jury
///         dispute (the verdict is applied later via `resolveDispute`).
/// @param epochId The epoch the escrow was committed under.
/// @param escrowedWork The work currently holding the escrow.
/// @param challengerWork The competing work claiming the credit instead.
function challenge(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork) external;

/// @notice Apply a finalized jury verdict to a disputed escrow — reassigning to
///         the challenger if it won, or clearing the dispute if the incumbent did.
/// @param epochId The epoch the escrow was committed under.
/// @param escrowedWork The work the dispute was opened against.
function resolveDispute(uint256 epochId, bytes32 escrowedWork) external;
```

- [ ] **Step 8: Update the escrow tests to the async flow**

In `chain/test/CWEEscrow.t.sol`, the `setUp` must now deploy a `CWEJury` and pass it to `CWEEscrow`, and set the escrow on the jury:

```solidity
import {CWEJury} from "../contracts/CWEJury.sol";
// ...
CWEJury internal jury;
// in setUp, after arbiter is created:
jury = new CWEJury(owner, arbiter);
escrow = new CWEEscrow(registry, aggregator, jury);
vm.prank(owner);
jury.setEscrow(address(escrow));
```

Add a helper that drives a full dispute with no jurors (the earliest-registration default path) so the existing challenge tests keep their meaning:

```solidity
/// @dev Run a challenge to completion with an empty committee: open the dispute,
///      warp past the voting window, finalize (falls back to earliest
///      registration), and resolve. Returns nothing; asserts happen in callers.
function _challengeAndResolve(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork) internal {
    escrow.challenge(epochId, escrowedWork, challengerWork);
    uint256 id = escrow.disputeIdOf(epochId, escrowedWork); // add this view (below)
    vm.warp(block.timestamp + jury.VOTING_WINDOW());
    jury.finalize(id);
    escrow.resolveDispute(epochId, escrowedWork);
}
```

Add a small view to `CWEEscrow.sol` so tests/demos can read the open dispute id:

```solidity
/// @notice The open dispute id for a `(epoch, work)` escrow (0 if none).
function disputeIdOf(uint256 epochId, bytes32 workId) external view returns (uint256) {
    return _escrows[epochId][workId].disputeId;
}
```

Rework the existing H1 challenge tests and add new ones. The updated/added tests (real assertions):

- `test_challenge_opensDispute_noInstantReassign`: after `challenge`, `escrowOf(escrowedWork) == amount` (unchanged), `escrowOf(challengerWork) == 0`, and `disputeIdOf(escrowedWork) != 0`.
- `test_release_blockedWhileDisputed_reverts`: after `challenge`, warp past `releaseEpoch`, expect `release` to revert `Disputed`.
- `test_resolveDispute_challengerWins_reassigns` (empty committee, challenger registered earlier): `_challengeAndResolve`, then `escrowOf(escrowedWork) == 0`, `escrowOf(challengerWork) == amount`, then warp and `release(challengerWork)` pays the challenger's payees; `liability() == 0` at the end.
- `test_resolveDispute_incumbentKeeps` (challenger registered LATER → fallback keeps incumbent): after `_challengeAndResolve`, `escrowOf(escrowedWork) == amount`, `disputeIdOf(escrowedWork) == 0`, and `release(escrowedWork)` now succeeds past the window.
- `test_challenge_twice_reverts`: a second `challenge` on an escrow with an open dispute reverts `AlreadyDisputed`.
- `test_resolveDispute_beforeFinalize_reverts`: `challenge`, then `resolveDispute` before finalize reverts `DisputeNotResolved`.
- `test_committeeOverridesFallback`: register the escrowed (incumbent) work EARLIER and the challenger LATER (so the fallback would keep the incumbent), add jurors, `challenge`, jurors `vote` for the challenger, warp, `finalize`, `resolveDispute` → `escrowOf(challengerWork) == amount` (the committee moved it against the timestamp default).

Preserve the existing solvency, reentrancy, double-release, and commit-window tests (they don't involve disputes) — only their `setUp` changes (jury wired in).

- [ ] **Step 9: Run the full contract suite**

Run: `cd chain && forge test 2>&1 | tail -12`
Expected: `CWEJuryTest` + `CWEEscrowTest` + the rest all PASS.

- [ ] **Step 10: Commit**

```bash
git add chain/contracts/CWEEscrow.sol chain/contracts/interfaces/ICWEEscrow.sol chain/test/CWEEscrow.t.sol
git commit -m "CWEEscrow: async dispute flow — challenge opens a jury vote, resolveDispute applies the verdict"
```

---

## Task 3: Deploy wiring + update the H1 ownership demo

**Files:**
- Modify: `chain/script/Deploy.s.sol`, `ops/demo/run_ownership_demo.sh`

**Interfaces:**
- Consumes: `CWEJury` (Task 1), the reworked `CWEEscrow` (Task 2).
- Produces: `deployments/localhost.json` gains a `jury` address; the escrow is constructed with the jury and `setEscrow` is called.

- [ ] **Step 1: Wire the jury into `Deploy.s.sol`**

Add `jury` to the `Deployed` struct; deploy `CWEJury(d.owner, EarliestRegistrationArbiter(d.arbiter))` BEFORE the escrow; construct the escrow with the jury; after `stopBroadcast`, call `setEscrow` as the owner (mirroring the existing `setPayoutPool` owner-guarded broadcast). Import `CWEJury` and `IJury`.

```solidity
import {CWEJury} from "../contracts/CWEJury.sol";
import {IJury} from "../contracts/interfaces/IJury.sol";
// Deployed struct: add `address jury;`

// after the arbiter is deployed, before the escrow:
d.jury = address(new CWEJury(d.owner, EarliestRegistrationArbiter(d.arbiter)));
// escrow now takes the jury:
d.escrow = address(new CWEEscrow(CWERegistry(d.registry), d.aggregator, IJury(d.jury)));

// after vm.stopBroadcast(), alongside the setPayoutPool block:
if (d.owner == deployer) {
    vm.broadcast(deployerKey);
    CWEJury(d.jury).setEscrow(d.escrow);
}
```

In `_writeDeployments`, serialise the jury address too (add `vm.serializeAddress(obj, "jury", d.jury);` before the final key).

- [ ] **Step 2: Verify the deploy compiles and runs**

Run: `cd chain && forge build 2>&1 | tail -3`
Expected: compiles. (The demo in Step 4 exercises the deploy end-to-end.)

- [ ] **Step 3: Update `run_ownership_demo.sh` to the async path**

The H1 ownership demo's step 7 (`challenge`) previously reassigned instantly. It must now: `challenge` → warp past the voting window → `finalize` the jury dispute → `resolveDispute` → then release. The real owner (R) is registered earlier than the fraud work (F), and no jurors are added, so `finalize` falls back to earliest-registration → R wins → reassign, preserving the demo's original outcome.

Concretely, in `run_ownership_demo.sh`:
- read `JURY=$(jq -r .jury "$DEP")`.
- after the `challenge` call, capture the dispute id: `DISPUTE=$(callnum $ESCROW "disputeIdOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)` and assert it is non-zero.
- assert `escrowOf(EPOCH, WORK_REAL)` is still 0 and `escrowOf(EPOCH, WORK_FRAUD)` is still the escrowed amount immediately after `challenge` (no instant reassign).
- warp past the voting window: `warp $((21 * 24 * 3600 + 60))` (21 days + a minute).
- `send $DEPLOYER $JURY "finalize(uint256)" $DISPUTE`.
- `send $DEPLOYER $ESCROW "resolveDispute(uint256,bytes32)" $EPOCH $WORK_FRAUD`.
- keep the existing assertions: escrow reassigned to `WORK_REAL`, then warp/release pays R's payees and the fraudster gets nothing.
- note the extra 21-day warp before release means `currentEpoch()` advances further; the existing release logic (`currentEpoch >= releaseEpoch`) still holds.

- [ ] **Step 4: Run the ownership demo to green**

Run: `make -C ops ownership-demo`
Expected: ends with `✅ OWNERSHIP DEMO PASSED` (now via the async default path). Debug via the anvil log on failure; do not proceed until green.

- [ ] **Step 5: Commit**

```bash
git add chain/script/Deploy.s.sol ops/demo/run_ownership_demo.sh
git commit -m "Deploy the jury and update the ownership demo to the async dispute path"
```

---

## Task 4: `arbitration-demo` + Makefile + CI + docs

**Files:**
- Create: `ops/demo/run_arbitration_demo.sh`
- Modify: `ops/Makefile`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the full deployed set (registry, escrow, jury, payouts) + `cwe-settlement` and the `consent()`/`callnum` helpers from `run_ownership_demo.sh`.

- [ ] **Step 1: Write `run_arbitration_demo.sh`**

Self-contained, PID-safe (model the header/cleanup on `run_ownership_demo.sh`), it proves the committee **overturns** the earliest-registration rule. The numbered recipe:

1. `set -euo pipefail`; `export PATH` for foundry/cargo; `WORKDIR=$(mktemp -d)`; build `cwe-settlement`.
2. Start Anvil (`anvil > log & ANVIL=$!`), `trap 'kill -TERM "$ANVIL" 2>/dev/null || true; rm -rf "$WORKDIR"' EXIT`, wait for RPC.
3. `mapfile` dev keys: `DEPLOYER` (owner+aggregator+verified creator), `U2` (a listener), `FRAUD_PAYEE=$(cast wallet address ${KEYS[3]})`, `REAL_PAYEE=$(cast wallet address ${KEYS[4]})`, and three jurors `J1/J2/J3` (keys 5–7).
4. Deploy; read `REG/TIERS/CONS/PAY/ESCROW/JURY` from `deployments/localhost.json`. `setFee`, `setVerifiedCreator DEPLOYER`.
5. Add the committee: `send $DEPLOYER $JURY "setJuror(address,bool)" <J1addr> true` for J1, J2, J3.
6. **The fraudster registers FIRST** (earliest timestamp): `WORK_FRAUD` under `CONTENT` with payee `FRAUD_PAYEE`, consent-signed. Then `warp 100`. **The real owner registers SECOND**: `WORK_REAL` under the SAME `CONTENT` with payee `REAL_PAYEE`, consent-signed. (So earliest-registration alone favours the fraudster — assert `arbiter.resolve` is not needed; instead assert via the outcome below.)
7. A listener subscribes (funds the pool); build a disclosure crediting `WORK_FRAUD` via fingerprint (escrow) — reuse the ownership-demo's disclosure+settlement mechanics so `WORK_FRAUD` gets an escrowed amount. Run `cwe-settlement`; assert `escrowOf(EPOCH, WORK_FRAUD) > 0`.
8. The real owner challenges: `send $DEPLOYER $ESCROW "challenge(uint256,bytes32,bytes32)" $EPOCH $WORK_FRAUD $WORK_REAL`. Capture `DISPUTE=$(callnum $ESCROW "disputeIdOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)`; assert non-zero.
9. **The committee votes for the real owner** (majority): `send $J1 $JURY "vote(uint256,bytes32)" $DISPUTE $WORK_REAL` for J1 and J2 (2 of 3 → majority); optionally J3 votes `$WORK_FRAUD`.
10. Warp past the voting window (`warp $((21*24*3600 + 60))`), then `send $DEPLOYER $JURY "finalize(uint256)" $DISPUTE`. Assert `verdictOf(DISPUTE) == WORK_REAL` (the committee's choice).
11. `send $DEPLOYER $ESCROW "resolveDispute(uint256,bytes32)" $EPOCH $WORK_REAL`? — no: resolve is keyed on the *escrowed* work: `send $DEPLOYER $ESCROW "resolveDispute(uint256,bytes32)" $EPOCH $WORK_FRAUD`. Assert `escrowOf(EPOCH, WORK_FRAUD) == 0` and `escrowOf(EPOCH, WORK_REAL) ==` the escrowed amount (reassigned by the committee's verdict, against the timestamp default).
12. Warp past the release epoch; `release(EPOCH, WORK_REAL)`; assert `REAL_PAYEE` gained the amount and `FRAUD_PAYEE` gained nothing.
13. Print `✅ ARBITRATION DEMO PASSED — the committee overturned a first-registered fraudster.` (else a clear `FAIL:` + `exit 1`).

The point the demo makes explicit in its output: earliest-registration alone would have kept the fraudster (registered first); the committee's majority moved the money to the real owner — something the old rule could not do.

- [ ] **Step 2: Run the demo to green**

Run: `make -C ops arbitration-demo` (add the target first, Step 3) — iterate until it prints `✅ ARBITRATION DEMO PASSED`. Debug via the anvil log. Do not weaken an assertion to force green; if blocked by a real contract bug, stop and report it.

- [ ] **Step 3: Add the Makefile target**

In `ops/Makefile`, add `arbitration-demo` to `.PHONY` and:

```make
arbitration-demo: ## Run the arbitration-jury end-to-end demo (self-contained Anvil)
	bash demo/run_arbitration_demo.sh
```

- [ ] **Step 4: Add the CI job**

In `.github/workflows/ci.yml`, add an `arbitration-e2e` job mirroring `ownership-e2e` (checkout; install Rust; `Swatinem/rust-cache`; install Foundry; install jq; `make -C ops arbitration-demo`).

- [ ] **Step 5: Full gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && ( cd chain && forge test ) && make -C ops arbitration-demo && make -C ops ownership-demo` — all green. (Foundry is at `$HOME/.foundry/bin`.)

Scan every new/changed file for stray agent/assistant attributions, then:

```bash
git add ops/demo/run_arbitration_demo.sh ops/Makefile .github/workflows/ci.yml
git commit -m "Add arbitration-jury end-to-end demo and CI job"
```

---

## Self-Review

**Spec coverage:** the committee contract (T1: allowlist, open/vote/finalize, majority, tie/zero→earliest-registration fallback, only-escrow-opens); the async escrow rework (T2: challenge opens a dispute, release blocked while disputed, resolveDispute reassigns-or-keeps, one-dispute-per-escrow, conserves liability, no permanent freeze); deploy wiring + the H1 ownership demo updated to the async default path (T3); the committee-overrides demo + CI (T4). Deferred items (staked open court, filing bond, Rust operator tool, random selection) are stated seams, not built — matching the spec.

**Placeholder scan:** the contracts (IJury, CWEJury, the CWEEscrow diffs) carry full code; the tests give real assertions for every enumerated case; the demos are explicit numbered recipes referencing the concrete helpers they reuse (`run_ownership_demo.sh`'s `consent`/`callnum`/`warp`). No "TBD"/"add error handling"/"write tests for the above" remain.

**Type consistency:** `IJury` (T1) is consumed by `CWEEscrow` (T2) and deploy (T3); `CWEJury.setEscrow`/`setJuror`/`openDispute`/`vote`/`finalize`/`verdictOf` signatures match across the tests, the escrow calls, and the demos; the escrow's new `disputeIdOf` view (added in T2) is used by both demos (T3, T4); `VOTING_WINDOW` (21 days) is referenced identically in tests and both demos' warps; the reassignment in `resolveDispute` mirrors the H1 challenge reassignment (amount/releaseEpoch/contentId, liability untouched).
