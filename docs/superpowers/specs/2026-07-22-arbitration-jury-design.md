<!-- File: docs/superpowers/specs/2026-07-22-arbitration-jury-design.md -->

# Arbitration Jury (Phase 2.3) — MVP Design

**Status date:** 2026-07-22
**Cycle:** Phase 2.3 — Arbitration jury flow (feature track)
**Depends on:** H1 (`CWEEscrow`, `IArbiter`/`EarliestRegistrationArbiter`, the
challenge/escrow spine), Phase 1 (`CWERegistry`).

---

## 1. Objective and guiding principle

Replace the blunt automatic dispute rule ("whoever registered first wins") with a
real **committee of jurors** who look at a contested ownership case and vote, and
whose verdict moves the escrowed money. The earliest-registration rule is kept only
as a safe fallback when the committee is silent.

### The layering this respects (protocol vs. judgment vs. operations)

A fair-economy that adjudicates real-world ownership cannot be *pure* protocol,
because code cannot know who actually created a work. The design keeps three layers
distinct so that as much as possible stays trustless:

1. **Rules layer (smart contracts).** The escrow money-flow, payout math, and the
   dispute lifecycle are on-chain and, once deployed, are not maintainer-alterable —
   the "protocol" part.
2. **Judgment layer (the jury).** Deciding a contested case is an unavoidable
   human-judgment step. This MVP fills it with a **trusted, owner-appointed
   committee**; a future staked, open "court" (with slashing) graduates the same
   seam to *trustless* judgment without touching the rules layer.
3. **Operations layer (off-chain services).** Running the hub, the settlement
   aggregator, and (future) a juror-coordination service — the "software +
   sysadmin" part.

The goal is not zero humans (impossible for ownership adjudication) but **zero
privileged humans over time**: shrink the trusted surface, then decentralise it.
The jury contract is the swappable seam where the judgment layer plugs in.

### In scope

- A new `CWEJury` contract: owner-managed juror allowlist, dispute open/vote/finalize
  lifecycle, majority tally with an earliest-registration fallback.
- `CWEEscrow` reworked from **synchronous** challenge (instant verdict) to
  **asynchronous** dispute: `challenge` opens a dispute and blocks release; a new
  `resolveDispute` applies the finalized verdict.
- Reuse of the existing `EarliestRegistrationArbiter` as the jury's fallback.
- Deploy wiring, a `make arbitration-demo` proving the committee can *overturn* the
  timestamp rule, updated H1 ownership demo + escrow tests, Foundry test suites, CI.

### Out of scope (deferred seams)

- The **staked, open court** (anyone can be a juror by staking; commit-reveal voting;
  slashing) — the trustless graduation of the judgment layer.
- A **filing bond/stake** to deter frivolous disputes (needs the same money-for-jurors
  machinery; the anti-griefing guards in §5 cover the MVP without it).
- The Rust **`services/arbitration/` operator tool** (file disputes / notify jurors /
  cast votes from a CLI) — convenience scaffolding over the contracts, deferred.
- **Random/weighted juror selection**, evidence interpretation (a dispute may carry an
  opaque evidence reference, but the contract does not read it).

---

## 2. Decisions (locked in brainstorming)

| # | Decision | Choice |
|---|---|---|
| D1 | Escrow integration | **Async rework** — `challenge` opens a dispute + pauses release; `resolveDispute` settles it. `CWEEscrow` is modified (a breaking change to the money contract, reviewed accordingly). |
| D2 | Juror model | **Owner-managed allowlist committee; one vote each; simple majority.** |
| D3 | Fallback on tie / no votes | **Earliest-registration**, reusing `EarliestRegistrationArbiter` — preserves H1's safety property. |
| D4 | Anti-griefing | Content-correlation (H1) + one-dispute-per-escrow + bounded, **permissionless** finalize; a filing bond is deferred. |
| D5 | Operator tool | Rust `services/arbitration/` **deferred**; MVP is contracts + a headless demo. |
| D6 | Trust model | Trusted committee now; **trustless staked court is a future swap** at the same seam. |

---

## 3. Architecture

### Contracts

- **`chain/contracts/CWEJury.sol`** (new) — implements the dispute lifecycle and the
  committee. Holds an `owner`, a juror allowlist, an `IArbiter fallbackArbiter` (the
  existing `EarliestRegistrationArbiter`, which already reads the registry for the
  earliest-registration default — so the jury needs no direct registry reference),
  and the authorised `escrow` address (only it may open disputes).
- **`chain/contracts/interfaces/IJury.sol`** (new) — the minimal surface `CWEEscrow`
  depends on: `openDispute`, `isResolved`, `verdictOf`.
- **`chain/contracts/CWEEscrow.sol`** (modified) — its `IArbiter arbiter` becomes
  `IJury jury`; `challenge` opens a dispute instead of resolving; `release` blocks
  while a dispute is unresolved; new `resolveDispute` applies the verdict.
- **`chain/contracts/EarliestRegistrationArbiter.sol`** + **`IArbiter.sol`** — kept,
  now consumed by `CWEJury` (the fallback) instead of by `CWEEscrow`.

### `IJury` (the escrow-facing seam)

```solidity
interface IJury {
    /// Open a dispute between the escrowed work and a challenger. Only the
    /// authorised escrow may call. Returns a nonzero dispute id.
    function openDispute(bytes32 escrowedWork, bytes32 challengerWork) external returns (uint256 disputeId);
    /// Whether a dispute has been finalized (a verdict is available).
    function isResolved(uint256 disputeId) external view returns (bool);
    /// The finalized verdict (the winning work id). Reverts if not finalized.
    function verdictOf(uint256 disputeId) external view returns (bytes32 winner);
}
```

### `CWEJury` (the committee)

State: `owner`; `mapping(address => bool) isJuror`; `IArbiter fallbackArbiter`;
`address escrow`; `uint256 nextDisputeId`; and per dispute a record:

```solidity
struct Dispute {
    bytes32 workA;        // the escrowed (incumbent) work
    bytes32 workB;        // the challenger's work
    uint256 voteEnd;      // block.timestamp after which finalize is allowed
    uint256 votesA;
    uint256 votesB;
    bool finalized;
    bytes32 verdict;      // set on finalize
    mapping(address => bool) hasVoted;
}
```

Functions:
- `setEscrow(address)` — owner-only, once (resolves the escrow⇄jury deploy cycle).
- `addJuror(address)` / `removeJuror(address)` — owner-only (mirrors the
  verified-creator allowlist pattern).
- `openDispute(workA, workB)` — **only `escrow`**; assigns `disputeId = ++nextDisputeId`
  (so 0 always means "no dispute"), sets `voteEnd = block.timestamp + VOTING_WINDOW`.
- `vote(disputeId, forWork)` — only a juror; dispute must exist, be unfinalized, and
  `block.timestamp < voteEnd`; the juror must not have voted; `forWork` must be `workA`
  or `workB`. Increments the matching tally and marks `hasVoted`.
- `finalize(disputeId)` — **anyone**, once, and only after `voteEnd`. Verdict:
  `votesA > votesB → workA`; `votesB > votesA → workB`; **tie or zero votes →
  `fallbackArbiter.resolve(workA, workB)`** (earliest registration). Sets `finalized`
  and `verdict`.
- `isResolved` / `verdictOf` — the `IJury` views.

`VOTING_WINDOW` is a public constant, **`21 days` (3 weeks) minimum**. This is a
floor, not a convenience value: real jurors need time to align schedules across
weekends and to gather evidence, so anything shorter would just force the
earliest-registration fallback in practice. It still sits inside the escrow's money
lifecycle (release is gated on the dispute clearing, not on the 30-day epoch), and
tests/demos warp past it.

### `CWEEscrow` changes

The `Escrow` struct gains dispute tracking: `uint256 disputeId` (0 = none/settled)
and `bytes32 challenger`.

- **`challenge(epochId, escrowedWork, challengerWork)`** — keeps every H1 guard
  (not self-challenge; escrow committed & unreleased; within the challenge window
  `currentEpoch() < releaseEpoch`; challenger's `contentId` equals the escrowed
  work's). **Adds:** require no open dispute (`e.disputeId == 0`, one-per-escrow).
  Then `e.disputeId = jury.openDispute(escrowedWork, challengerWork)` and
  `e.challenger = challengerWork`. It **no longer reassigns** here.
- **`resolveDispute(epochId, escrowedWork)`** (new) — anyone. Requires
  `e.disputeId != 0` and `jury.isResolved(e.disputeId)`. Reads
  `winner = jury.verdictOf(e.disputeId)`. If `winner == e.challenger`: reassign the
  escrow to the challenger (move `amount`/`releaseEpoch`/`contentId` into the
  challenger's slot with `disputeId == 0`; delete the old slot) — the same
  reassignment H1 did, just gated by the verdict. Otherwise the incumbent keeps it:
  clear `e.disputeId = 0` (and `e.challenger`). Emits an event either way.
- **`release(epochId, workId)`** — unchanged except it now also requires
  `e.disputeId == 0` (a work with an unresolved dispute cannot be released). Because
  `resolveDispute` is the only thing that clears `disputeId`, release is blocked from
  `challenge` until the verdict is applied — the "pause".

**Safety review points (this modifies audited money code):** no double-release; a
reassignment preserves `amount`/`releaseEpoch`/`contentId` and never changes
`liability`; funds can never be permanently frozen (after `voteEnd`, *anyone* can
`finalize` then `resolveDispute`); one dispute per escrow; the challenger can only be
a same-content work; the earliest-registration fallback still governs a silent
committee.

---

## 4. The dispute lifecycle (happy path + the override)

```
commit (fingerprint escrow to incumbent work F)
  │
challenge(epoch, F, R)        ── same-content guard; opens a dispute
  │   escrow: disputeId = jury.openDispute(F, R); release BLOCKED
  │   jury:   Dispute{workA=F, workB=R, voteEnd = now + VOTING_WINDOW}
  │
vote(id, R) × committee       ── each allowlisted juror votes once
  │
[warp past voteEnd]
finalize(id)                  ── anyone; majority → verdict
  │   votesR > votesF → verdict = R    (committee OVERRIDES earliest-reg,
  │   tie/zero → fallbackArbiter          which alone would have kept F)
  │
resolveDispute(epoch, F)      ── anyone; winner == challenger R → reassign F→R
  │   escrow: escrow moves to R, disputeId cleared, release UNBLOCKED
  │
[warp past releaseEpoch] release(epoch, R) → split-pay R's payees
```

The headline the demo proves: a **fraudster who registered first** is overturned by a
committee siding with the real artist — an outcome the earliest-registration rule
alone could never produce.

---

## 5. Anti-griefing and timing

A pending dispute freezes money, so the design bounds the harm:

- **Content-correlation (kept from H1):** a challenger must claim the escrowed work's
  exact `contentId`. You cannot freeze an escrow over content you have no claim to.
- **One dispute per escrow:** `challenge` reverts if `e.disputeId != 0`. An escrow
  cannot be re-disputed after it resolves.
- **Bounded, permissionless unfreezing:** after `voteEnd`, *anyone* can `finalize`
  (defaulting to earliest-registration if the committee didn't vote) and then *anyone*
  can `resolveDispute`. Funds cannot be locked indefinitely by an ignored dispute.
- **Deferred:** a filing bond (slashed for a frivolous challenge) — the natural next
  guard, deferred with the staked-court money mechanics.

Timing: `challenge` must still fall within the escrow's challenge window
(`currentEpoch() < releaseEpoch`, per the H1 commit-time fix). The vote runs for
`VOTING_WINDOW` and may extend past that window; release is gated on `disputeId == 0`,
not on the window, so a dispute correctly holds the money until resolved.

---

## 6. Component changes

| Area | Change |
|---|---|
| `chain/contracts/CWEJury.sol` (new) | committee allowlist + dispute open/vote/finalize + fallback tally |
| `chain/contracts/interfaces/IJury.sol` (new) | escrow-facing seam (openDispute/isResolved/verdictOf) |
| `chain/contracts/CWEEscrow.sol` | async: challenge opens a dispute; new resolveDispute; release blocks while disputed; `arbiter`→`jury` |
| `chain/contracts/EarliestRegistrationArbiter.sol` / `IArbiter.sol` | kept; now the jury's fallback |
| `chain/script/Deploy.s.sol` | deploy jury (registry + arbiter + owner), pass it to escrow, `setEscrow` |
| `chain/test/CWEEscrow.t.sol` | update to the async flow; add resolveDispute / release-blocked-while-disputed / one-dispute tests |
| `chain/test/CWEJury.t.sol` (new) | allowlist, one-vote, window, majority, tie/zero→fallback, only-escrow-opens |
| `ops/demo/run_arbitration_demo.sh` (new) + `ops/demo/run_ownership_demo.sh` | new committee-overrides demo; update ownership demo to the async path (empty committee → earliest-reg default) |
| `ops/Makefile`, `.github/workflows/ci.yml` | `arbitration-demo` target + `arbitration-e2e` CI job |

---

## 7. Testing

**Contract (Foundry):**
- `CWEJury`: only owner adds/removes jurors; only a juror votes; one vote per juror;
  `forWork` must be a party; no vote after `voteEnd`; `finalize` only after `voteEnd`,
  once; majority elects the right work; a tie and a zero-vote dispute both fall back to
  earliest-registration; only the authorised escrow may `openDispute`.
- `CWEEscrow` (async): `challenge` opens a dispute and does not reassign; `release`
  reverts while a dispute is unresolved; `resolveDispute` reassigns on a
  challenger-verdict and keeps the incumbent otherwise; one dispute per escrow;
  reassignment conserves `liability` and preserves amount/window/content; a
  committee-silent dispute resolves to the earliest-registration default (the H1
  outcome) through the new path.

**End-to-end:** `make -C ops arbitration-demo` prints `✅ ARBITRATION DEMO PASSED`,
proving the committee overturns a first-registered fraudster; the updated
`ownership-demo`, `hub-demo`, `player-demo`, and the full workspace gate stay green.

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| Modifying audited escrow money code | Same careful review H1 got; conservation/no-lock/no-double-release invariants re-tested; the reassignment path is the H1 one, now verdict-gated. |
| Money frozen by an ignored dispute | Bounded window + permissionless `finalize`/`resolveDispute` + earliest-registration default. |
| Trusted committee is not trustless | Explicitly a stub (D6); the staked open court graduates the same seam later without touching the escrow. |
| Griefing via frivolous disputes | Content-correlation + one-per-escrow now; filing bond deferred. |
| Deploy cycle (escrow needs jury, jury needs escrow) | `CWEJury.setEscrow` once, owner-only, after both are deployed. |

---

## 9. Deliverable

`make -C ops arbitration-demo` prints `✅ ARBITRATION DEMO PASSED`; the updated H1
ownership demo and all other demos still pass; `cargo fmt`/`clippy`/`test` and
`forge test` stay green. Phase 2 becomes 3-of-3 done, and the judgment layer has a
working, swappable seam ready to graduate to a trustless court.
