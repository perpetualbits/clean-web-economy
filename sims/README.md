# DAPR simulator (`cwe-dapr`)

Reference implementation of the Phase 1 payout math: how a subscription period's
tier fees become per-work creator payouts. This crate is the **single source of
truth** — WP5's on-chain settlement job links the very same code, so the
simulator and the real settlement cannot disagree numerically.

## The formula (Phase 1 scope)

Each user's paid tier fee is split across the works they listened to, in
proportion to a per-row *value*:

```
value_i   = minutes_i · price_ppm_i · region_ppm_i     # one usage row's weight
D_user    = Σ_i value_i   over that user's rows          # the user's total value
credit_i  = tier_fee_user · value_i / D_user             # that row's share of the fee
payout_w  = Σ credit_i    over all rows for work w        # summed across all users
```

The richer DAPR model — bandwidth credibility, per-user diminishing returns, the
α/β exponents in `docs/specs/DAPR_usage_aggregation_protocol.md` §7–8 — is
**out of Phase 1 scope**. Only the weighted split above is implemented.

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
