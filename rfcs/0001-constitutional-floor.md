# RFC-0001: Constitutional Floor — Entrenched Rights, Exit, and Minority Protection

**Status:** Draft
**Target:** Phase 4 — Governance; amends `docs/governance_charter_v0.1.md`
**Related:** `docs/specs/governance_no-drm_clause.md`, `docs/tech_gov_spec_v0.1.md`, `CWEIdentity`, `CWEJury`, `cwe-dapr`

## Summary

CWE's stated aim is to make as many people as possible better off. Its governance
layer (1-person-1-vote General Assembly, simple-majority policy votes) is the right
tool for that, and it is also the one mechanism in the system that can harm a
minority: a large enough majority can always gain a little by taking a lot from a
few. This RFC adds a second layer beneath the vote — a **constitutional floor** —
that the maximising layer cannot reach. It has three parts:

1. **Entrenched rights** — a short, closed list of protections that no policy or
   technical vote can remove, amendable only by a multi-stage referendum.
2. **Individual exit** — the right of any member to leave with their data, keys,
   reputation and unsettled earnings, so the majority stays honest without a vote.
3. **Intensity-aware voting** — extending quadratic weighting from funding to any
   proposal that a protected group flags as affecting them, so a strongly held
   minority preference is not simply outnumbered.

Nothing here replaces the Assembly. It bounds it.

## Motivation

### The gap in v0.1

The charter already gestures at a floor: §7.1 says amendments "cannot violate core
principles: privacy, fairness, decentralization." But that line has no definition,
no enforcement path, and no test for what counts as a violation. A simple-majority
*policy* proposal (§4.1) is not even an amendment, so it is not covered at all. The
No-DRM clause shows what a real floor looks like — definitions, prohibited acts,
redress, a permanent-prohibition amendment rule — but it covers one topic.

Exit exists only at the collective level (§8: fork the whole DAO after six months
below 50% support). An individual creator or consumer who is on the losing side of
a vote has no defined way out that preserves what they have built.

Quadratic voting (§3.2, §4.1) applies only to funding requests. The questions most
likely to divide a majority from a minority — content policy, pricing bands,
what the jury may rule on — are decided by headcount.

### Why a floor, not just good intentions

The protections that already exist in CWE are mostly *in the code*: ZK proofs keep
consumption private, DAPR pays a niche creator from the people who actually
listened rather than from a popularity contest, `CWEIdentity` gates on a credential
rather than an owner allowlist, `CWEJury` decides disputes case by case. These are
real minority protections. But each lives in its own spec, and each is a Technical
Upgrade away from being changed by a 2/3 vote. The floor's job is to name them in
one place and raise the bar for touching them.

### Alternatives considered

- **Leave it to the Ethics Council.** Its recommendations are non-binding (§6.1).
  A floor that can be ignored is not a floor.
- **Supermajority for everything.** Makes ordinary governance sluggish and still
  lets a 67% bloc remove a 33% minority's protections.
- **A bill of rights with no mechanism.** This is roughly what §7.1 is now.
- **Pure fork-as-exit.** Forking is a nuclear option that requires coordination; it
  does not help one person.

## Design

### 1. Entrenched rights (Article E)

A new charter article listing rights that bind *all* proposal types — Policy,
Technical Upgrade, Funding, Emergency — not just amendments.

| # | Right | Where it already lives | What it forbids a vote from doing |
|---|-------|------------------------|-----------------------------------|
| E1 | **Consumption privacy.** No member can be identified from what they consume. | ZK usage proofs, discovery-layer privacy spec | Introducing any usage path that is not proof-based; linking a tier credential to a consumption record. |
| E2 | **Pay-by-use.** A creator is paid from the fees of the people who consumed their work, under DAPR, regardless of audience size or popularity. | `cwe-dapr`, DAPR aggregation spec | Redistributing from small audiences to large ones; minimum-audience thresholds for payout; popularity multipliers. |
| E3 | **Open protocol, open clients.** | No-DRM clause | (Already entrenched; this folds it under the same amendment rule.) |
| E4 | **Verified-human, one vote.** No wealth, stake, or token weighting in the Assembly. | Charter §3.2, `CWEIdentity` | Stake-weighted votes; delegated-vote purchase; credential tiers that carry extra votes. |
| E5 | **Due process.** Any adverse decision against a member — delisting, escrow seizure, credential revocation — must come from a jury or an issuer acting under published rules, with a right of appeal. | `CWEJury`, `CWEEscrow`, `CWEIdentity` issuer set | Assembly votes that delist or penalise a named member or class of members directly. |
| E6 | **Exit.** See §2 below. | New | Lock-in of any kind. |
| E7 | **Non-discrimination of lawful content.** No policy may exclude a creator or work by viewpoint, genre, language, or region, as opposed to by a rule applied to all. | Jury flow, legal interoperability guidelines | Genre bans; language requirements; "cultural fit" criteria. |

**Amendment rule for Article E.** Mirrors No-DRM §7.4 and tightens it:

1. Proposal co-signed by 5% of active members *and* by majority of at least two
   chambers (§3.1).
2. 60-day public review, including a published impact statement from the Ethics
   Council naming which members would lose what.
3. Referendum 1: 2/3 of votes cast *and* majority in each of the Consumer and
   Creator chambers separately (a concurrent-majority test, so one class cannot
   amend away the other's protection).
4. Cooling-off of 90 days, during which members may exercise exit (§2) on the
   pre-amendment terms.
5. Referendum 2: same thresholds. Only then does the change execute.

The two-referendum structure exists so that a transient majority cannot entrench
itself in one sitting, and so that exit on old terms is always available before
the floor moves.

**Justiciability.** Any member may challenge a proposal — before or after its vote
— as violating Article E. The challenge goes to the Arbitration Jury Pool under
the existing file → vote → finalize flow. A proposal under challenge cannot
execute until the jury finalizes. A jury that finds a violation voids the proposal;
this is appealable once to a second random jury. The jury's question is narrow:
*does this proposal remove or weaken a listed right?* — not whether the proposal
is wise.

### 2. Individual exit (Article X)

Any member may leave at any time. On exit the protocol guarantees:

- **Keys and credentials** remain the member's; the DAO cannot revoke a credential
  as a consequence of exit.
- **Unsettled earnings** (creator) and **unused tier balance** (consumer) settle at
  the next epoch on the terms in force at the moment of exit.
- **Reputation and provenance** — the creator's registration, consent chain and
  signed manifests — are exportable in the open manifest format so they can be
  re-registered elsewhere or in a fork.
- **Content** stays the creator's; storage nodes serve it until the creator's own
  withdrawal request, not the DAO's.
- **No penalty, fee, or waiting period** for exit beyond the epoch boundary.

Exit is deliberately cheap. A governance majority that knows the minority can walk
out, taking its audience's fees with it, has a standing reason to keep the minority
inside. This is the economic form of the floor; Article E is the legal form.

**Group exit.** When members holding ≥10% of either chamber exit within one 90-day
window citing a specific decision, that decision is automatically re-referred to
the Assembly with the Ethics Council's impact statement attached. This is a signal,
not a veto.

### 3. Intensity-aware voting (Article Q)

Extend quadratic weighting beyond funding:

- A proposal is **flagged** when either the Council of Creators or the Council of
  Consumers, by majority, declares that it affects a definable subgroup
  differently from the membership at large (e.g. a pricing-band change that
  mainly hits long-form creators; a jury-scope change that mainly affects a
  language community).
- A flagged proposal is decided by **quadratic vote** instead of simple majority:
  each member receives a fixed budget of voice credits per epoch; casting *n*
  votes on one proposal costs *n²* credits. Credits are per-identity (E4), not
  purchasable, and expire each epoch.
- Flagging cannot be used to block: the Council that flags must publish its
  reason, and the flag itself can be challenged to a jury as abusive.

Quadratic voting lets a minority that cares a great deal outweigh a majority that
cares a little, without giving anyone more than one identity's worth of credits.
It is the right tool for *contested* questions and the wrong tool for routine
ones, which is why it needs a flag.

### 4. Charter integration

- Insert Article E before current §7 (Amendment Process); renumber.
- Rewrite §7.1 to reference Article E instead of the free-text "core principles".
- Insert Article X as a new §2.4 under Membership (Rights).
- Insert Article Q as a new row in the §4.1 proposal table: **Flagged** — scope:
  any proposal flagged by a chamber — requirement: quadratic vote.
- Add to §9 Glossary: *Constitutional floor*, *Flagged proposal*, *Voice credit*,
  *Concurrent majority*.

### 5. Contract sketch (Phase 4)

```
CWEConstitution
  bytes32[] entrenched           // hashes of Article E rights text
  function challenge(proposalId, rightIndex) → juryCaseId   // opens CWEJury case
  function isBlocked(proposalId) → bool   // true while a challenge is open or upheld

CWEAssembly
  modifier notBlocked(proposalId)   // execution checks CWEConstitution
  enum VoteKind { Simple, Supermajority, Quadratic, Constitutional }
  function flag(proposalId, chamber, reasonHash)   // chamber-majority only; sets Quadratic
  function amendEntrenched(...)     // enforces the two-referendum, concurrent-majority flow

CWEExit
  function exit() // marks identity exited; schedules epoch settlement; emits ExitRequested
  function exportProvenance(creatorId) → manifest bundle (signed)
```

`CWEJury` is reused unchanged for challenges. `CWEIdentity` gains an `exited`
flag that issuers cannot set and cannot revoke.

## Drawbacks

- **Rigidity.** A floor can protect a mistake as easily as a right. The
  two-referendum path is slow by design; if a listed right turns out to be wrong,
  fixing it takes the better part of a year. Mitigation: keep the list short and
  phrase rights as outcomes, not mechanisms.
- **Jury load and jury politics.** Every contentious proposal will be challenged.
  The narrow question ("does it remove a listed right?") is meant to keep cases
  short, and the appeal is limited to one round.
- **Flag abuse.** A Council could flag everything to force quadratic votes.
  The published-reason and challenge rules address this; a pattern of abusive flags
  is itself jury-reviewable.
- **Cheap exit can be gamed.** A creator could exit and re-enter to reset
  something. Re-entry under the same credential should carry forward the same
  state; the identity is persistent even if membership is not.
- **Quadratic voting needs sybil resistance.** It only works because E4 and
  `CWEIdentity` hold. If identity verification weakens, Article Q should
  automatically fall back to simple majority.

## Adoption Plan

1. Discuss this RFC under the standard 7–30 day window.
2. If accepted in principle: charter v0.2 PR with Articles E, X, Q and the §7.1
   rewrite.
3. Phase 4 implementation order: `CWEExit` first (it is independent and the most
   immediately valuable), then `CWEConstitution` + challenge path on top of
   `CWEJury`, then quadratic voting in `CWEAssembly`.
4. Add `make -C ops constitution-demo`: a proposal that would weaken E2 is filed,
   challenged, voided by a jury; a creator exits with earnings and provenance
   intact; a flagged proposal is decided quadratically and a minority prevails.
5. Add a tech-gov spec section mirroring each article.

## Unresolved Questions

- Should Article E be **closed** (only these seven, ever) or **open to additions**
  by the same two-referendum process? Additions are safer than removals; the
  draft leaves it open to additions, closed to removals except by the full path.
- Is a **concurrent majority** across two chambers enough, or should Node
  Operators and Auditors also be able to block an amendment that affects them?
- Should the Ethics Council's impact statement become **binding** for Article E
  amendments only?
- Quadratic voting's **credit budget**: per epoch, per year, or per proposal? Per
  epoch is simplest but favours members who vote often.
- Does E7 need a carve-out for content that is lawful somewhere and unlawful
  elsewhere? The legal interoperability guidelines may already answer this.
- Should **group exit** (≥10% of a chamber) trigger more than re-referral — e.g.
  an automatic jury review of the cited decision?
