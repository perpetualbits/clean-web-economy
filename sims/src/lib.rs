//! DAPR payout-math reference library (WP2).
//!
//! This crate is the **single source of truth** for how a subscription period's
//! tier fees are turned into per-work creator payouts in Phase 1. WP5's on-chain
//! settlement job links this exact code, so the simulator and the real settlement
//! can never disagree numerically.
//!
//! # The formula (Phase 1 scope)
//!
//! Following `sims/README.md` and dev-spec §5.3, each usage row contributes a
//! *value* and every user's paid tier fee is split across the works they listened
//! to in proportion to those values:
//!
//! ```text
//! value_i   = minutes_i · price_ppm_i · region_ppm_i        // one usage row's weight
//! D_user    = Σ_i value_i        over that user's rows       // the user's total value
//! credit_i  = tier_fee_user · value_i / D_user               // that row's share of the fee
//! payout_w  = Σ credit_i         over all rows for work w     // summed across all users
//! ```
//!
//! The richer DAPR model (bandwidth credibility, per-user diminishing returns,
//! the α/β exponents in `docs/specs/DAPR_usage_aggregation_protocol.md` §7–8) is
//! deliberately **out of Phase 1 scope**; only the weighted split above is
//! implemented here.
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

use serde::{Deserialize, Serialize};

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
}

impl Dataset {
    /// Sum of every user's tier fee — the total value that must be fully
    /// distributed (to works plus, at most, the `unallocated` bucket).
    pub fn total_fees(&self) -> u128 {
        self.tier_fees.values().copied().sum()
    }
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

/// Compute per-work payouts from a dataset.
///
/// Each user's fee is split across their usage rows in proportion to row value,
/// using largest-remainder apportionment so the split is exact. Credits are then
/// summed per work. The returned [`Payouts`] always satisfies the fairness
/// invariant `total_to_works() + unallocated == dataset.total_fees()`.
pub fn allocate(dataset: &Dataset) -> Result<Payouts, DaprError> {
    // Group usage rows by user so each user's fee is apportioned over just their
    // own rows. `BTreeMap` keeps user iteration order stable and reproducible.
    let mut rows_by_user: BTreeMap<&UserId, Vec<&UsageRow>> = BTreeMap::new();
    for row in &dataset.usage {
        rows_by_user.entry(&row.user).or_default().push(row);
    }

    let mut per_work: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut unallocated: u128 = 0;

    // Apportion every paying user's fee. A user with no usage rows (or only
    // zero-value rows) has nowhere to send their fee, so it becomes `unallocated`.
    for (user, fee) in &dataset.tier_fees {
        let rows = rows_by_user.get(user).cloned().unwrap_or_default();

        // Compute each row's value and the user's total value in one pass.
        let mut values = Vec::with_capacity(rows.len());
        let mut d_user: u128 = 0;
        for row in &rows {
            let v = row.value()?;
            d_user = d_user.checked_add(v).ok_or(DaprError::Overflow)?;
            values.push(v);
        }

        // No attributable value → the whole fee is unallocated.
        if d_user == 0 {
            unallocated = unallocated.checked_add(*fee).ok_or(DaprError::Overflow)?;
            continue;
        }

        // Apportion `fee` across the rows by value, exactly, and fold the results
        // into the per-work totals.
        let shares = apportion(*fee, &values, d_user)?;
        for (row, share) in rows.iter().zip(shares) {
            let entry = per_work.entry(row.work.clone()).or_insert(0);
            *entry = entry.checked_add(share).ok_or(DaprError::Overflow)?;
        }
    }

    Ok(Payouts {
        per_work,
        unallocated,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a dataset from terse tuples to keep the tests readable.
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
                })
                .collect(),
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
        let out = allocate(&ds).unwrap();
        assert_eq!(out.total_to_works() + out.unallocated, ds.total_fees());
    }

    /// A user with a single work receives their entire fee on that work.
    #[test]
    fn single_work_user_gets_whole_fee() {
        let ds = dataset(
            &[("u1", 777_777)],
            &[("u1", "wONLY", 42, 1_000_000, 1_000_000)],
        );
        let out = allocate(&ds).unwrap();
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
        let out = allocate(&ds).unwrap();
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
        let out = allocate(&ds).unwrap();
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
        let out = allocate(&ds).unwrap();
        let full = *out.per_work.get("wFull").unwrap();
        let half = *out.per_work.get("wHalf").unwrap();
        assert_eq!(full, 600_000);
        assert_eq!(half, 300_000);
        assert_eq!(full + half, 900_000);
    }
}
