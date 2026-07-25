# H5 — Storage layer + real bandwidth receipts (cycle 1): design

**Date:** 2026-07-25
**Status:** Approved. Sections A–B approved live in brainstorming; Sections C–D and the
§9 open questions approved in review on 2026-07-25.
**Governing specs:** `docs/specs/anti-fraud_and_bandwidth_receipt_protocol.md` (AFBRP),
`docs/specs/client-storage_handshake_specification.md`,
`docs/specs/storage_node_policy_and_compliance_specification.md`
**Roadmap item:** H5 (hardening track) — *the P2P storage/swarm that supplies the real
bandwidth-credibility signal H3 wired as a neutral input, turning the anti-fraud
"strict loss" from demonstrated to live.*

---

## 1. Purpose and scope

H3 gave DAPR a **bandwidth-credibility multiplier** per work (`bandwidth_ppm`, ppm in
`[0, 1e6]`): below neutral it *discounts* a work's payout and burns the shortfall (the
anti-fraud "strict loss"). But settlement passes an **empty (neutral) map** today
(`services/settlement/src/chain.rs` — "Bandwidth credibility is not yet wired… neutral"),
and `make antifraud-demo` merely *hand-sets* a low value to show the discount works.
Nothing real drives it: a modified client can still claim it consumed a work it never
downloaded. H2 proved the usage *numbers* are honest; it did not prove real content moved.

This cycle delivers the **smallest honest slice that makes the knob live**: a minimal
storage node that actually serves content bytes, a consumer that downloads and
**co-signs a bandwidth receipt** with the node, and the aggregator verifying receipts to
compute a **real per-(user, work) bandwidth-credibility** that feeds DAPR — so a
"claimed but no bytes moved" fraud becomes a live strict loss.

### 1.1 In scope
- A minimal Rust `cwe-storage` node: serves content fragments over HTTP, counts bytes
  served, holds a CWEIdentity **storage-node credential**, co-signs receipts.
- A consumer-side receipt path: download fragments, co-sign receipts, accumulate a
  per-epoch **receipt bundle**.
- Aggregator verification: both signatures, node-credential validity (CWEIdentity),
  anti-replay, epoch binding; sum verified bytes per (user, work).
- A **per-row credibility** extension to `cwe_dapr::allocate_from_raw` (neutral default
  reproduces current payouts bit-for-bit).
- Settlement integration: a receipts-bundle input drives per-row credibility in event
  mode; neutral when absent (legacy demos unaffected).
- `make bandwidth-demo` + a CI job: real download pays full; no-bytes fraud is a strict
  loss; an uncredentialed node's receipts are rejected.

### 1.2 Explicitly deferred (each a named future cycle)
- **ZK bandwidth proof** (AFBRP §5): hide which works/peers and per-work bytes from the
  aggregator (the strong-privacy target; another circuit like H2's).
- **Peer-diversity proof** (AFBRP §6.2): prove ≥ D distinct peers in ZK.
- **Full P2P storage swarm** (the two storage specs): IPFS/torrent distribution,
  redundancy, availability/proof-of-storage, chunk expiry, node registration/discovery.
- **Node compliance & staking/slashing** (AFBRP §6.3, storage-node policy §8–§12).
- **Ephemeral-key unlinkability** (AFBRP §4.1, §10.2): per-transfer keys + non-linkable
  peer pseudonyms. Cycle 1 uses stable node identity keys (credentialed).
- **Dust-weight credibility floor** *(found in review, deliberately deferred — not a
  gap the team missed)*: `expected_bytes(U,W) = weight(U,W) × RATE(W) / 1e12` (§4)
  floors to a minimum of one byte (`.max(1)` in `services/settlement/src/receipts.rs`)
  once the claimed weight is small enough, so verifying **one** byte buys full
  (`1e6`) credibility for that row. Because the DAPR payout target is scale-invariant,
  a single-row claimant at full credibility still receives 100% of their own tier fee
  — so a "dust weight plus one verified byte" claim still routes a whole tier fee to
  a puppet work, with nothing burned, whereas an honest one-minute claim must move
  ~960 KB for the same credit. This is a *deterrent* gap, not a money-extraction hole:
  the claimant only ever recovers their own fee (the "extract ≤ pay-in" cap from H3
  still holds), and a credentialed node must still genuinely serve and co-sign that
  byte — it is not free to manufacture. Closing it needs either an absolute floor on
  expected bytes per claim, or weight-magnitude sensitivity in the payout target
  itself; both are spec-level decisions. **Deferred to H5 cycle 2** (agreed
  2026-07-25), where closing this is a scoping driver rather than a nice-to-have.
- **Unmetered fragment requests / receipts attest reads, not delivery** *(found in
  review, deliberately deferred — not a gap the team missed)*: the ledger
  `services/storage/src/main.rs` signs from records bytes **read off disk and handed
  to axum's response writer**, not bytes actually **delivered** to the consumer's
  socket, and the `Receipt` type binds no byte range at all (`bytes` but no
  `offset`/`len`). Because the anti-replay key is `(node, session_nonce,
  chunk_nonce)` — all three client-chosen — a modified consumer can issue
  `GET /content/...` requests, never read the response body, and still obtain a
  node-signed receipt for the full byte count under a fresh chunk nonce each time.
  Review testing extracted 25 MiB of "verified bytes" from an 8 MiB file this way,
  with no collusion required: the client owns its own key, the epoch matches, and
  the node is genuinely credentialed, so every existing verification gate passes.
  This is **strictly cheaper than the dust-weight gap above**: the dust-weight gap
  only pays off for a dust-sized claimed weight, whereas this one lets a claim of
  ANY size reach full credibility, for the cost of re-requesting (and discarding)
  a small fragment repeatedly rather than genuinely moving bytes. It is still a
  *deterrent* gap, not a money-extraction one, for the same reason as above: the
  DAPR payout target is scale-invariant (H3), so an inflated count still only ever
  recovers the claimant's own tier fee, and the target work's content must still be
  genuinely hosted on a credentialed node. Closing it needs binding `offset`/`len`
  into the receipt and deduping by byte RANGE per (user, work) rather than by chunk
  nonce, plus writing the ledger entry only after the response body has actually
  been written — a spec-level change, not attempted here. **Deferred to H5 cycle 2**
  (agreed 2026-07-25), where it is the highest-priority item of the two gaps, being
  the cheaper attack.
- **Whole-file reads in `fragment`** (`services/storage/src/lib.rs`): regardless of
  the requested `offset`/`len` window, `fragment` loads the ENTIRE content file into
  memory via `std::fs::read` before slicing out the requested window, so concurrent
  requests against a large file are a memory-exhaustion vector. Must be fixed before
  any node is publicly reachable; harmless for the demo's small fixtures.
- **Credential lookups precede signature verification** (`services/settlement/src/chain.rs`,
  `run_events`): each distinct node address in a receipt bundle has its on-chain
  `isValid` credential resolved BEFORE any receipt signature is checked, and an RPC
  error there aborts settlement outright. Harmless while the bundle is an
  operator-supplied local file; becomes a live denial-of-service the moment
  consumers submit bundles directly, since an attacker can name an address whose
  `isValid` call reverts or times out. Fix is to verify signatures first and only
  resolve credentials for nodes whose receipts survive that check.

### 1.3 Decisions locked in brainstorming
| # | Decision | Choice |
|---|---|---|
| D1 | Cycle ambition | Smallest honest slice: live receipts → real per-(user,work) credibility feeding DAPR. Non-ZK |
| D2 | Credibility granularity | **Per-(user, work)** (small `allocate_from_raw` extension), not per-work — catches puppet works AND per-user padding, no collateral damage |
| D3 | Trust anchor | Storage nodes hold a **CWEIdentity "storage-node" credential** (reuse H6's issuer set); aggregator counts only credentialed nodes. Staking/ephemeral-keys deferred |
| D4 | Privacy boundary | Integrity-first: aggregator sees per-(user,work) bytes + work_id (like H2 cycle-1 revealed work_id). ZK hiding deferred |
| D5 | Expected-bytes basis | `weight × RATE(W)` — because H2 made minutes/plays private, "expected" derives from the proven weight, not raw minutes |
| D6 | `RATE(W)` sourcing | **Aggregator deploy config** (a rates map alongside `DEPLOYMENTS`). Must not be settable by the payout beneficiary — see §4.1. Manifest/registry sourcing (with a protocol floor) is the deferred graduation |
| D7 | `bandwidth-demo` usage path | **Event mode** (real Groth16 proofs, reusing the H2 `zk-demo` machinery), accepting the ~5 s proving cost, so the demo exercises the live proven-weight path |
| D8 | Consumer receipt key | The consumer's **wallet key** — event-mode rows are keyed by the submitter address (`chain.rs`), so wallet-signed receipts join directly to the right DAPR row. No separate key |
| D9 | `cwe-storage` transport | Plain HTTP. The real P2P swarm is deferred |

---

## 2. Architecture

```
CONSUMER (client)                    STORAGE NODE (cwe-storage)          AGGREGATOR (settlement)
─────────────────                    ──────────────────────────          ───────────────────────
 request fragments of work W ──────► serve real bytes; count them
                            ◄──────  per-chunk receipt (NODE signs)
 both sign receipt tuple:
   (work_id, consumer_addr, node_addr,
    bytes, epoch, session_nonce, chunk_nonce)
        │
 accumulate per-epoch receipt bundle
 submit receipts.json ──────────────────────────────────────────────►  verify BOTH sigs
                                                                        + node holds a valid CWEIdentity
                                                                          storage-node credential
                                                                        + anti-replay (epoch/nonces)
                                                                        Σ verified bytes per (U,W)
                                                                        credibility_ppm(U,W) =
                                                                          clamp(bytes·1e6 /
                                                                            (weight·RATE(W)), 0, 1e6)
                                                                        → per-row DAPR discount
                                                                          (shortfall burned)
```

**Why mutual signatures:** the node won't sign bytes it didn't serve; the aggregator
won't count a receipt missing the consumer's signature. Neither party can fabricate a
receipt alone. The **CWEIdentity storage-node credential** stops a fraudster spinning up
their own "node" to co-sign fakes with a colluding client — only credentialed nodes count.

### 2.1 Components touched / added
| Component | Change |
|---|---|
| `services/storage` (new crate `cwe-storage`) | HTTP fragment server; node keypair + storage-node credential; byte accounting; per-chunk receipt signing |
| `libs/receipt` (new crate `cwe-receipt`) | The `Receipt` type, its EIP-191 signing/verification, bundle (de)serialization, anti-replay dedup key — portable, shared by node + consumer + settlement |
| `clients/player-plugin` (or a demo client) | Download fragments, co-sign receipts, write the receipt bundle |
| `sims` (`cwe_dapr`) | `allocate_from_raw` gains per-row credibility (extract a per-row-credibility core; keep per-work wrapper) |
| `services/settlement` | Read + verify the receipts bundle (event mode); compute per-row credibility; pass to the extended DAPR; `RATE(W)` sourced from the manifest/registry/config |
| `chain/` (CWEIdentity) | A `storage-node` credential type/topic issued to nodes at deploy; the aggregator checks `isValid` |
| `ops/` | `make bandwidth-demo` + `bandwidth-e2e` CI job |

---

## 3. Receipt format & anti-replay

```
receipt = {
  work_id:        bytes32,   // which work's content moved (revealed this cycle)
  consumer_addr:  address,   // binds bytes to THIS user (per-(user,work) credibility)
  node_addr:      address,   // storage node; must hold a valid storage-node credential
  bytes:          u64,       // bytes served, agreed by both parties
  epoch:          u64,
  session_nonce:  bytes32,   // per consumer↔node session
  chunk_nonce:    u64        // per chunk-group within a session
}
+ sig_node(receipt)          // EIP-191 over the canonical encoding (reuse H1 machinery)
+ sig_consumer(receipt)
```

- No `Com(...)` hiding — cycle 1 is integrity-first; the aggregator sees per-(user,work)
  bytes. The commitment/hiding is part of the deferred ZK layer.
- **Anti-replay** (aggregator-enforced): reject `epoch ≠ settlement epoch`; dedupe on
  `(node_addr, session_nonce, chunk_nonce)` — repeats dropped. Kills replay of old
  receipts. A node's `bytes` is capped at the credibility clamp (over-serving buys no
  extra credit).
- **Eligibility:** the aggregator recovers `node_addr` from `sig_node` and checks it holds
  a valid, non-revoked storage-node credential via `CWEIdentity.isValid`. Uncredentialed
  → the receipt is not counted.

---

## 4. Credibility math (per-(user, work))

```
expected_bytes(U,W)  = weight(U,W) × RATE(W)          // RATE(W): public per-work
                                                        //   "bytes per unit weight" constant
credibility_ppm(U,W) = clamp( verified_bytes(U,W) × 1e6 / expected_bytes(U,W), 0, 1e6 )
                       // expected == 0 → neutral 1e6 (no expectation ⇒ no discount)
```

- Honest: `verified_bytes ≈ weight × RATE` → ratio ≈ 1.0 → full credit.
- Puppet work, no downloads: `verified_bytes = 0` → 0 → **strict loss** (burned).
- Padder inflating their own weight on a real work: bytes don't scale with the padded
  weight → ratio < 1 → discounted, and only *their* row.
- Deterministic integer math (`mul_div`, saturating clamp); no floating point.
- `RATE(W)` is a public per-work constant. It is not privacy-sensitive (it's a property
  of the content, not the user), but it **is** security-sensitive — see §4.1.

### 4.1 `RATE(W)` is security-critical (fail closed, not open)

The `expected == 0 → neutral 1e6` rule above is a **fail-open**: whoever sets `RATE(W)`
can switch off the discount for that work entirely by setting it to `0`. If `RATE` came
from the work's own signed manifest, a puppet-work fraudster — who *is* the creator of
their puppet work — would publish `RATE = 0` and sail through §7.1 scenario 2 at full
credit, silently defeating the cycle's whole point.

Two rules follow, and both are requirements, not nice-to-haves:

1. **`RATE(W)` comes from the aggregator's deploy config** (D6) — a source the payout
   beneficiary does not control. It is never read from the receipts bundle (which the
   consumer writes) or from creator-signed data this cycle.
2. **A missing or zero `RATE(W)` fails closed:** a work with no configured rate, or a
   configured rate of `0`, is treated as *unknown expectation* → its rows get
   **credibility `0`** (strict loss), not neutral `1e6`. The neutral-on-`expected == 0`
   path in §4 applies only to a genuinely zero *weight* row, where there is nothing to
   discount. Settlement logs the work id when it drops a row this way, so a
   misconfiguration is loud rather than a silent free pass.

When `RATE` graduates to a manifest or registry field in a later cycle, it needs a
protocol-level floor (or a chain-anchored value) for the same reason.

---

## 5. DAPR extension (per-row credibility)

Extend `cwe_dapr` without changing existing per-work callers:
- Extract the core as `allocate_from_raw_with_row_credibility(tier_fees, rows: &[RawRow],
  credibility_ppm: &[u64])` — `credibility_ppm[i]` is row `i`'s multiplier in `[0, 1e6]`,
  applied in the **cred** path: `cred_i = mul_div(raw_i, credibility_ppm[i], 1e6)`, while
  the denominator stays the **bandwidth-free** `rw_u = Σ raw` (so a shortfall is *burned*,
  not redistributed — H3's strict-loss property preserved).
- Keep `allocate_from_raw(tier_fees, rows, bandwidth_ppm: &BTreeMap<WorkId,u64>)` as a
  wrapper that maps each row's work → its per-work credibility and delegates to the core.
- **Neutral credibility (`1e6`) reproduces current payouts bit-for-bit** — the same
  property H3 itself guarantees; all existing DAPR tests stay green unchanged.

---

## 6. Settlement integration

- Config: optional `RECEIPTS` env → a receipts-bundle path (mirrors the `DISCLOSURE`
  pattern). Add the `CWEIdentity` address to `Deployments` (needed for `isValid`).
- **Event mode** (`services/settlement/src/chain.rs`): after decoding proven rows, if a
  receipts bundle is present, verify each receipt (both sigs, node credential, anti-replay,
  epoch), sum verified bytes per (user, work), compute per-row `credibility_ppm` (§4), and
  call `allocate_from_raw_with_row_credibility`. If absent → neutral (current behavior).
- **Disclosure mode / legacy demos:** no receipts bundle → neutral bandwidth, unchanged.
- `RATE(W)` is read from an aggregator-side rates config (optional `RATES` env → a
  `{work_id: bytes_per_weight}` map, loaded next to `DEPLOYMENTS`). Never from the
  receipts bundle. A work missing from the map, or mapped to `0`, fails closed per §4.1.
  Absent the whole receipts bundle, bandwidth stays neutral and the rates map is unused —
  so the legacy demos are untouched.

---

## 7. Demo & tests

### 7.1 `make bandwidth-demo` (CI job `bandwidth-e2e`)
Self-contained Anvil + a running `cwe-storage` node. Three points:
1. **Honest consumer** downloads real bytes of work W, co-signs receipts, submits usage
   (event mode, real proof via the H2 path) + the receipts bundle → credibility ≈ 1.0 →
   full payout, fees conserved.
2. **Puppet-work fraud**: a second identity claims heavy usage of work F but downloads no
   bytes (empty receipts) → credibility 0 → **strict loss** (its fee is burned/unallocated,
   creator F earns ~0). Assert honest earns full and the fraudster earns ~0.
3. **Uncredentialed node**: receipts co-signed by a node without a valid storage-node
   credential are rejected by the aggregator → still a strict loss. Assert rejection.

### 7.2 Unit / integration tests
- `cwe-receipt`: sign/verify round-trip; a tampered field or missing signature fails;
  anti-replay dedup key rejects a reused `(node, session, chunk)`.
- `cwe-storage`: serves the requested bytes; byte count matches; refuses to sign a receipt
  for bytes it didn't serve.
- `cwe_dapr`: `allocate_from_raw_with_row_credibility` — neutral row credibility equals
  `allocate_from_raw` bit-for-bit; a zero-credibility row burns its share (strict loss);
  a fractional credibility discounts proportionally.
- Settlement: receipts drive per-row credibility; an uncredentialed/invalid/replayed
  receipt is dropped; `RATE`/expected-bytes edge cases — a zero-*weight* row is neutral,
  but a work with a missing or zero configured `RATE` **fails closed to credibility 0**
  (§4.1), and `RATE` is never taken from the bundle.

### 7.3 Full gate stays green
`cargo fmt/clippy/test`, `forge test`, and the now-**nine** `make …-demo`s (adding
`bandwidth-demo`).

---

## 8. Roadmap / project-map sync (at merge)
Flip together: `ROADMAP.md` + `docs/roadmap.md` H5 → done-for-cycle-1 with the deferred
sub-items listed (ZK bandwidth proof, peer-diversity, full P2P swarm, node
compliance/staking, ephemeral-key unlinkability); update the "What is real vs stubbed"
*Anti-fraud* / bandwidth rows; `project-map.js` H5 roadmap entry → done + a
`cwe-storage`/`cwe-receipt` node; `project.updated` = merge date.

---

## 9. Review outcome (2026-07-25)

All four questions raised for review are resolved; the decisions are recorded as D6–D9
in §1.3 and folded into §§4, 4.1, 6 and 7.

| Question | Resolution |
|---|---|
| `RATE(W)` sourcing | Aggregator deploy config (D6). Manifest/registry sourcing deferred, and only with a protocol floor — see §4.1 |
| `bandwidth-demo` usage path | Event mode, real proofs (D7) |
| Consumer receipt key | The wallet key; verified to match how event mode keys rows (D8) |
| `cwe-storage` transport | Plain HTTP (D9) |

The review also found a fail-open in the original §4 credibility math — a zero `RATE(W)`
neutralises the discount, which a puppet-work creator could have exploited had `RATE`
come from creator-signed data. §4.1 now makes the rate source beneficiary-independent
and makes a missing/zero rate fail closed.
