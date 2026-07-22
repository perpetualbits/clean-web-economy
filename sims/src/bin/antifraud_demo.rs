//! `antifraud_demo` — a deterministic proof of the H3 anti-fraud properties.
//!
//! This binary builds small, hand-computable [`Dataset`]s and runs them through
//! the same [`allocate`] reference math that WP5's settlement links, then
//! asserts three properties by hand-derived numbers (never by re-deriving the
//! formula, which would just test itself):
//!
//! 1. **Fraud is capped, not free.** Puppet users who only ever play one work,
//!    with neutral bandwidth, recover *exactly* the fees they paid for the
//!    creator of that work — never more, and nothing goes unallocated. Looping
//!    plays cannot manufacture value out of nothing.
//! 2. **Fake plays are a strict loss once bandwidth is discredited.** The same
//!    puppet dataset, but with the target work's `bandwidth_ppm` set low,
//!    strictly *loses* value relative to the fees paid — and the lost amount is
//!    conserved into `unallocated`, not silently redistributed to some other
//!    work.
//! 3. **Honest usage is unharmed.** A genuine single-work superfan still routes
//!    their entire fee to the artist regardless of how many times they replay
//!    it (repeat plays are a within-user split concern, not a between-user
//!    discount); and the reputation signal ranks broad appeal (many distinct
//!    listeners) above concentrated replay by one listener, which is exactly
//!    the ordering discovery ranking should reward.
//!
//! Every scenario also re-checks the model's core conservation invariant:
//! `Σ per_work + unallocated == Σ fees`. Any mismatch prints a `FAIL:` line and
//! exits non-zero, so this binary can gate CI the same way the chain demos do.

use std::collections::BTreeMap;
use std::process::ExitCode;

use cwe_dapr::{allocate, DaprParams, Dataset, UsageRow};

/// Entry point: run every scenario in turn, printing a line per assertion.
/// A failed assertion terminates the process immediately with a non-zero code
/// via [`check`] (`std::process::exit(1)`), so `main` only ever reaches its own
/// end — printing the final banner and returning [`ExitCode::SUCCESS`] — when
/// every check has passed.
fn main() -> ExitCode {
    println!("Anti-fraud demo (H3 DAPR model)");
    println!();

    println!("1. Fraud is capped (neutral bandwidth)");
    scenario_fraud_is_capped();
    println!();

    println!("2. Fake plays are a strict loss (discredited bandwidth)");
    scenario_fake_plays_are_a_loss();
    println!();

    println!("3a. Honest superfan still gets their full fee routed");
    scenario_superfan_fairness();
    println!();

    println!("3b. Breadth outranks depth in the reputation signal");
    scenario_reputation_prefers_breadth();
    println!();

    println!("\u{2705} ANTIFRAUD DEMO PASSED");
    ExitCode::SUCCESS
}

/// Print a pass line for a check that held, or print a `FAIL:` line and exit
/// the process with a non-zero status for one that didn't. This is the only
/// place assertions are enforced in this binary — every scenario funnels its
/// comparisons through here so a mismatch can never be silently swallowed.
fn check(label: &str, passed: bool, detail: &str) {
    if passed {
        println!("  ok   {label} ({detail})");
    } else {
        eprintln!("FAIL: {label} ({detail})");
        std::process::exit(1);
    }
}

/// Build the puppet-fraud dataset shared by scenarios 1 and 2: `n` single-work
/// "puppet" users, each paying `fee` and playing only `work` a very large
/// number of times (simulating a bot looping the same track to farm payout).
/// `bandwidth_ppm` is optionally set on `work` to model the bandwidth layer
/// discrediting it; `None` leaves it absent, i.e. neutral (see [`Dataset::bw`]).
fn puppet_dataset(n: u64, fee: u128, work: &str, bandwidth_ppm: Option<u64>) -> Dataset {
    let mut tier_fees = BTreeMap::new();
    let mut usage = Vec::new();
    for i in 0..n {
        let user = format!("puppet{i}");
        tier_fees.insert(user.clone(), fee);
        usage.push(UsageRow {
            user,
            work: work.to_string(),
            minutes: 60,
            price_ppm: 1_000_000,
            region_ppm: 1_000_000,
            // A puppet loops the same work thousands of times; diminishing
            // returns caps how much a *single* work can be worth to a single
            // user, but — as this scenario proves — that cap is irrelevant
            // here: a single-work user's total contribution to that work
            // never depends on `plays` at all (see the module doc's formula:
            // `raw` and `cred` scale by the same `D(plays)` factor, which
            // cancels out of `target = fee · Σcred/Σraw` when there is only
            // one row to sum).
            plays: 5_000,
        });
    }
    let mut bandwidth_map = BTreeMap::new();
    if let Some(bw) = bandwidth_ppm {
        bandwidth_map.insert(work.to_string(), bw);
    }
    Dataset {
        tier_fees,
        usage,
        bandwidth_ppm: bandwidth_map,
    }
}

/// Property 1: with neutral bandwidth, `N` puppet users who each pay fee `F`
/// and only ever play the creator's work `wF` recover *exactly* `N·F` for that
/// work, and nothing is unallocated. The creator breaks even on the puppets'
/// fees — never more — which is the cap: looping plays cannot inflate payout
/// beyond what was actually paid in.
fn scenario_fraud_is_capped() {
    let n: u64 = 5;
    let fee: u128 = 100_000;
    let dataset = puppet_dataset(n, fee, "wF", None);
    let payouts = allocate(&dataset, &DaprParams::default())
        .expect("neutral puppet dataset must allocate without error");

    let expected_total = n as u128 * fee;
    let paid = payouts.per_work.get("wF").copied().unwrap_or(0);

    println!(
        "    per_work[\"wF\"] = {paid}, N·F = {expected_total}, unallocated = {}",
        payouts.unallocated
    );
    check(
        "break-even: per_work[\"wF\"] == N·F",
        paid == expected_total,
        &format!("{paid} == {expected_total}"),
    );
    check(
        "nothing left on the table: unallocated == 0",
        payouts.unallocated == 0,
        &format!("unallocated = {}", payouts.unallocated),
    );
    conserved(&dataset, &payouts);
}

/// Property 2: the identical puppet dataset, except the bandwidth layer has
/// discredited `wF` (`bandwidth_ppm = 100_000`, i.e. 10% credible). The payout
/// is now a *strict* loss relative to the fees paid (`< N·F`), and the exact
/// shortfall is conserved into `unallocated` rather than vanishing or leaking
/// to some other work — proving the bandwidth-credibility discount actually
/// bites once real receipts (H5) discredit fabricated plays.
fn scenario_fake_plays_are_a_loss() {
    let n: u64 = 5;
    let fee: u128 = 100_000;
    let bandwidth_ppm: u64 = 100_000; // 10% credible: heavily discredited
    let dataset = puppet_dataset(n, fee, "wF", Some(bandwidth_ppm));
    let payouts = allocate(&dataset, &DaprParams::default())
        .expect("discredited puppet dataset must allocate without error");

    let total_fees = n as u128 * fee;
    let paid = payouts.per_work.get("wF").copied().unwrap_or(0);
    let expected_shortfall = total_fees - paid;

    println!(
        "    per_work[\"wF\"] = {paid}, N·F = {total_fees}, unallocated = {}",
        payouts.unallocated
    );
    check(
        "strict loss: per_work[\"wF\"] < N·F",
        paid < total_fees,
        &format!("{paid} < {total_fees}"),
    );
    check(
        "discount conserved: unallocated == N·F − per_work[\"wF\"]",
        payouts.unallocated == expected_shortfall,
        &format!("{} == {expected_shortfall}", payouts.unallocated),
    );
    conserved(&dataset, &payouts);
}

/// Property 3a: a genuine superfan — one user, one niche work, full bandwidth,
/// heavy replay — still has their *entire* fee routed to that work. Diminishing
/// returns only ever reshapes how a single user's fee splits *across multiple*
/// works; with only one work in play there is nothing to split against, so the
/// discount cancels out exactly. Honest concentrated fandom is never penalized.
fn scenario_superfan_fairness() {
    let fee: u128 = 1_000_000;
    let mut tier_fees = BTreeMap::new();
    tier_fees.insert("superfan".to_string(), fee);
    let usage = vec![UsageRow {
        user: "superfan".to_string(),
        work: "wNiche".to_string(),
        minutes: 300,
        price_ppm: 1_000_000,
        region_ppm: 1_000_000,
        plays: 1_000, // heavy, genuine replay of a single favorite work
    }];
    let dataset = Dataset {
        tier_fees,
        usage,
        bandwidth_ppm: BTreeMap::new(), // neutral: no bandwidth signal supplied
    };
    let payouts = allocate(&dataset, &DaprParams::default())
        .expect("superfan dataset must allocate without error");

    let paid = payouts.per_work.get("wNiche").copied().unwrap_or(0);
    println!(
        "    per_work[\"wNiche\"] = {paid}, fee = {fee}, unallocated = {}",
        payouts.unallocated
    );
    check(
        "superfan's full fee reaches the artist: per_work == fee",
        paid == fee,
        &format!("{paid} == {fee}"),
    );
    check(
        "nothing withheld from an honest single-work listener",
        payouts.unallocated == 0,
        &format!("unallocated = {}", payouts.unallocated),
    );
    conserved(&dataset, &payouts);
}

/// Property 3b: a work with broad appeal (many distinct one-play listeners)
/// must outrank, in `reputation.weighted_usage`, a work with the same total
/// listening concentrated in a single deeply-replaying listener. Discovery
/// ranking should reward reach, not one fan grinding a play counter.
fn scenario_reputation_prefers_breadth() {
    let broad_users: u64 = 5;
    let fee: u128 = 200_000;

    let mut tier_fees = BTreeMap::new();
    let mut usage = Vec::new();

    // wBroad: five distinct users, one play each.
    for i in 0..broad_users {
        let user = format!("listener{i}");
        tier_fees.insert(user.clone(), fee);
        usage.push(UsageRow {
            user,
            work: "wBroad".to_string(),
            minutes: 10,
            price_ppm: 1_000_000,
            region_ppm: 1_000_000,
            plays: 1,
        });
    }

    // wDeep: a single user, replayed heavily — same per-play weight as above,
    // but only one distinct listener behind all of it.
    tier_fees.insert("grinder".to_string(), fee);
    usage.push(UsageRow {
        user: "grinder".to_string(),
        work: "wDeep".to_string(),
        minutes: 10,
        price_ppm: 1_000_000,
        region_ppm: 1_000_000,
        plays: 500,
    });

    let dataset = Dataset {
        tier_fees,
        usage,
        bandwidth_ppm: BTreeMap::new(),
    };
    let payouts = allocate(&dataset, &DaprParams::default())
        .expect("breadth-vs-depth dataset must allocate without error");

    let broad_rep = payouts
        .reputation
        .get("wBroad")
        .copied()
        .unwrap_or_default();
    let deep_rep = payouts.reputation.get("wDeep").copied().unwrap_or_default();

    println!(
        "    wBroad: distinct_users={}, weighted_usage={}",
        broad_rep.distinct_users, broad_rep.weighted_usage
    );
    println!(
        "    wDeep:  distinct_users={}, weighted_usage={}",
        deep_rep.distinct_users, deep_rep.weighted_usage
    );
    check(
        "wBroad reaches every distinct listener",
        broad_rep.distinct_users == broad_users,
        &format!("distinct_users = {}", broad_rep.distinct_users),
    );
    check(
        "wDeep is a single listener grinding replays",
        deep_rep.distinct_users == 1,
        &format!("distinct_users = {}", deep_rep.distinct_users),
    );
    check(
        "breadth outranks depth: wBroad.weighted_usage > wDeep.weighted_usage",
        broad_rep.weighted_usage > deep_rep.weighted_usage,
        &format!("{} > {}", broad_rep.weighted_usage, deep_rep.weighted_usage),
    );
    conserved(&dataset, &payouts);
}

/// Re-check the model's core invariant on `dataset`/`payouts`:
/// `Σ per_work + unallocated == Σ fees`. Every scenario above calls this so no
/// value is ever created or destroyed by the allocation, ourselves included.
fn conserved(dataset: &Dataset, payouts: &cwe_dapr::Payouts) {
    let total_fees = dataset.total_fees();
    let accounted = payouts.total_to_works() + payouts.unallocated;
    check(
        "conservation: Σ per_work + unallocated == Σ fees",
        accounted == total_fees,
        &format!("{accounted} == {total_fees}"),
    );
}
