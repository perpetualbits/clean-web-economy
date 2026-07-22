# DAPR simulator (`cwe-dapr`)

Reference implementation of the payout math: how a subscription period's tier
fees become per-work creator payouts. This crate is the **single source of
truth** — WP5's on-chain settlement job links the very same code, so the
simulator and the real settlement cannot disagree numerically.

## The formula (H3: user-centric anti-fraud model)

Each user's paid tier fee is split across the works they consumed, in
proportion to a per-row value that is discounted twice — once for repeat plays
of the same work, once for the bandwidth layer's credibility signal — before
being shrunk to a *bandwidth-free* total and apportioned exactly:

```
value_i   = minutes_i · price_ppm_i · region_ppm_i    # one usage row's weight
raw_i     = value_i · D(plays_i)                      # diminishing-returns discount
cred_i    = raw_i · bandwidth_ppm(work_i)              # bandwidth-credibility discount
RW_user   = Σ_i raw_i           over that user's rows    # bandwidth-FREE denominator
target    = tier_fee_user · (Σ cred_i / RW_user)       # ≤ tier_fee_user
credit_i  = target · cred_i / Σ cred_i                 # that row's share of `target`
payout_w  = Σ credit_i          over all rows for work w  # summed across all users
```

See `sims/src/lib.rs` (`allocate`, `d_ppm`, `Dataset::bw`) for the exact
implementation; the module doc there is the authoritative derivation.

### User-centric split

The fee is apportioned per user, over *that user's own rows only* — one user's
listening never dilutes or inflates another user's payout. This is what makes
the model composable: a puppet account's behavior only ever affects the fee
that puppet itself paid in.

### Diminishing returns (`k`)

Repeat plays of the *same* work by the *same* user are worth progressively
less: the `j`-th play counts for `1/(1 + k·(j-1))` of the first, governed by
the tunable `DaprParams::diminishing_k_ppm` (default `k = 1.0`, i.e. `1/j`).
This only ever reshapes how one user's fee splits **across multiple works** —
a single-work user's total contribution to that one work is unaffected by
`plays` at all, because the discount appears in both the numerator (`cred`)
and the bandwidth-free denominator (`RW_user`) and cancels. That cancellation
is deliberate: it is what keeps heavy, honest replay of one favorite work
(a superfan) from ever being penalized.

### Bandwidth-credibility discount + neutral default

`bandwidth_ppm(work)` — supplied by the bandwidth layer, clamped to
`[0, 1_000_000]` — scales `cred` but not `RW_user`. Because the denominator
stays bandwidth-free, a discredited work's share of `target` shrinks below the
user's full fee rather than merely being redistributed to other works: the
shortfall becomes `unallocated`, a real loss to whoever is farming the fake
plays. A work absent from `bandwidth_ppm` defaults to `1_000_000` (fully
credible) — the neutral value that reproduces every pre-H3 fixture bit for
bit, since no fixture supplies bandwidth data.

### The reputation signal

Alongside the payout, `allocate` returns a `Reputation` per work —
`distinct_users` (breadth) and `weighted_usage` (the same bandwidth- and
diminishing-adjusted `cred` used for payout, summed instead of apportioned).
It is purely informational: it never changes how much a work is paid, only
how discovery might rank or surface it. It is built to reward broad appeal
(many distinct listeners) over one listener grinding a play counter.

### Deferred seams

- **Real bandwidth receipts (H5).** `bandwidth_ppm` is an input this crate
  trusts; nothing here verifies it. H5 is expected to populate it from actual
  bandwidth/delivery receipts rather than the neutral default used today.
- **Reputation into hub ranking (fast-follow).** `Reputation` is computed and
  returned but not yet consumed anywhere — wiring it into the Discovery Hub's
  ranking is deliberately out of scope for this task.
- **Governance influence exponent.** `docs/specs/DAPR_usage_aggregation_protocol.md`
  §7–8 describes further α/β exponents beyond `diminishing_k_ppm`; only the
  diminishing-returns rate is a tunable `DaprParams` field today.

See `sims/src/bin/antifraud_demo.rs` (`make -C ops antifraud-demo`) for a
deterministic, hand-checked demonstration of the anti-fraud properties this
model is meant to guarantee: fraud is capped to break-even, discredited plays
are a strict loss (conserved to `unallocated`, not merely redistributed), and
honest single-work and broad-appeal usage are never penalized.

## Exact integer math

All amounts are integers. Fractional prices and region factors are carried as
**parts-per-million** (`1.2 → 1_200_000`, `0.9 → 900_000`), so no floating point
enters the allocation and the result is bit-for-bit reproducible — a requirement
for on-chain settlement.

Splitting an integer fee by a ratio leaves a remainder. Rather than dropping it
(which would break `Σ payouts == Σ fees`), each user's fee is apportioned with the
**largest-remainder method**, so their credits sum to *exactly* their fee. The
only `unallocated` amount arises when a user has no attributable value at all
(every row zero-minute); that fee cannot be routed to any work.

Invariant, checked for every fixture: `Σ payouts + unallocated == Σ fees`.

## Fixtures

Inputs live in `fixtures/*.json` as a `Dataset` (`tier_fees` per user + `usage`
rows). Each has a committed `*_expected.json` oracle:

| Fixture | Exercises |
|---|---|
| `basic.json` | the original `sample_usage.csv` data, plus tier fees |
| `single_work.json` | a user whose whole fee lands on one work |
| `zero_value.json` | a zero-minute user → `unallocated` bucket |
| `region_factors.json` | region factor < 1.0 scaling a share |
| `multi_user.json` | a larger 5-user / 7-work set with uneven splits |

`sample_usage.csv` is kept as a human-readable illustration; `basic.json` is its
structured equivalent (with the tier fees the CSV lacks).

## Run

```sh
# Recompute an oracle after changing a fixture or the math:
cargo run -p cwe-dapr --bin simulate -- sims/fixtures/basic.json

# Tests: unit (math), plus fixture tests that re-derive every oracle and assert it
# matches the committed file (guards against silent drift):
cargo test -p cwe-dapr
```
