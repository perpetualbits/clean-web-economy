# H5 cycle 2 — credibility integrity: design

**Date:** 2026-07-26
**Status:** Approved in brainstorming (Sections A–C approved live).
**Governing specs:** `docs/specs/anti-fraud_and_bandwidth_receipt_protocol.md` (AFBRP),
`docs/specs/client-storage_handshake_specification.md`
**Predecessor:** `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md`
(cycle 1 — shipped 2026-07-25)
**Roadmap item:** H5 cycle 2 — *close the two deterrent gaps cycle 1 documented rather
than closed, and graduate `RATE(W)` off aggregator config.*

---

## 1. Purpose and scope

Cycle 1 made the bandwidth-credibility signal live: a credentialed `cwe-storage` node
serves real bytes, consumer and node co-sign receipts, and settlement converts verified
bytes into a per-(user, work) payout multiplier whose shortfall is burned. A usage claim
backed by **zero** bytes became a live strict loss.

Review then established two ways to satisfy that requirement without genuinely moving
content. Both were documented and deferred; this cycle closes them.

- **Fetch-and-discard.** Receipts bind no byte range, the node writes its ledger entry
  before the response body is flushed, and the anti-replay key
  `(node, session_nonce, chunk_nonce)` is entirely client-chosen. So a modified client
  can request a fragment, read none of it, and re-request the same window under a fresh
  chunk nonce to mint fresh non-colliding receipts. Demonstrated in review: **25 MiB of
  verified bytes extracted from an 8 MiB file**, no collusion, every verification gate
  passing.
- **Dust weight.** `expected_bytes = weight · RATE(W) / 1e12` floors to a single byte
  once the claimed weight is small enough, so one verified byte buys full credibility.
  Because the DAPR payout target is scale-invariant, a single-row claimant at full
  credibility still receives 100% of their own tier fee.

Neither is a money-extraction hole — H3's "extract ≤ pay-in" cap holds, so a claimant
only ever recovers their own fee — but both defeat the **strict-loss deterrent**, letting
wash-trading run at break-even to inflate reputation and discovery signals. This cycle
restores the deterrent.

It also graduates **`RATE(W)`** from an aggregator-maintained config table to an on-chain
registry field, because a hand-maintained rate table per registered work does not scale
past a demo and is itself a centralisation the roadmap wants removed.

### 1.1 In scope
- Receipts bind **content position** (`chunk_index`) instead of session position;
  credit dedup moves to `(consumer, work_id, chunk_index)`.
- The storage node credits bytes **as they are yielded downstream**, not on read.
- A **per-user, per-epoch evidence floor** (`MIN_EPOCH_BYTES`) below which a claimant's
  credibility is scaled down.
- `RATE(W)` becomes an on-chain `CWERegistry.bandwidthRate`, protocol-clamped, mirrored
  in the signed manifest; the `RATES` config file is removed.
- An adversarial `bandwidth-client --mode` for the demo, and settlement emitting
  machine-readable per-(user, work) verified bytes so demo assertions can target
  mechanisms rather than only payouts.
- `make bandwidth-demo` extended to six acts.

### 1.2 Explicitly deferred (unchanged from cycle 1 unless noted)
- **ZK bandwidth proof** (AFBRP §5) and **peer-diversity proof** (AFBRP §6.2) — cluster B,
  each its own cycle.
- **Full P2P storage swarm** — cluster C; arguably its own phase.
- **Node compliance & staking/slashing** (AFBRP §6.3) — cluster D.
- **Ephemeral-key unlinkability** (AFBRP §4.1, §10.2) — cheapest after B and D land.
- **`MIN_EPOCH_BYTES` and the rate clamps as governance parameters.** Aggregator config
  and contract constants this cycle (neither is beneficiary-controlled, so cycle 1's
  §4.1 config risk does not apply); Phase 4 governance owns them later.

Note that "proving the client *application* consumed the bytes" is **not** on this list.
§3.2 explains why it is neither achievable over HTTP nor meaningful for a bandwidth
measure — it is out of scope permanently, not deferred.

### 1.3 Decisions locked in brainstorming
| # | Decision | Choice |
|---|---|---|
| E1 | Cycle scope | Cluster A only — the two gaps plus `RATE(W)` graduation. B–E remain separate cycles |
| E2 | Light-user fee | Preserve DAPR's "fee follows consumption" split. Deter the fraudster with an **absolute evidence floor per user per epoch**, not by distorting per-row splits |
| E3 | Dedup mechanism | **Fixed-size chunk index**, deduped on `(consumer, work_id, chunk_index)`. Not byte-range union (fiddly, untestable to the needed standard); not a content-length cap (needs another on-chain field, does not require *distinct* bytes) |
| E4 | `RATE(W)` sourcing | **On-chain `CWERegistry.bandwidthRate`**, protocol-clamped, manifest mirrors — the pattern `pricePerMin` already uses |
| E5 | Contract migration | An **additive setter**, not an 8th `registerWork` argument, so no existing registration path or demo changes |
| E6 | Receipt compatibility | Breaking change, deliberately un-versioned — cycle 1 shipped days ago and nothing external consumes bundles |

---

## 2. Why the three changes compose

Each closes a different link, and any two without the third still leak:

| | Caps re-fetch amplification | Makes each byte cost bandwidth | Sets an absolute floor |
|---|---|---|---|
| §3.1 chunk-index dedup | ✅ | — | — |
| §3.2 credit only on full delivery | — | ✅ | — |
| §3.3 epoch floor | — | — | ✅ |

- **§3.1 + §3.2 without §3.3:** a dust-weight claim still passes on a sub-byte
  expectation.
- **§3.1 + §3.3 without §3.2:** an attacker mints the floor from bytes the server only
  read off disk, without anything crossing the wire.
- **§3.2 + §3.3 without §3.1:** re-fetching the same window under fresh nonces still
  amplifies without bound.

---

## 3. The evidence model

### 3.1 Receipts bind content position

```
receipt = {
  work_id:     bytes32,   // which work's content moved
  consumer:    address,   // binds bytes to THIS user
  node:        address,   // must hold a valid storage-node credential
  chunk_index: u64,       // WHICH fixed-size block of the content
  bytes:       u64,       // bytes actually delivered for that block
  epoch:       u64
}
+ sig_node(receipt) + sig_consumer(receipt)
```

`session_nonce` and `chunk_nonce` are **removed**, not retained. Both were client-chosen,
which is exactly what let a fresh session mint fresh credit; keeping them would imply a
protection they no longer provide.

**Credit dedup key: `(consumer, work_id, chunk_index)`.** Each distinct block of a work
counts once per consumer per epoch, so total credit for `(U, W)` is capped at the work's
content size no matter how many requests are issued.

This also corrects a **latent double-count in cycle 1**: under the old key, two different
nodes serving the same chunk to the same consumer counted twice, though it is the same
content. Keying on content position makes "this consumer received this piece of this
work" count exactly once, which is what should always have been true.

Replay protection is unaffected: `epoch` is inside the signed receipt, so a receipt
cannot be re-spent in a later epoch, and within an epoch the dedup key collides.

`CHUNK_SIZE` is a protocol constant shared by node, client and aggregator
(**131_072 bytes**, matching the client's existing default chunk length). The final chunk
of a work is short; its receipt carries its true `bytes`.

### 3.2 The node credits only what actually left the server

The `content` handler streams the chunk and credits it **all-or-nothing**: the ledger
records the chunk's byte count only once the entire chunk has been yielded to the
transport. A transfer abandoned part-way credits **zero** for that chunk. A client that
loses a chunk to a genuine network failure simply re-requests the same index — §3.1's
dedup makes that idempotent, so retrying is free and cannot double-count.

All-or-nothing is chosen over proportional crediting because it is deterministic, removes
partial-chunk accounting entirely, and reduces "was this chunk delivered?" to a boolean
the ledger can hold.

**What this does and does not establish, stated plainly so it is not misread later.** The
guarantee is that credited bytes *left the server and were accepted by the client's
transport*. It is **not** a claim that the client application consumed them. That
distinction matters less than it first appears, and the reasoning is worth recording:

A client that receives a chunk in full and then throws it away has still paid the
bandwidth — its own downstream and the node's upstream. Since bandwidth credibility
exists to measure exactly that expenditure, "receive-and-discard" is not a distinct
threat; it is an ordinary download that happens to be wasteful. What made cycle 1's
fetch-and-discard cheap was never the discarding. It was (a) crediting bytes the server
had only *read from disk*, so nothing had to cross the wire at all, and (b) unbounded
re-fetch amplification. §3.2 closes the first and §3.1 the second.

Consequently there is no residual "prove the application consumed it" gap to defer — that
property is neither achievable over HTTP nor meaningful for a bandwidth measure.

### 3.3 Per-user, per-epoch evidence floor

Over the **deduped** bytes of §3.1, per user `U`:

```
total_verified_bytes(U) = Σ_W verified_bytes(U, W)          // deduped by chunk
epoch_factor(U)         = clamp(total_verified_bytes(U) · 1e6 / MIN_EPOCH_BYTES, 0, 1e6)
credibility(U, W)       = row_credibility(U, W) · epoch_factor(U) / 1e6
```

`row_credibility` is cycle 1's ratio, unchanged. The factor is applied in settlement, so
**`cwe-dapr` needs no changes at all this cycle** and its bit-for-bit neutrality guarantee
is untouched.

- Honest light user (one 30-second play ≈ 480 KB) saturates the floor → no discount. This
  is decision E2: a light user is a legitimate subscriber, not a suspect.
- Dust claimant (negligible bytes) → factor ≈ 0 → burned.
- Because a Sybil only ever recovers its own fee, the floor turns wash-trading from
  break-even into a net loss of roughly `MIN_EPOCH_BYTES` of bandwidth per account per
  epoch. **State the cost precisely rather than as "any non-zero floor makes fraud a
  loss"** — the floor is deliberately cross-work (`Σ_W`, so a legitimately multi-work
  epoch is judged on its combined evidence), which means it is satisfied by 128 KiB of
  *any* content, not necessarily of the work being claimed. A dust claimant therefore
  needs one verified byte of the claimed work — which still requires a **credentialed**
  node to serve and co-sign it, and that gate is unaffected — plus 128 KiB of anything
  else per epoch. That is a real, recurring, per-account cost where previously there was
  none, and it is what makes farming uneconomic at scale; it is not an absolute barrier
  to a single determined claimant. Making the floor per-work would close that path but
  would penalise any honest user whose epoch spans several small works, which decision
  E2 rules out.

`MIN_EPOCH_BYTES` is aggregator config (`MIN_EPOCH_BYTES` env, default **131_072** — one
complete `CHUNK_SIZE`). Setting it to exactly one chunk is deliberate: with §3.2's
all-or-nothing crediting, the floor then reads as *"at least one complete chunk of a real
work genuinely crossed the wire to this user this epoch"*, which is the smallest evidence
statement that means anything. A floor below one chunk would be unreachable-by-fractions
and add nothing; a floor of several chunks would start touching genuinely light users,
which decision E2 rules out. Deterministic integer math throughout; no floating point.

**Accepted consequence, named so it is not discovered later as a surprise.** Because the
floor is *absolute*, a user whose entire epoch's consumption is a single work **smaller
than one chunk** cannot fully satisfy it, and is partially discounted despite having
consumed that work completely. At 128 kbps that threshold is roughly eight seconds of
audio, so the affected set is jingles, stings and sound effects rather than ordinary
content.

This is a deliberate trade, not an oversight. A relative floor — scaling the denominator
to the user's own expected bytes — would remove the penalty, but it also removes the
defence: a dust-weight fraudster's expectation is tiny for the same reason, so a relative
floor hands them a factor of 1.0 and reopens the gap this cycle exists to close. An
absolute floor is the only form that distinguishes "claimed little and received little"
from "claimed a full fee on nothing", and its cost falls on a narrow, identifiable band
of very short works. If that band matters commercially, the fix is a per-work minimum
expectation carried on-chain beside `bandwidthRate` — deferred, not designed here.

---

## 4. `RATE(W)` graduation

### 4.1 On-chain, additive

`CWERegistry.Work` gains `uint256 bandwidthRate`, with:

```solidity
function setBandwidthRate(bytes32 workId, uint256 rate) external;  // registrant only
function bandwidthRateOf(bytes32 workId) external view returns (uint256);
```

- Protocol clamp enforced on set, so a creator cannot make their own work trivially
  cheap to prove:

  ```solidity
  uint256 internal constant MIN_BANDWIDTH_RATE = 60_000;        // ≈ 8 kbps
  uint256 internal constant MAX_BANDWIDTH_RATE = 2_000_000_000; // ≈ 266 Mbps
  ```

  Reference points for calibration: 128 kbps audio ≈ 960_000 bytes/min; 4K video ≈
  187_500_000 bytes/min. `MIN` sits an order of magnitude below any real medium, so no
  legitimate work is excluded, while still refusing a rate low enough to make a work
  free to prove. `MAX` sits comfortably above 4K so a mistyped rate cannot silently
  burn an honest creator's earnings by demanding unmeetable evidence. The clamp is a
  guardrail, not the primary defence — §3.3's epoch floor is.
- Unset (`0`) means "no rate", which settlement already treats as **fail closed** — so
  the default is safe and every existing registration keeps working untouched.
- `registerWork`'s signature is **unchanged** (decision E5): no demo, script or test that
  registers a work needs migrating.
- The signed `WorkManifest` mirrors the value and MUST equal the on-chain one, exactly as
  `price_per_min` does; the hub validates the mirror on ingest.

### 4.2 The unit stays "bytes per 10¹² weight"

`RATE(W)` remains bytes per `RATE_SCALE` (10¹²) units of proven DAPR weight — **not** raw
bitrate. Converting to a true bytes-per-minute field was considered and rejected: the
aggregator can divide out the public per-work price, but *region multiplier* and
*repeat-play discount* are private per-user values it never sees. That is precisely why
cycle 1 (decision D5) derived expectation from proven weight in the first place.

**Every modelling distortion errs generous, and this must not be misread later.** A work
priced below full price, or a user replaying content, both yield an expectation *lower*
than the bytes genuinely moved, so honest users over-deliver and clear it comfortably.
The mechanism can under-demand evidence but never over-demand it — the correct direction
for something that burns money when unsatisfied. That generosity is exactly why §3.3's
epoch floor carries the real deterrent weight, and why `MIN_BANDWIDTH_RATE` matters.

### 4.3 Settlement reads the rate from the chain

One `bandwidthRateOf` call per distinct work in the epoch, cached per run alongside the
existing credential lookups. The `RATES` env file and its config plumbing are removed.

---

## 5. Components touched

| Component | Change |
|---|---|
| `libs/receipt` | `Receipt` gains `chunk_index`, loses `session_nonce`/`chunk_nonce`; `dedup_key` → `(consumer, work_id, chunk_index)`; `CHUNK_SIZE` constant |
| `services/storage` (node) | Ledger keyed by `(consumer, work_id, chunk_index)`; streaming response that credits on yield; `/content` takes `chunk_index` |
| `services/storage` (client) | Requests by chunk index; `--mode honest\|discard\|refetch` for the demo |
| `services/settlement` | Chunk dedup; `epoch_factor`; on-chain rate lookup replacing `RATES`; per-(user, work) verified-bytes output |
| `chain/` (`CWERegistry`) | `bandwidthRate` + clamped setter + getter |
| `services/discovery-hub` | Manifest mirrors `bandwidth_rate`; validated against chain on ingest |
| `ops/` | `bandwidth-demo` extended to six acts |
| `sims` (`cwe-dapr`) | **No change** |

---

## 6. Demo

`make bandwidth-demo`, one settled epoch, one user per act, **every work given a
bandwidth rate** so no act can pass or fail for a missing-rate reason.

| # | Act | Proves |
|---|---|---|
| 1 | Honest full download | Paid in full *(cycle 1 regression)* |
| 2 | Never downloaded | Requests receipts for every chunk without downloading any → the node holds no ledger entry and refuses all → **zero** verified bytes → strict loss |
| 3 | Re-fetch amplification | Fetches chunk 0 a hundred times → credited once → strict loss |
| 4 | Dust weight | Tiny claimed weight, negligible bytes → epoch floor drives credibility to 0 |
| 5 | Unset rate | A work whose `bandwidthRate` was never set → fails closed |
| 6 | Uncredentialed node | Receipts rejected *(cycle 1 regression)* |

**Where mid-transfer abandonment is proven, and why not here.** An "abandon the transfer
part-way" act would be a race rather than a test. The node writes 16 KiB pieces into a
socket whose buffers auto-tune to megabytes, so on loopback a whole 128 KiB chunk is
usually absorbed by the kernel before the client's abort is noticed; the stream then
completes legitimately and the chunk is credited. That is correct behaviour — the bytes did
leave the server, which is what §3.2 says the measure counts — but it makes any byte-exact
demo assertion timing-dependent. Mid-transfer abandonment is therefore proven
**deterministically at the unit level** by driving `DeliveryStream` directly with no
sockets (§7). Act 2 exercises the same trust property in its race-free form: a client
asking the node to attest content it never requested at all.

**Assertions must target mechanisms, not only payouts.** Cycle 1 asserted only on final
payouts, which is how a wrong-reason failure can masquerade as a right-reason pass.
Settlement therefore emits machine-readable per-(user, work) verified bytes alongside the
payout output, and each act asserts **both** its byte-level claim and its payout claim.
Act 2 in particular must assert that verified bytes are near zero *and* that the fee
burned — otherwise a zero payout caused by the epoch floor would look like proof that
credit-on-yield works.

---

## 7. Tests

- **`cwe-receipt`:** new receipt round-trip; `dedup_key` is `(consumer, work, chunk_index)`;
  two receipts differing only by `node` collide (the cycle-1 double-count, now fixed).
- **`cwe-storage`:** a stream driven to completion credits the chunk; a stream polled
  part-way and dropped credits **nothing** — both testable by driving the stream
  directly, no socket required. The node still refuses to sign for a chunk it never
  served, and a re-requested chunk after a failed transfer credits once, not twice.
- **`cwe-settlement`:** dedup across two nodes serving the same chunk; `epoch_factor`
  arithmetic including the clamp and the zero-bytes case; an honest light user clears the
  floor; a dust claimant does not; on-chain rate lookup, its cache, and fail-closed on
  unset.
- **`chain`:** `setBandwidthRate` registrant-only; the clamp rejects `59_999` and
  `2_000_000_001` and accepts both bounds exactly; getter returns 0 for an unset work.
  Assert `MIN_BANDWIDTH_RATE == 60_000` and `MAX_BANDWIDTH_RATE == 2_000_000_000`
  literally, so a silent change to either fails loudly.
- **Full gate stays green:** `cargo fmt/clippy/test`, `forge test`, and the nine
  `make …-demo`s.

**Mutation requirement (carried into the plan):** the final review must verify the demo
**fails** when each fix is reverted — not merely that it passes. Cycle 1's demo was found
sound only because a reviewer mutated the code; that check becomes a stated requirement
rather than a fortunate habit.

---

## 8. Roadmap / project-map sync (at merge)
Flip together: `ROADMAP.md`, `docs/roadmap.md`, `project-map.js` — H5 cycle 2 → done,
with the two gaps removed from the known-limitations lists and the socket-level delivery
bound added as the honest residual. Update the `cwe-receipt`/`cwe-storage`/settlement
nodes and `project.updated`.
