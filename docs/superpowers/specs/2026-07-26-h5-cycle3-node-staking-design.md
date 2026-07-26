# H5 cycle 3 — node staking and objective fraud proofs: design

**Date:** 2026-07-26
**Status:** Approved in brainstorming (Sections A–C approved live).
**Governing specs:** `docs/specs/anti-fraud_and_bandwidth_receipt_protocol.md` (AFBRP §6.3),
`docs/specs/storage_node_policy_and_compliance_specification.md` §8–§12
**Predecessors:** `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md` (cycle 1),
`docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md` (cycle 2)
**Roadmap item:** H5 cycle 3 — *node compliance & staking/slashing, the boundary cycle 2
identified for a colluding credentialed node.*

---

## 1. Purpose and scope

Cycle 2 closed the two ways a **client acting alone** could fake bandwidth evidence. Its
final review then established what remains: the cheapest attack on the system is a
**colluding credentialed node**, and the CWEIdentity storage-node credential is the only
thing currently gating one. The sole penalty is revocation by an issuer.

This cycle adds an economic cost to operating a node, and one slashing condition that is
objectively provable by anyone.

### 1.1 What staking can and cannot buy — read this before the rest

Being precise here matters more than usual, because the obvious framing ("staking deters
fraud") is not true of the main threat:

- **Out-of-range fabrication is objectively provable.** A receipt naming a `chunk_index`
  beyond a work's real extent is self-evidently invalid — given the content length
  on-chain, it is arithmetic plus the node's own signature. Anyone can submit it; no jury,
  no interaction, no discretion. **This cycle implements exactly this.**
- **A node that genuinely hosts the content and over-attests to a colluding consumer is
  undetectable.** Both parties are lying, both benefit, and bandwidth delivery leaves no
  durable artefact to contradict them. No slashing condition in this architecture catches
  it. **This cycle does not claim to.**

Consequently staking's primary value here is a **capital cost to operate a node at all**,
which makes node farms expensive whether or not anyone is ever slashed. The fraud proof is
a second, narrower benefit: it makes a rogue node defrauding an *honest* creator's work
costly.

Note the asymmetry that makes the fraud proof meaningful despite the collusion gap: it
catches a node lying about **someone else's** work. When creator and node are both
dishonest the creator can simply declare a large content length, and we are back in the
undetectable case above — which is already documented, not a new hole.

### 1.2 In scope
- A new `CWEStake` contract: bond, request-unbond, withdraw-after-delay, slash, `isBonded`.
- Receipt signing moves from RFC 8785 canonical JSON to a `keccak256(abi.encode(...))`
  digest, so a receipt is verifiable **on-chain**.
- `CWERegistry.contentLength` beside `bandwidthRate`, registrant-set, mirrored in the
  signed manifest.
- The storage node **refuses** an out-of-range chunk index (404, no ledger entry, no
  signature) — a prerequisite, see §3.3.
- Settlement gates receipts on credential **and** `isBonded`, and verifies signatures
  **before** any chain lookup.
- `make staking-demo` (the tenth demo) + a CI job.
- Both carry-over findings from cycle 2's final review (§5).

### 1.3 Explicitly deferred
- **Retrievability audits** — challenging a node to re-serve a chunk it attested. A real
  and distinct case (a node that never held the content), but it adds a challenge
  protocol, timing windows, and a liveness burden on honest operators.
- **Jury adjudication of bandwidth disputes.** Deliberately rejected rather than deferred:
  the existing `CWEJury` works for ownership because registration order and fingerprints
  are durable and inspectable. Whether bytes moved three weeks ago is not inspectable by
  anyone, so a jury would produce the appearance of adjudication without the substance.
- **Detecting a colluding node that hosts real content** (§1.1) — believed undetectable
  in this architecture, not merely unimplemented.
- **Permissionless admission by stake alone** — the eventual graduation once slashing has
  been exercised in practice; see D3.
- ZK bandwidth proof, peer diversity, full P2P swarm, ephemeral keys — H5 cycle 4+.

### 1.4 Decisions locked in brainstorming
| # | Decision | Choice |
|---|---|---|
| D1 | Cycle scope | Node staking only. ZK/peer-diversity/swarm/ephemeral-keys stay separate cycles |
| D2 | What staking buys | **Sybil barrier first**, one objective fraud proof second. No general slashing regime, and no claim to catch full collusion |
| D3 | Node admission | Credential **AND** active bond. The credential stays H6's identity/revocation lever; stake adds economic cost. Permissionless-by-stake deferred until slashing is proven in practice |
| D4 | Contract shape | A new `CWEStake`. Not folded into `CWEIdentity` (shared by creators and jurors) or `CWERegistry` (about works, not nodes) |
| D5 | Signing preimage | `keccak256(abi.encode(...))`, EIP-191-signed — the `consentDigest` pattern already in `CWERegistry`. Canonical JSON cannot be verified on-chain affordably |
| D6 | Unbonding delay | **Derived, not chosen**: fraud only becomes visible at settlement, so the delay must exceed one epoch plus settlement. Two epochs |

---

## 2. `CWEStake`

```solidity
function bond() external payable;
function requestUnbond() external;
function withdraw() external;
function slash(
    bytes32 workId, address consumer, address node,
    uint64 chunkIndex, uint64 numBytes, uint64 epoch,
    bytes calldata nodeSig
) external;
function isBonded(address node) external view returns (bool);
```

```solidity
uint256 public constant MIN_STAKE   = 10 ether;  // devnet-calibrated; Phase 4 governance owns it
uint256 public constant SLASH_BPS   = 1_000;     // 10% of the current bond per proven receipt
uint256 public constant BOUNTY_BPS  = 5_000;     // 50% OF THE SLASHED AMOUNT to the submitter
uint256 public constant UNBOND_DELAY = 2 * 30 days;  // == 2 * CWEConsumption.EPOCH_LENGTH
```

- **`UNBOND_DELAY` is derived, not chosen for feel.** A receipt is only exposed to
  scrutiny once its epoch settles, so a delay shorter than one epoch plus settlement would
  make slash-and-run free. Two epochs. The code must state the derivation, and a test pins
  it equal to `2 * CWEConsumption.EPOCH_LENGTH` so the two cannot drift apart.
- `requestUnbond` drops `isBonded` to **false immediately** — a node must not keep serving
  while its capital is already leaving. Slashing remains possible throughout the window.
- **Partial slashing, not total.** Each proven receipt takes `SLASH_BPS` of the *current*
  bond. Repeat offences compound and eventually drop the bond below `MIN_STAKE`, at which
  point `isBonded` goes false and the node de-gates itself — no separate ejection path
  needed. Total slashing on a single receipt was rejected as disproportionate for what may
  be an operator bug, and it would also destroy the evidence trail after one offence.
- **Bounty:** `BOUNTY_BPS` of the slashed amount to the submitter, remainder burned. The
  bounty exists because an objective proof nobody submits deters nothing. A generous share
  is safe *because* the proof is objective — a valid one cannot be manufactured, so there
  is nothing to game.

---

## 3. The fraud proof

### 3.1 Receipts become on-chain-verifiable

The signed message changes from RFC 8785 canonical JSON to:

```
digest = keccak256(abi.encode(work_id, consumer, node, chunk_index, bytes, epoch))
```

EIP-191-signed, exactly as `CWERegistry.consentDigest` already is. The `Receipt` struct
keeps its fields; only the preimage changes, and `serde_jcs` leaves the receipt signing
path. Bundles stay JSON for transport.

### 3.2 Verification

`slash` recovers the signer from `nodeSig` over the recomputed digest, reads
`CWERegistry.contentLength(workId)`, and slashes if

```
uint256(chunkIndex) * CHUNK_SIZE >= contentLength
```

Both operands are widened to `uint256` before multiplying: `chunkIndex` is a `uint64` and
`CHUNK_SIZE` is ~2¹⁷, so a `uint64` multiply would overflow and could wrap an out-of-range
index into an apparently valid one.

Only the **node's** signature is required: the consumer's counter-signature says nothing
about whether the node lied, and no one can forge a node's signature to frame it. Each
proven digest is recorded so the same proof cannot be replayed.

**An unset `contentLength` must REJECT the proof, not accept it.** `contentLength`
defaults to `0` for any work whose registrant never set it, and with `0` the comparison
above is true for *every* index — so a naive implementation would make **every receipt for
every unrated work slashable**, letting anyone drain honest nodes. This is the same shape
as the `RATE(W) == 0` fail-open caught in cycle 1, in the opposite and more dangerous
direction: there the failure was crediting fraud, here it would be destroying honest
capital. `slash` must revert when `contentLength == 0`, and a test must pin it.

### 3.3 An honest node must not be slashable — a prerequisite, not a nicety

Today `fragment_for_chunk` returns an empty slice for an index past the end of the
content, and an empty `DeliveryStream` fires its completion callback on the first poll —
so the node records a 0-byte entry and **will sign a receipt for an out-of-range index**.
That is harmless now and was recorded as a minor finding in cycle 2.

The moment out-of-range receipts are slashable it becomes a mechanism for **slashing
honest nodes**: a client requests chunk 9999 of a small work, the node dutifully signs,
and anyone takes its stake.

So the node must **return 404 for an out-of-range chunk index, record nothing, and sign
nothing**. This is load-bearing for the whole cycle, and §6's Act 5 exists to prove it.

### 3.4 `contentLength`

Joins `bandwidthRate` on `CWERegistry.Work`: registrant-set, mirrored in the signed
manifest and validated on ingest — the pattern cycle 2 established. `CHUNK_SIZE` becomes a
Solidity constant mirroring `cwe_receipt::CHUNK_SIZE`, pinned equal by tests on both sides.

---

## 4. The cross-language digest is this cycle's biggest risk

Rust computes the digest; Solidity recomputes it. If they disagree, `ecrecover` yields
some other address and every proof is rejected — **the feature is silently dead**. It
fails closed (no wrongful slashing), which is the safe direction, but nothing else would
surface it.

Mitigation, as cycle 1 did for the credential keccak: a fixed receipt with known field
values, its digest **pinned as a literal in both a Rust test and a Solidity test**. A
divergence then fails at build time rather than in production.

---

## 5. Settlement integration

- The node check becomes credential **AND** `isBonded`, cached per node per run.
- **Signatures are verified before any chain lookup.** Cycle 2's review flagged that the
  credential loop runs before signature verification, so a bundle of fabricated node
  addresses forces one RPC each; adding the bond check would double that. Verifying first
  and resolving only surviving nodes closes it.
- `fragment_for_chunk`'s whole-file read (the other cycle-2 carry-over) is fixed in
  passing, since §3.3 rewrites that function anyway.

---

## 6. Demo — `make staking-demo` (tenth demo)

A separate demo rather than extending `bandwidth-demo`, which would otherwise reach ten
acts. Each act uses its own node and its own work, for the ledger-isolation reason
recorded in cycle 2's plan.

| # | Act | Proves |
|---|---|---|
| 1 | Bonded + credentialed | Receipts count — baseline |
| 2 | Credentialed but **unbonded** | Receipts dropped; the stake genuinely gates |
| 3 | Unbond requested | Stops counting **immediately**, before withdrawal |
| 4 | Out-of-range receipt submitted | Node slashed, submitter paid the bounty, remainder burned |
| 5 | Honest node asked for an out-of-range chunk | **404, no signature** — an honest node cannot be framed into slashability |
| 6 | Same proof submitted twice | Second rejected |

**Act 5 is the one that matters most.** It is the safety property that makes slashing
tolerable; without it the feature is a weapon pointed at honest operators.

Assertions must target mechanisms as well as outcomes, per cycle 2 §6 — read balances and
`isBonded` directly, not only the final payout.

---

## 7. Tests

- **`CWEStake`:** bond/unbond/withdraw timing and `isBonded` transitions; slash accepted
  for an out-of-range index; rejected for an in-range one; rejected for a bad signature;
  rejected on replay; **rejected when `contentLength == 0`** (§3.2 — the dangerous
  fail-open); the bounty/burn split; slashing still possible during the unbonding window;
  repeated slashes eventually drop the bond below `MIN_STAKE` and flip `isBonded` false;
  `UNBOND_DELAY == 2 * EPOCH_LENGTH` and the three BPS constants pinned literally.
- **Cross-language digest:** the same fixed receipt pinned to the same literal digest in
  Rust and in Solidity (§4).
- **`CHUNK_SIZE`** pinned equal in Solidity and `libs/receipt`.
- **`cwe-storage`:** an out-of-range chunk index yields 404, leaves no ledger entry, and
  yields no receipt; an in-range one is unaffected.
- **`cwe-settlement`:** an unbonded node's receipts are dropped even with a valid
  credential; signatures are verified before any chain lookup.
- **Full gate stays green**, and the demo count becomes **ten**.

**Mutation requirement, carried forward:** the final review must verify the demo *fails*
when each fix is individually reverted — the bond gate, the 404 guard, and the replay
guard. Cycle 2 found two of three mutation checks did not bite; that is expected practice
now, not a bonus.

---

## 8. Roadmap / project-map sync (at merge)
Flip `ROADMAP.md`, `docs/roadmap.md` and `project-map.js` together: H5 cycle 3 → done,
`CWEStake` added, the demo count to ten, and cycle 4 named with what remains (ZK bandwidth
proof, peer diversity, P2P swarm, ephemeral keys, retrievability audits). Record §1.1's
undetectable-collusion boundary as a standing limitation, not a deferral.
