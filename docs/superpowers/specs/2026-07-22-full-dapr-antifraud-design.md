<!-- File: docs/superpowers/specs/2026-07-22-full-dapr-antifraud-design.md -->

# Full DAPR + Anti-Fraud (H3) — MVP Design

**Status date:** 2026-07-22
**Cycle:** H3 — hardening track (graduate the flat payout math to the real DAPR model)
**Depends on:** Phase 1 (`cwe-dapr`, `cwe-wallet-zk`, `services/settlement`), the client
session store shared by the browser extension and the player agent.
**Governing specs:** `docs/specs/DAPR_usage_aggregation_protocol.md`,
`docs/specs/anti-fraud_and_bandwidth_receipt_protocol.md`.

---

## 1. Objective and guiding principle

Replace the flat payout split (`credit = fee · minutes·price·region / user_total`)
with the real DAPR weighting: **diminishing returns on repeat plays**, a
**bandwidth-credibility** discount, and a **diversity/reputation** signal for
discovery — all on the **user-centric** economic model (each subscriber's fee is
divided only among the creators they consumed).

**Guiding principle — fairness first, honest anti-fraud.** CWE keeps the
user-centric model (fairer to independent creators than global-pool pro-rata) and
layers anti-fraud onto it. The honest properties H3 delivers:

1. **Structural fraud cap (always on):** because a fee only pays out to works its
   payer consumed, an attacker can never extract more payout than they paid in
   subscription fees. Self-botting is at best break-even (cycling your own money
   minus gas) — never profitable.
2. **Bandwidth credibility → strict loss (mechanism built now, data later):** a
   play not backed by real data transfer is discounted, and the discounted fee goes
   to the unallocated pool — so a *fake* play pays out *less* than the fee behind it.
   The real cryptographic bandwidth receipts arrive with the storage layer (H5); H3
   builds the credibility **input** and neutral default, so today's numbers are
   unchanged and the demo exercises the mechanism by feeding fake plays a low score.
3. **Diminishing returns + diversity keep discovery honest:** a bot (or a genuine
   superfan) looping one work cannot fake *broad* appeal. Because a bot and a real
   superfan are indistinguishable by play-pattern alone, diminishing returns is
   deliberately **not** used to cut a single-work user's payout (that would punish
   honest niche-artist superfans) — it shapes the within-user split and the
   discovery ranking.

**H3 is pure Rust — no contract changes.** The on-chain payout ledger already pays
a Merkle root of per-work credits; H3 only changes how those credits are computed
off-chain. None of the audited money contracts are touched.

### In scope

- `cwe-dapr`: the enriched user-centric model (diminishing returns via a per-work
  play count, bandwidth-credibility discount, governance parameter `k`,
  deterministic integer/fixed-point), plus a per-work diversity/reputation output.
- `cwe-wallet-zk`: the session store counts plays per work; the usage `Opening`/
  commitment binds `plays` (so it cannot be under-reported to dodge the model).
- `cwe-settlement`: assemble the enriched dataset (minutes + plays + neutral
  bandwidth); the disclosure carries plays.
- The browser extension + player agent absorb the commitment/flush shape change
  (mostly free from the shared session store).
- A new anti-fraud demo + updated existing demos + tests + CI.

### Out of scope (deferred seams)

- **Real bandwidth receipts** — the ZK/mutually-signed bandwidth-proof machinery
  (needs the storage/swarm layer, H5). Bandwidth credibility stays a neutral input.
- **Wiring the reputation signal into the live Discovery Hub ranking** — H3
  *produces* the per-work signal; consuming it in hub search/trending is a fast-follow.
- **A governance "bandwidth influence" exponent** and other tunable weights beyond
  `k` — added when governance (Phase 4) exists; bandwidth is a direct linear
  credibility multiplier for now.
- The full DAPR ZK/MPC/rollup pipeline (H2/H4) — the disclosure remains the Phase-1
  stand-in for ZK.

---

## 2. Decisions (locked in brainstorming)

| # | Decision | Choice |
|---|---|---|
| D1 | Economic model | **User-centric** — each fee splits only among works its payer consumed (kept over the spec's global-pool draft; fairer + fraud-capped). |
| D2 | Diminishing returns | Per-work **play count**; `j`-th play worth `1/(1+k·(j-1))`; default `k = 1` (⇒ `j`-th play worth `1/j`). Shapes the within-user split + discovery, **not** a single-work user's payout. |
| D3 | Bandwidth credibility | A per-work multiplier that **discounts the effective fee** (discount → unallocated); neutral default `1.0`, so today's payouts are unchanged. The real receipts are H5. |
| D4 | Diversity | A per-work **discovery/reputation signal** (distinct listeners + weighted usage), **not** a payout lever. |
| D5 | Determinism | Exact **integer/fixed-point** math (no floating point); reproducible for the on-chain Merkle root. The user-centric choice avoids irrational exponents entirely. |
| D6 | Integrity | `plays` is **bound in the usage commitment** alongside `minutes`. |
| D7 | Surface | **Pure Rust**, no Solidity changes. |

---

## 3. The model (precise)

For a subscriber `u` with tier fee `F_u`, and each work `w` they consumed with
`minutes`, `plays`, creator `price_ppm`, `region_ppm`, and a per-work bandwidth
credibility `bw_ppm(w)` (default `1_000_000` = neutral):

```
D(v)        = ( Σ_{j=1..v} 1/(1 + k·(j-1)) ) / v          // diminishing multiplier ∈ (0,1], D(1)=1
raw(u,w)    = minutes · price_ppm · region_ppm · D(plays) // bandwidth-free weight
cred(u,w)   = raw(u,w) · bw_ppm(w) / 1_000_000            // bandwidth-discounted weight
RW_u        = Σ_w raw(u,w)                                // user's raw total (denominator)

credit(u,w) = F_u · cred(u,w) / RW_u                      // this work's payout from u
payout_w    = Σ_u credit(u,w)                             // summed across users
```

**Why the denominator is `raw` (not `cred`):** dividing by the bandwidth-free total
means bandwidth appears only in the numerator, so a low-credibility play pays out
*less* rather than merely re-weighting the split. For a single-work user this yields
`credit = F_u · bw_ppm/1e6` — full fee at neutral bandwidth, a real discount for a
fake (low-bandwidth) play. `D(plays)` appears in both `raw` numerator and `RW_u`
denominator, so it re-shapes a multi-work user's split but **cancels for a
single-work user** — honest superfans are never docked.

**Conservation.** `Σ_w payout_w + unallocated = Σ_u F_u`, where `unallocated`
collects both zero-usage fees and the bandwidth-discounted remainder
`F_u·(1 − Σ_w cred(u,w)/RW_u)`. Nothing is created or destroyed.

**Determinism.** `D(v)` is a sum of rationals (`k` rational ⇒ no irrational powers).
Each term `1/(1+k·(j-1))` is computed as a scaled integer division
`SCALE·1_000_000 / (1_000_000 + k_ppm·(j-1))`, summed, divided by `v`, at a fixed
`SCALE` (the plan pins the exact width; `u128`/`u256` intermediates). Per-user
allocation uses the existing largest-remainder integer split so `Σ` conserves
`F_u` exactly. No floating point anywhere on the payout path.

**Neutral-default safety.** With every `bw_ppm = 1_000_000` and `plays = 1`,
`D(1)=1` and `cred = raw`, so `credit = F_u · raw/RW_u` — identical to today's flat
split. H3 is therefore a strict superset: existing fixtures reproduce bit-for-bit.

### Diversity / reputation output (discovery, not payout)

Alongside `Payouts`, DAPR emits a per-work signal: `distinct_users_w` and
`weighted_usage_w = Σ_u cred(u,w)`. This is the honest popularity signal for
Discovery ranking (broad appeal beats concentrated replays because `D(plays)`
suppresses repeats). The exact ranking formula is the hub's (deferred); H3 only
produces the deterministic signal.

---

## 4. Anti-fraud, stated honestly

| Property | Status in H3 |
|---|---|
| Extract ≤ pay-in (fraud never profitable) | **Structural, always on** — a consequence of user-centric fee-conservation. |
| Fake play pays out *less* than its fee (strict loss) | **Mechanism built; data deferred.** Driven by `bw_ppm` < neutral; the demo exercises it via the input. Real receipts = H5. |
| Bot cannot fake broad popularity in discovery | Diminishing returns + distinct-listener diversity in the reputation signal. |
| Under-reporting plays to dodge diminishing | Prevented — `plays` is bound in the commitment (D6). |
| Honest niche-artist superfan keeps full payout | Preserved — `D(plays)` cancels for a single-work user; neutral bandwidth = full fee. |

---

## 5. Components

| Area | Change |
|---|---|
| `sims/` (`cwe-dapr`) | `DaprParams { diminishing_k_ppm }`; `UsageRow.plays`; per-work `bandwidth_ppm` on `Dataset` (default neutral); `allocate` implements the §3 model deterministically; `Payouts` gains the per-work reputation signal (`distinct_users`, `weighted_usage`) |
| `libs/wallet-zk` | session store counts plays per work (a play = a `start` on a work) and flushes `{work, minutes, plays}`; `Opening`/commitment binds `plays` (commitment pre-image `work‖minutes‖plays‖salt`) |
| `services/settlement` | assemble the enriched `Dataset` (minutes + plays + neutral bandwidth); disclosure `Opening`s carry `plays`; verify the plays-bound commitment |
| `clients/browser-ext` (core + glue) | flush/commit the `plays` field (mostly free from the shared store; WASM wrapper signature) |
| `clients/player-plugin` | same — absorb the `plays` field in accrual/settle |
| `ops/` | new `antifraud-demo`; update `run_demo.sh` (+ any demo asserting the commitment shape) for `(work, minutes, plays)` |

No `chain/` changes.

---

## 6. The anti-fraud demo (`make antifraud-demo`)

Headless, deterministic (it can drive `cwe-dapr` directly and/or the settlement
path). It proves the two honest properties:

1. **Fraud is capped (neutral bandwidth):** a creator spins up puppet subscribers
   who each pay a full tier fee and loop the creator's own work. Assert the creator's
   total payout **equals** the puppets' fees (break-even) and **never exceeds** them —
   fraud cannot profit.
2. **Fake plays are a strict loss (bandwidth input):** re-run with the fake plays'
   `bandwidth_ppm` set low (simulating "no real data moved"). Assert the botted work's
   payout is now **strictly less** than the fees paid, with the difference in
   `unallocated` — the mechanism that turns unprofitable into loss.
3. **Honest fairness preserved:** a genuine superfan (single niche work, full
   bandwidth) still routes their entire fee to that artist; many casual listeners
   out-rank one replayer in the reputation signal; every user's fee is conserved.

Prints `✅ ANTIFRAUD DEMO PASSED` on success.

---

## 7. Testing

**`cwe-dapr` (unit):** fee conservation with diminishing + bandwidth (Σ payouts +
unallocated = Σ fees); neutral defaults reproduce the existing fixtures bit-for-bit;
`D(v)` is monotone non-increasing in `v` and deterministic across runs; a single-work
user's payout is unaffected by `D` (superfan invariant) but *is* discounted by low
bandwidth; a multi-work user's split shifts toward less-replayed works; the reputation
signal ranks broad appeal over concentrated replays.

**`cwe-wallet-zk`:** the commitment binds `plays` — an opening that changes `plays`
fails to match its commitment (tamper detection); the session store counts plays per
work and flushes them; snapshot round-trip preserves play counts.

**`cwe-settlement`:** the enriched dataset assembly carries plays + neutral bandwidth
and reproduces existing payouts when plays=1/bandwidth-neutral.

**End-to-end:** `make -C ops antifraud-demo` passes; `make demo`/`hub-demo`/
`ownership-demo`/`player-demo`/`arbitration-demo` and the full workspace gate stay
green (updated for the `(work, minutes, plays)` commitment).

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| Changing the commitment pre-image breaks every client/settlement path | Add `plays` in the shared `cwe-wallet-zk` (one place); update all callers in the same cycle; neutral `plays=1` reproduces today's commitments' *semantics* while the pre-image is versioned; the demos are the end-to-end gate. |
| Non-determinism from the diminishing math | Exact integer/fixed-point, no floats; a reproducibility test asserts identical output across runs; `k` rational so no irrational powers. |
| Over-claiming anti-fraud | The spec states exactly what holds now (fraud capped) vs what the bandwidth *input* demonstrates (strict loss) vs what H5 supplies (real receipts). |
| Penalising honest superfans | `D(plays)` cancels for single-work users; only low bandwidth (a fake, not a real stream) discounts payout. |

---

## 9. Deliverable

`make -C ops antifraud-demo` prints `✅ ANTIFRAUD DEMO PASSED`; all existing demos and
the full `cargo fmt`/`clippy`/`test` gate stay green. `cwe-dapr` computes the real
user-centric DAPR model deterministically, plays are bound end-to-end, and the
bandwidth-credibility and reputation seams are in place for H5 and discovery.
