//! DAPR payout-math reference library (WP2, enriched in H3).
//!
//! This crate is the **single source of truth** for how a subscription period's
//! tier fees are turned into per-work creator payouts. WP5's on-chain settlement
//! job links this exact code, so the simulator and the real settlement can never
//! disagree numerically.
//!
//! # The formula (user-centric DAPR model)
//!
//! Following `sims/README.md` and dev-spec §5.3, each usage row contributes a
//! *raw* value, discounted for repeat plays of the same work and for the
//! bandwidth layer's credibility signal; every user's paid tier fee is then
//! split across the works they consumed in proportion to that discounted value:
//!
//! ```text
//! value_i   = minutes_i · price_ppm_i · region_ppm_i         // one usage row's weight
//! raw_i     = value_i · D(plays_i)                           // diminishing-returns discount
//! cred_i    = raw_i · bandwidth_ppm(work_i)                  // bandwidth-credibility discount
//! RW_user   = Σ_i raw_i             over that user's rows      // bandwidth-FREE denominator
//! target    = tier_fee_user · (Σ cred_i / RW_user)           // ≤ tier_fee_user
//! credit_i  = target · cred_i / Σ cred_i                     // that row's share of `target`
//! payout_w  = Σ credit_i            over all rows for work w   // summed across all users
//! ```
//!
//! Using the bandwidth-free `RW_user` as the denominator means low bandwidth
//! *discounts* a user's payout (the shortfall becomes `unallocated`) rather than
//! merely redistributing it to other works — the anti-fraud property the model
//! depends on. See [`allocate`] and [`d_ppm`] for the exact per-row computation,
//! and [`Reputation`] for the discovery-facing signal computed alongside it.
//!
//! # Why integer (ppm) math
//!
//! All amounts are integers. Prices and region factors, which are naturally
//! fractional (e.g. `1.2`, `0.9`), are carried as **parts-per-million** (`ppm`):
//! `1.2 → 1_200_000`, `0.9 → 900_000`. Floating point is never used in the
//! allocation, so the result is bit-for-bit reproducible on any machine — a hard
//! requirement for on-chain settlement.
//!
//! # Exact fairness (largest-remainder apportionment)
//!
//! Splitting an integer fee by a ratio inevitably leaves a remainder. Rather than
//! discarding it (which would break `Σ payouts == Σ fees`), each user's fee is
//! apportioned with the **largest-remainder method**: the flooring leftover is
//! handed out one unit at a time to the rows with the largest fractional part.
//! This guarantees each user's credits sum to *exactly* their fee, so globally
//! `Σ payouts + unallocated == Σ fees` with zero minted or lost value. The only
//! `unallocated` amount arises when a user has **no attributable value at all**
//! (every row zero-minute), whose fee cannot be routed to any work.

#![forbid(unsafe_code)] // pure arithmetic and data-shuffling; no unsafe needed

use std::collections::BTreeMap;

use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

/// Compute `a · b / denom` exactly, using a 256-bit intermediate so the product
/// never overflows. Returns [`DaprError::Overflow`] only if the *result* exceeds
/// `u128` (never for a genuine ratio where `b ≤ denom`), or [`DaprError::Overflow`]
/// on a zero denominator (a caller bug this surfaces loudly).
fn mul_div(a: u128, b: u128, denom: u128) -> Result<u128, DaprError> {
    if denom == 0 {
        return Err(DaprError::Overflow);
    }
    // 256-bit product / 256-bit divide, then narrow back to u128.
    let prod = U256::from(a) * U256::from(b);
    let quot = prod / U256::from(denom);
    u128::try_from(quot).map_err(|_| DaprError::Overflow)
}

/// Identifier for a subscriber. Only used to group rows by user; never leaves the
/// simulator, mirroring the privacy stance of the real aggregator.
pub type UserId = String;

/// Identifier for a work (a track). In production this is a 256-bit manifest id;
/// in fixtures it is a short human-readable label such as `"wA"`.
pub type WorkId = String;

/// A single unit of listening: one user's time on one work, with the pricing that
/// applied. Prices and the region factor are expressed in parts-per-million.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRow {
    /// Which subscriber generated this usage.
    pub user: UserId,
    /// Which work was consumed.
    pub work: WorkId,
    /// Whole minutes listened. May be `0` (an edge case the fixtures exercise).
    pub minutes: u64,
    /// Creator's price per minute, in ppm (`1.2` per minute → `1_200_000`).
    pub price_ppm: u64,
    /// Regional adjustment factor, in ppm (`1.0` → `1_000_000`, `0.9` → `900_000`).
    pub region_ppm: u64,
    /// Number of plays this row aggregates; `1` is a single play. Repeat plays of
    /// the same work by the same user are worth progressively less (diminishing
    /// returns, see [`d_ppm`]). Defaults to `0` when absent from a fixture, which
    /// [`d_ppm`] treats identically to `1` (a single, undiminished play) — so
    /// older fixtures without this field keep reproducing their exact payouts.
    #[serde(default)]
    pub plays: u64,
}

impl UsageRow {
    /// The row's raw value `minutes · price_ppm · region_ppm`, computed in `u128`.
    ///
    /// Returns [`DaprError::Overflow`] if the product does not fit in `u128`; for
    /// Phase 1 fixtures (modest minute counts and ppm prices) it never will, but
    /// failing loudly beats a silent wraparound that would corrupt every payout.
    fn value(&self) -> Result<u128, DaprError> {
        (self.minutes as u128)
            .checked_mul(self.price_ppm as u128)
            .and_then(|v| v.checked_mul(self.region_ppm as u128))
            .ok_or(DaprError::Overflow)
    }
}

/// A complete input to the DAPR computation: what each user paid, and every usage
/// row for the epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    /// The tier fee each user paid this epoch, in the smallest integer credit
    /// unit. A user appearing in `usage` but not here is treated as paying `0`.
    pub tier_fees: BTreeMap<UserId, u128>,
    /// Every `(user, work, minutes, price, region)` usage row for the epoch.
    pub usage: Vec<UsageRow>,
    /// Per-work bandwidth credibility, in ppm, as supplied by the bandwidth
    /// layer. A work absent from this map is neutral (see [`Dataset::bw`]).
    #[serde(default)]
    pub bandwidth_ppm: BTreeMap<WorkId, u64>,
}

impl Dataset {
    /// Sum of every user's tier fee — the total value that must be fully
    /// distributed (to works plus, at most, the `unallocated` bucket).
    pub fn total_fees(&self) -> u128 {
        self.tier_fees.values().copied().sum()
    }

    /// The bandwidth credibility for `work` in ppm, clamped to `[0, 1_000_000]`.
    /// `1_000_000` (neutral) when the bandwidth layer has provided none. Clamping the
    /// upper bound guarantees `cred ≤ raw`, so bandwidth can only discount payout — a
    /// stray above-neutral value degrades to neutral instead of breaking conservation
    /// and failing the epoch.
    pub fn bw(&self, work: &WorkId) -> u64 {
        self.bandwidth_ppm
            .get(work)
            .copied()
            .unwrap_or(1_000_000)
            .min(1_000_000)
    }
}

/// Governance-tunable DAPR parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaprParams {
    /// Diminishing-returns rate `k`, in ppm (`1_000_000` = k=1.0, the default:
    /// the j-th play is worth 1/j).
    pub diminishing_k_ppm: u64,
}

impl Default for DaprParams {
    /// The neutral default: `k = 1.0`, so the `j`-th play of a work is worth
    /// `1/j` of the first — the standard diminishing-returns curve.
    fn default() -> Self {
        DaprParams {
            diminishing_k_ppm: 1_000_000,
        }
    }
}

/// A per-work discovery/reputation signal (not a payout term).
///
/// This is informational output for the discovery layer: it never affects how
/// much a work is paid, only how it might be ranked or surfaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reputation {
    /// Number of distinct users who consumed the work this epoch.
    pub distinct_users: u64,
    /// Bandwidth-discounted, diminishing-adjusted usage summed across users.
    pub weighted_usage: u128,
}

/// The result of a DAPR run: how much each work is owed, plus any fee that could
/// not be attributed to a work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payouts {
    /// Credit owed to each work, keyed by work id. Ordered (`BTreeMap`) so the
    /// serialised output — and any Merkle tree built from it — is deterministic.
    pub per_work: BTreeMap<WorkId, u128>,
    /// Fee that belonged to users with zero total attributable value, and so
    /// could not be routed to any work. `0` for every fixture where each user
    /// has at least one positive-value row.
    pub unallocated: u128,
    /// Per-work reputation signal, derived alongside the payout but never
    /// spent: breadth (distinct users) and depth (weighted usage) of appeal.
    pub reputation: BTreeMap<WorkId, Reputation>,
}

impl Payouts {
    /// Total credit routed to works (excludes [`Payouts::unallocated`]).
    pub fn total_to_works(&self) -> u128 {
        self.per_work.values().copied().sum()
    }
}

/// Errors the DAPR computation can raise.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DaprError {
    /// An intermediate product exceeded `u128`. See [`UsageRow::value`].
    #[error("arithmetic overflow while computing usage value")]
    Overflow,
}

/// Compute per-work payouts (and the reputation signal) from a dataset.
///
/// For every usage row, `raw = minutes·price_ppm·region_ppm·D(plays)` — the row's
/// value, discounted by diminishing returns for repeat plays of the same work.
/// `cred = raw · bandwidth_ppm` further discounts by the bandwidth layer's
/// credibility signal. Per user, the fee is first shrunk to `target = fee ·
/// (Σcred/Σraw)` — using the *bandwidth-free* `Σraw` as the denominator so low
/// bandwidth discounts the payout rather than merely redistributing it — then
/// `target` is split across the user's rows in proportion to `cred` with
/// largest-remainder apportionment, exactly as before. Whatever the bandwidth
/// discount removes (`fee − target`) joins `unallocated`, alongside fees from
/// users with no attributable value at all. The returned [`Payouts`] always
/// satisfies `total_to_works() + unallocated == dataset.total_fees()`.
pub fn allocate(dataset: &Dataset, params: &DaprParams) -> Result<Payouts, DaprError> {
    let k = params.diminishing_k_ppm;

    // Group usage rows by user so each user's fee is apportioned over just their
    // own rows. `BTreeMap` keeps user iteration order stable and reproducible.
    let mut rows_by_user: BTreeMap<&UserId, Vec<&UsageRow>> = BTreeMap::new();
    for row in &dataset.usage {
        rows_by_user.entry(&row.user).or_default().push(row);
    }

    let mut per_work: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut unallocated: u128 = 0;
    // Reputation accumulators: total bandwidth+diminishing-adjusted usage, and
    // the set of distinct users, per work. Populated alongside the payout pass
    // so the two signals are always computed from the same row data.
    let mut rep_usage: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut rep_users: BTreeMap<WorkId, std::collections::BTreeSet<&UserId>> = BTreeMap::new();

    // Apportion every paying user's fee. A user with no usage rows (or only
    // zero-value rows) has nowhere to send their fee, so it becomes `unallocated`.
    for (user, fee) in &dataset.tier_fees {
        let rows = rows_by_user.get(user).cloned().unwrap_or_default();

        // Compute each row's raw (diminished) and cred (bandwidth-discounted)
        // value, and this user's totals of each, in one pass.
        let mut creds = Vec::with_capacity(rows.len());
        let mut rw_u: u128 = 0;
        let mut sum_cred: u128 = 0;
        for row in &rows {
            let value = row.value()?; // minutes·price_ppm·region_ppm
            let raw = mul_div(value, d_ppm(row.plays, k) as u128, 1_000_000)?;
            let cred = mul_div(raw, dataset.bw(&row.work) as u128, 1_000_000)?;
            rw_u = rw_u.checked_add(raw).ok_or(DaprError::Overflow)?;
            sum_cred = sum_cred.checked_add(cred).ok_or(DaprError::Overflow)?;
            creds.push(cred);

            // Reputation uses `cred` (bandwidth+diminishing adjusted usage),
            // accumulated into a local before inserting to avoid re-borrowing
            // the map while it is already borrowed by `entry`.
            let updated = rep_usage.get(&row.work).copied().unwrap_or(0);
            let updated = updated.checked_add(cred).ok_or(DaprError::Overflow)?;
            rep_usage.insert(row.work.clone(), updated);
            rep_users.entry(row.work.clone()).or_default().insert(user);
        }

        // No attributable raw value at all → the whole fee is unallocated.
        if rw_u == 0 || sum_cred == 0 {
            unallocated = unallocated.checked_add(*fee).ok_or(DaprError::Overflow)?;
            continue;
        }

        // Amount actually paid to works this user's fee funds: `fee` scaled by
        // the bandwidth-discounted share of raw value. `≤ fee`; the shortfall is
        // the bandwidth discount, which goes to `unallocated` below.
        let target = mul_div(*fee, sum_cred, rw_u)?;
        unallocated = unallocated
            .checked_add(fee.checked_sub(target).ok_or(DaprError::Overflow)?)
            .ok_or(DaprError::Overflow)?;

        // Split `target` across rows by cred, exactly (largest remainder), and
        // fold the results into the per-work totals.
        let shares = apportion(target, &creds, sum_cred)?;
        for (row, share) in rows.iter().zip(shares) {
            let entry = per_work.entry(row.work.clone()).or_insert(0);
            *entry = entry.checked_add(share).ok_or(DaprError::Overflow)?;
        }
    }

    // Fold the accumulators into the public `Reputation` shape.
    let reputation = rep_usage
        .into_iter()
        .map(|(w, weighted_usage)| {
            let distinct_users = rep_users.get(&w).map(|s| s.len() as u64).unwrap_or(0);
            (
                w,
                Reputation {
                    distinct_users,
                    weighted_usage,
                },
            )
        })
        .collect();

    Ok(Payouts {
        per_work,
        unallocated,
        reputation,
    })
}

/// Split `total` across items in proportion to `weights` (which sum to
/// `weight_sum`), returning integer shares that sum to *exactly* `total`.
///
/// Uses the largest-remainder (Hamilton) method: every item first gets the floor
/// of its ideal share, then the leftover units — there are fewer than
/// `weights.len()` of them — go one each to the items with the largest division
/// remainder, ties broken by original index for determinism.
///
/// Preconditions: `weight_sum == Σ weights` and `weight_sum > 0`. The caller
/// (`allocate`) guarantees both; the zero-sum case is handled before calling.
fn apportion(total: u128, weights: &[u128], weight_sum: u128) -> Result<Vec<u128>, DaprError> {
    // `base` holds each item's floored share; `remainders` the leftover numerator
    // used to rank who gets the spare units.
    let mut base = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    let mut assigned: u128 = 0;

    for &w in weights {
        // numerator = total * weight; the ideal (fractional) share is numerator/weight_sum.
        let numerator = total.checked_mul(w).ok_or(DaprError::Overflow)?;
        let floor = numerator / weight_sum; // integer floor of the ideal share
        let rem = numerator % weight_sum; // how much was rounded away (0..weight_sum)
        assigned += floor; // running total of floored shares
        base.push(floor);
        remainders.push(rem);
    }

    // The leftover is exactly the units lost to flooring; it is strictly less than
    // the number of items, so at most one extra unit is ever added per item.
    let leftover = total - assigned;

    // Rank item indices by remainder (largest first); ties keep the lower index so
    // the outcome is fully deterministic and reproducible on-chain.
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| remainders[b].cmp(&remainders[a]).then(a.cmp(&b)));

    // Hand out the leftover units to the top-ranked items.
    for &idx in order.iter().take(leftover as usize) {
        base[idx] += 1;
    }

    Ok(base)
}

/// Upper bound on the play count `d_ppm` iterates over. Real per-epoch plays are
/// bounded by minutes listened; this cap purely defends the O(plays) loop against
/// an adversarially large committed value. `d_ppm` is monotone non-increasing, so
/// clamping to the cap yields a conservative (slightly higher) multiplier with a
/// negligible payout effect — the diminishing average is already near its floor by
/// this many plays — while bounding the work per row.
pub const PLAYS_CAP: u64 = 100_000;

/// The diminishing-returns multiplier for `plays` repeat plays, in ppm.
///
/// `D(v) = ( Σ_{j=1..v} 1/(1 + k·(j-1)) ) / v`, with `k = k_ppm / 1_000_000`. The
/// `j`-th play is worth `1/(1 + k·(j-1))`, so repeats count for progressively less;
/// the result is the average per-play value, in ppm (`1_000_000` = full weight).
/// `plays == 0` is treated as a single unit (`1_000_000`). Pure integer math, so it
/// is bit-for-bit reproducible. `plays` is clamped to [`PLAYS_CAP`] before the loop
/// runs, so an adversarially large committed play count cannot blow up the work
/// done here (or in [`allocate`], which calls this once per usage row).
pub fn d_ppm(plays: u64, k_ppm: u64) -> u64 {
    let v = plays.clamp(1, PLAYS_CAP);
    // Σ term_j, each term scaled to ppm: term_j = 1e6 · 1e6 / (1e6 + k·(j-1)).
    let mut sum_ppm: u128 = 0;
    for j in 1..=v {
        // denominator 1 + k·(j-1), itself in ppm.
        let denom_ppm = 1_000_000u128 + (k_ppm as u128) * ((j - 1) as u128);
        // 1/denom in ppm = 1e6·1e6/denom_ppm (denom_ppm already carries the 1e6 scale).
        sum_ppm += 1_000_000u128 * 1_000_000u128 / denom_ppm;
    }
    // Average over v plays, still in ppm.
    (sum_ppm / v as u128) as u64
}

#[cfg(test)]
mod dr_tests {
    use super::*;

    /// D_ppm is 1.0 for a single play, decreases with repeats, and matches the
    /// closed form for k=1 (j-th play worth 1/j): D(2)=(1+1/2)/2=0.75 → 750000.
    #[test]
    fn diminishing_multiplier() {
        let k = 1_000_000; // k = 1.0
        assert_eq!(d_ppm(1, k), 1_000_000);
        assert_eq!(d_ppm(0, k), 1_000_000); // no plays behaves as a single unit
        assert_eq!(d_ppm(2, k), 750_000);
        // Monotone non-increasing.
        let mut prev = 1_000_001u64;
        for v in 1..=50 {
            let d = d_ppm(v, k);
            assert!(d <= prev, "D must not increase at v={v}");
            prev = d;
        }
        // Deterministic across calls.
        assert_eq!(d_ppm(37, k), d_ppm(37, k));
    }

    /// An adversarially large play count is clamped to `PLAYS_CAP` before the
    /// O(plays) loop runs: `d_ppm(u64::MAX, ..)` must match `d_ppm(PLAYS_CAP, ..)`
    /// exactly, and — since this test completes at all — the loop never actually
    /// iterated anywhere near `u64::MAX` times (the settlement-DoS this guards
    /// against). Monotonicity is checked up to the cap in `diminishing_multiplier`.
    #[test]
    fn diminishing_multiplier_clamps_huge_play_counts() {
        let k = 1_000_000;
        assert_eq!(d_ppm(u64::MAX, k), d_ppm(PLAYS_CAP, k));
    }
}

#[cfg(test)]
mod h3_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ds(fees: &[(&str, u128)], rows: &[(&str, &str, u64, u64, u64, u64)]) -> Dataset {
        let mut tier_fees = BTreeMap::new();
        for (u, f) in fees {
            tier_fees.insert(u.to_string(), *f);
        }
        let usage = rows
            .iter()
            .map(|(u, w, m, p, r, pl)| UsageRow {
                user: u.to_string(),
                work: w.to_string(),
                minutes: *m,
                price_ppm: *p,
                region_ppm: *r,
                plays: *pl,
            })
            .collect();
        Dataset {
            tier_fees,
            usage,
            bandwidth_ppm: BTreeMap::new(),
        }
    }

    /// Neutral params (plays=1, bandwidth default) reproduce the flat split exactly.
    #[test]
    fn neutral_defaults_reproduce_flat_split() {
        let d = ds(
            &[("u1", 1_000_000)],
            &[
                ("u1", "wA", 60, 1_000_000, 1_000_000, 1),
                ("u1", "wB", 20, 1_000_000, 1_000_000, 1),
            ],
        );
        let p = allocate(&d, &DaprParams::default()).unwrap();
        // 60:20 split of the fee, fully conserved.
        assert_eq!(p.per_work["wA"], 750_000);
        assert_eq!(p.per_work["wB"], 250_000);
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// A single-work user's payout is unaffected by diminishing returns
    /// (superfan invariant): heavy replays still route the whole fee to the work.
    #[test]
    fn single_work_superfan_unaffected_by_diminishing() {
        let d = ds(
            &[("u1", 1_000_000)],
            &[("u1", "wA", 300, 1_000_000, 1_000_000, 100)],
        );
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.per_work["wA"], 1_000_000); // full fee, no diminishing penalty
        assert_eq!(p.unallocated, 0);
    }

    /// Low bandwidth discounts a single-work user's payout; the difference is
    /// unallocated (this is the anti-fraud mechanism).
    #[test]
    fn low_bandwidth_discounts_and_conserves() {
        let mut d = ds(
            &[("u1", 1_000_000)],
            &[("u1", "wA", 60, 1_000_000, 1_000_000, 1)],
        );
        d.bandwidth_ppm.insert("wA".into(), 250_000); // 25% credible
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.per_work["wA"], 250_000); // paid = fee · bw
        assert_eq!(p.unallocated, 750_000); // discounted remainder
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// A `bandwidth_ppm` value above neutral (`1_000_000`) — a bug or bad
    /// governance input — must clamp to neutral rather than inflate `cred` above
    /// `raw`. Before the clamp this made `sum_cred > rw_u`, so `target > fee` and
    /// `fee.checked_sub(target)` underflowed to `DaprError::Overflow`, failing the
    /// whole epoch. Clamped, the row behaves exactly as if bandwidth were absent:
    /// the full fee is paid, nothing is unallocated, and no error is raised.
    #[test]
    fn above_neutral_bandwidth_clamps_and_does_not_error() {
        let mut d = ds(
            &[("u1", 1_000_000)],
            &[("u1", "wA", 60, 1_000_000, 1_000_000, 1)],
        );
        d.bandwidth_ppm.insert("wA".into(), 2_000_000); // double neutral: bogus input
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.per_work["wA"], 1_000_000); // clamped to neutral: full fee, no inflation
        assert_eq!(p.unallocated, 0);
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// Diminishing returns shifts a multi-work user's split toward the less-replayed
    /// work, while still conserving the fee.
    #[test]
    fn diminishing_shifts_multiwork_split() {
        // Equal minutes/price/region; wB replayed 4×, wA once.
        let d = ds(
            &[("u1", 1_000_000)],
            &[
                ("u1", "wA", 10, 1_000_000, 1_000_000, 1),
                ("u1", "wB", 10, 1_000_000, 1_000_000, 4),
            ],
        );
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert!(
            p.per_work["wA"] > p.per_work["wB"],
            "less-replayed work gets more"
        );
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// The reputation signal ranks broad appeal over concentrated replays.
    #[test]
    fn reputation_prefers_breadth() {
        // wBroad: two distinct users, one play each. wDeep: one user, many plays.
        let d = ds(
            &[("u1", 1_000_000), ("u2", 1_000_000)],
            &[
                ("u1", "wBroad", 10, 1_000_000, 1_000_000, 1),
                ("u2", "wBroad", 10, 1_000_000, 1_000_000, 1),
                ("u1", "wDeep", 10, 1_000_000, 1_000_000, 50),
            ],
        );
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.reputation["wBroad"].distinct_users, 2);
        assert_eq!(p.reputation["wDeep"].distinct_users, 1);
        assert!(p.reputation["wBroad"].weighted_usage > p.reputation["wDeep"].weighted_usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a dataset from terse tuples to keep the tests readable. Every row
    /// gets `plays: 1` (a single, undiminished play) and bandwidth is left
    /// empty (neutral for every work) — these are the pre-H3 fixture tests,
    /// unchanged, proving the new model reproduces the old one bit-for-bit.
    fn dataset(fees: &[(&str, u128)], rows: &[(&str, &str, u64, u64, u64)]) -> Dataset {
        Dataset {
            tier_fees: fees.iter().map(|(u, f)| (u.to_string(), *f)).collect(),
            usage: rows
                .iter()
                .map(|(u, w, m, p, r)| UsageRow {
                    user: u.to_string(),
                    work: w.to_string(),
                    minutes: *m,
                    price_ppm: *p,
                    region_ppm: *r,
                    plays: 1,
                })
                .collect(),
            bandwidth_ppm: BTreeMap::new(),
        }
    }

    /// The core fairness invariant: every unit of every fee is accounted for.
    #[test]
    fn fairness_all_fees_conserved() {
        let ds = dataset(
            &[("u1", 1_000_000), ("u2", 500_000)],
            &[
                ("u1", "wA", 120, 1_200_000, 1_000_000),
                ("u1", "wB", 30, 600_000, 1_000_000),
                ("u2", "wA", 10, 1_200_000, 1_000_000),
                ("u2", "wC", 90, 2_500_000, 900_000),
            ],
        );
        let out = allocate(&ds, &DaprParams::default()).unwrap();
        assert_eq!(out.total_to_works() + out.unallocated, ds.total_fees());
    }

    /// A user with a single work receives their entire fee on that work.
    #[test]
    fn single_work_user_gets_whole_fee() {
        let ds = dataset(
            &[("u1", 777_777)],
            &[("u1", "wONLY", 42, 1_000_000, 1_000_000)],
        );
        let out = allocate(&ds, &DaprParams::default()).unwrap();
        assert_eq!(out.per_work.get("wONLY"), Some(&777_777));
        assert_eq!(out.unallocated, 0);
    }

    /// Zero-minute rows carry zero value; a user whose rows are all zero-value has
    /// their fee routed to `unallocated`, and the invariant still holds.
    #[test]
    fn zero_value_user_fee_is_unallocated() {
        let ds = dataset(
            &[("u1", 1_000_000), ("uZero", 400_000)],
            &[
                ("u1", "wA", 60, 1_000_000, 1_000_000),
                ("uZero", "wB", 0, 1_000_000, 1_000_000), // zero minutes → zero value
            ],
        );
        let out = allocate(&ds, &DaprParams::default()).unwrap();
        assert_eq!(out.per_work.get("wA"), Some(&1_000_000));
        assert!(!out.per_work.contains_key("wB")); // no value → no credit
        assert_eq!(out.unallocated, 400_000);
        assert_eq!(out.total_to_works() + out.unallocated, ds.total_fees());
    }

    /// Largest-remainder apportionment must place the leftover unit deterministically
    /// and conserve the fee exactly, even when the split does not divide evenly.
    #[test]
    fn apportionment_is_exact_and_deterministic() {
        // Fee 10 split three equal ways: ideal 3.33 each; floors 3,3,3 = 9, one
        // leftover unit goes to the largest-remainder, lowest-index item (index 0).
        let ds = dataset(
            &[("u1", 10)],
            &[
                ("u1", "wX", 1, 1_000_000, 1_000_000),
                ("u1", "wY", 1, 1_000_000, 1_000_000),
                ("u1", "wZ", 1, 1_000_000, 1_000_000),
            ],
        );
        let out = allocate(&ds, &DaprParams::default()).unwrap();
        assert_eq!(out.per_work.get("wX"), Some(&4)); // got the leftover unit
        assert_eq!(out.per_work.get("wY"), Some(&3));
        assert_eq!(out.per_work.get("wZ"), Some(&3));
        assert_eq!(out.total_to_works(), 10); // exact, nothing lost
    }

    /// Region factors below 1.0 reduce a row's value (and thus its share) relative
    /// to an otherwise identical full-region row.
    #[test]
    fn region_factor_reduces_share() {
        // Two equal-minute, equal-price works; one at region 0.5. The full-region
        // work should receive twice the credit of the half-region one.
        let ds = dataset(
            &[("u1", 900_000)],
            &[
                ("u1", "wFull", 100, 1_000_000, 1_000_000),
                ("u1", "wHalf", 100, 1_000_000, 500_000),
            ],
        );
        let out = allocate(&ds, &DaprParams::default()).unwrap();
        let full = *out.per_work.get("wFull").unwrap();
        let half = *out.per_work.get("wHalf").unwrap();
        assert_eq!(full, 600_000);
        assert_eq!(half, 300_000);
        assert_eq!(full + half, 900_000);
    }
}
