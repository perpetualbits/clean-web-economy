# Full DAPR + Anti-Fraud (H3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the payout math from a flat split to the real **user-centric DAPR model** — diminishing returns on repeat plays, a bandwidth-credibility discount, a diversity/reputation signal — with `plays` bound in the usage commitment, all in deterministic integer math.

**Architecture:** Pure Rust, no contract changes. `cwe-dapr` gets the enriched model; `cwe-wallet-zk` counts and binds `plays`; `cwe-settlement` assembles the enriched dataset; the two clients absorb the `plays` field; a new demo proves the anti-fraud properties.

**Design spec:** `docs/superpowers/specs/2026-07-22-full-dapr-antifraud-design.md`.

## Global Constraints

- **Pure Rust; no Solidity/contract changes** this cycle.
- **No attribution to any coding agent, assistant, or automated tool** anywhere — code, comments, docs, commit messages, branch/PR text. Hard rule.
- **Every function has a `///` doc block**; non-trivial lines get an inline comment only where it adds understanding.
- **Determinism:** the payout path is exact integer/fixed-point — **no floating point**. Reproducible bit-for-bit for the on-chain Merkle root.
- **Neutral-default safety:** with `plays = 1` and every `bandwidth_ppm = 1_000_000`, the new model must reproduce the existing payouts **bit-for-bit** (a strict superset). Existing `cwe-dapr` and settlement fixtures must still pass unchanged in value.
- **User-centric + fee-conserving:** `Σ per_work + unallocated == Σ tier_fees` always holds.
- **Integrity:** `plays` is bound in the commitment pre-image alongside `minutes`.
- `cargo fmt` / `clippy -D warnings` / `test` stay green; every existing demo stays green.

## Core formula (from spec §3), for reference in every task

```
D_ppm(v, k)  : diminishing multiplier in ppm ∈ [0, 1_000_000], = 1_000_000 at v=1
value(row)   = minutes · price_ppm · region_ppm                 // existing "raw value"
raw(row)     = mul_div(value, D_ppm(plays, k), 1_000_000)       // diminished
cred(row)    = mul_div(raw,   bw_ppm(work),    1_000_000)       // bandwidth-discounted
RW_u         = Σ_row raw(row)          // per-user denominator (bandwidth-free)
sum_cred_u   = Σ_row cred(row)
target_u     = mul_div(fee_u, sum_cred_u, RW_u)   // amount actually paid to works (≤ fee)
credit(row)  = apportion(target_u, cred_row, sum_cred_u)        // largest-remainder split
unallocated += fee_u − target_u
```

---

## File Structure

- Modify: `sims/src/lib.rs` — `UsageRow.plays`, `Dataset.bandwidth_ppm`, `DaprParams`, `mul_div`, `D_ppm`, `allocate` rework, `Reputation` output.
- Modify: `sims/Cargo.toml` — add `ruint` for the 256-bit `mul_div` intermediate.
- Modify: `services/settlement/src/{chain.rs,settle.rs}` — pass params + bandwidth; build `UsageRow` with `plays` (neutral in Task 1, real in Task 2).
- Modify: `libs/wallet-zk/src/commit.rs` — `Opening.plays`, 128-byte pre-image.
- Modify: `libs/wallet-zk/src/zk.rs` — `UsageEntry.plays`.
- Modify: `libs/wallet-zk/src/session.rs` — `SessionState.per_work_plays`, count on `start`, flush `plays`.
- Modify: `clients/browser-ext/core/src/lib.rs` + `clients/browser-ext/src/background.js` — commitment/flush carry `plays`.
- Modify: `clients/player-plugin/src/settle.rs` — `build_openings` carries `plays`.
- Modify: `ops/demo/run_demo.sh` (+ any demo building commitments/openings) — `(work, minutes, plays)` pre-image + openings JSON.
- Create: `sims/src/bin/antifraud_demo.rs` (or `ops/demo/run_antifraud_demo.sh`), `ops/Makefile` target, `.github/workflows/ci.yml` job.

---

## Task 1: `cwe-dapr` — the enriched user-centric model

**Files:**
- Modify: `sims/src/lib.rs`, `sims/Cargo.toml`
- Modify (to stay green): `services/settlement/src/chain.rs`, `services/settlement/src/settle.rs`

**Interfaces:**
- Produces: `DaprParams { diminishing_k_ppm: u64 }` (`Default` = `1_000_000`); `UsageRow.plays: u64`; `Dataset.bandwidth_ppm: BTreeMap<WorkId, u64>` + `Dataset::bw(&self, &WorkId) -> u64` (default `1_000_000`); `Payouts.reputation: BTreeMap<WorkId, Reputation>` where `Reputation { distinct_users: u64, weighted_usage: u128 }`; `allocate(&Dataset, &DaprParams) -> Result<Payouts, DaprError>`; `pub fn d_ppm(plays: u64, k_ppm: u64) -> u64`.

- [ ] **Step 1: Add `ruint` and the `mul_div` helper (overflow-safe)**

In `sims/Cargo.toml` add `ruint = "1"` to `[dependencies]`. In `sims/src/lib.rs`:

```rust
use ruint::aliases::U256;

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
```

- [ ] **Step 2: Write the diminishing multiplier + its test (TDD)**

Add the test first:

```rust
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
}
```

Then implement:

```rust
/// The diminishing-returns multiplier for `plays` repeat plays, in ppm.
///
/// `D(v) = ( Σ_{j=1..v} 1/(1 + k·(j-1)) ) / v`, with `k = k_ppm / 1_000_000`. The
/// `j`-th play is worth `1/(1 + k·(j-1))`, so repeats count for progressively less;
/// the result is the average per-play value, in ppm (`1_000_000` = full weight).
/// `plays == 0` is treated as a single unit (`1_000_000`). Pure integer math, so it
/// is bit-for-bit reproducible.
pub fn d_ppm(plays: u64, k_ppm: u64) -> u64 {
    let v = plays.max(1);
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
```

Run: `cargo test -p cwe-dapr dr_tests -- --nocapture` → PASS.

- [ ] **Step 3: Add the new fields (types) and keep the crate compiling**

- `UsageRow`: add `pub plays: u64,` (document: number of plays this row aggregates; `1` = a single play; used for diminishing returns).
- `Dataset`: add `#[serde(default)] pub bandwidth_ppm: BTreeMap<WorkId, u64>,` and:
  ```rust
  /// The bandwidth credibility for `work` in ppm; `1_000_000` (neutral) when the
  /// bandwidth layer has provided none. A value below neutral discounts payout.
  pub fn bw(&self, work: &WorkId) -> u64 {
      self.bandwidth_ppm.get(work).copied().unwrap_or(1_000_000)
  }
  ```
- `DaprParams`:
  ```rust
  /// Governance-tunable DAPR parameters.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct DaprParams {
      /// Diminishing-returns rate `k`, in ppm (`1_000_000` = k=1.0, the default:
      /// the j-th play is worth 1/j).
      pub diminishing_k_ppm: u64,
  }
  impl Default for DaprParams {
      fn default() -> Self {
          DaprParams { diminishing_k_ppm: 1_000_000 }
      }
  }
  ```
- `Reputation` + `Payouts.reputation`:
  ```rust
  /// A per-work discovery/reputation signal (not a payout term).
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Reputation {
      /// Number of distinct users who consumed the work this epoch.
      pub distinct_users: u64,
      /// Bandwidth-discounted, diminishing-adjusted usage summed across users.
      pub weighted_usage: u128,
  }
  ```
  Add `pub reputation: BTreeMap<WorkId, Reputation>,` to `Payouts`.

- [ ] **Step 4: Write the allocate-rework tests (TDD)**

```rust
#[cfg(test)]
mod h3_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ds(fees: &[(&str, u128)], rows: &[(&str, &str, u64, u64, u64, u64)]) -> Dataset {
        let mut tier_fees = BTreeMap::new();
        for (u, f) in fees { tier_fees.insert(u.to_string(), *f); }
        let usage = rows.iter().map(|(u, w, m, p, r, pl)| UsageRow {
            user: u.to_string(), work: w.to_string(),
            minutes: *m, price_ppm: *p, region_ppm: *r, plays: *pl,
        }).collect();
        Dataset { tier_fees, usage, bandwidth_ppm: BTreeMap::new() }
    }

    /// Neutral params (plays=1, bandwidth default) reproduce the flat split exactly.
    #[test]
    fn neutral_defaults_reproduce_flat_split() {
        let d = ds(&[("u1", 1_000_000)],
                   &[("u1","wA",60,1_000_000,1_000_000,1),
                     ("u1","wB",20,1_000_000,1_000_000,1)]);
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
        let d = ds(&[("u1", 1_000_000)], &[("u1","wA",300,1_000_000,1_000_000,100)]);
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.per_work["wA"], 1_000_000); // full fee, no diminishing penalty
        assert_eq!(p.unallocated, 0);
    }

    /// Low bandwidth discounts a single-work user's payout; the difference is
    /// unallocated (this is the anti-fraud mechanism).
    #[test]
    fn low_bandwidth_discounts_and_conserves() {
        let mut d = ds(&[("u1", 1_000_000)], &[("u1","wA",60,1_000_000,1_000_000,1)]);
        d.bandwidth_ppm.insert("wA".into(), 250_000); // 25% credible
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.per_work["wA"], 250_000);      // paid = fee · bw
        assert_eq!(p.unallocated, 750_000);         // discounted remainder
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// Diminishing returns shifts a multi-work user's split toward the less-replayed
    /// work, while still conserving the fee.
    #[test]
    fn diminishing_shifts_multiwork_split() {
        // Equal minutes/price/region; wB replayed 4×, wA once.
        let d = ds(&[("u1", 1_000_000)],
                   &[("u1","wA",10,1_000_000,1_000_000,1),
                     ("u1","wB",10,1_000_000,1_000_000,4)]);
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert!(p.per_work["wA"] > p.per_work["wB"], "less-replayed work gets more");
        assert_eq!(p.total_to_works() + p.unallocated, 1_000_000);
    }

    /// The reputation signal ranks broad appeal over concentrated replays.
    #[test]
    fn reputation_prefers_breadth() {
        // wBroad: two distinct users, one play each. wDeep: one user, many plays.
        let d = ds(&[("u1",1_000_000),("u2",1_000_000)],
                   &[("u1","wBroad",10,1_000_000,1_000_000,1),
                     ("u2","wBroad",10,1_000_000,1_000_000,1),
                     ("u1","wDeep",10,1_000_000,1_000_000,50)]);
        let p = allocate(&d, &DaprParams::default()).unwrap();
        assert_eq!(p.reputation["wBroad"].distinct_users, 2);
        assert_eq!(p.reputation["wDeep"].distinct_users, 1);
        assert!(p.reputation["wBroad"].weighted_usage > p.reputation["wDeep"].weighted_usage);
    }
}
```

Run to verify they FAIL to compile / fail (allocate signature/behaviour not yet updated).

- [ ] **Step 5: Rework `allocate`**

Replace the body to implement the §3 formula. Keep the existing `apportion` helper (largest-remainder). Sketch:

```rust
pub fn allocate(dataset: &Dataset, params: &DaprParams) -> Result<Payouts, DaprError> {
    let k = params.diminishing_k_ppm;
    let mut rows_by_user: BTreeMap<&UserId, Vec<&UsageRow>> = BTreeMap::new();
    for row in &dataset.usage {
        rows_by_user.entry(&row.user).or_default().push(row);
    }

    let mut per_work: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut unallocated: u128 = 0;
    // Reputation accumulators.
    let mut rep_usage: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut rep_users: BTreeMap<WorkId, std::collections::BTreeSet<&UserId>> = BTreeMap::new();

    for (user, fee) in &dataset.tier_fees {
        let rows = rows_by_user.get(user).cloned().unwrap_or_default();

        // Per row: raw (diminished) and cred (bandwidth-discounted).
        let mut raws = Vec::with_capacity(rows.len());
        let mut creds = Vec::with_capacity(rows.len());
        let mut rw_u: u128 = 0;
        let mut sum_cred: u128 = 0;
        for row in &rows {
            let value = row.value()?;                                  // minutes·price·region
            let raw = mul_div(value, d_ppm(row.plays, k) as u128, 1_000_000)?;
            let cred = mul_div(raw, dataset.bw(&row.work) as u128, 1_000_000)?;
            rw_u = rw_u.checked_add(raw).ok_or(DaprError::Overflow)?;
            sum_cred = sum_cred.checked_add(cred).ok_or(DaprError::Overflow)?;
            raws.push(raw);
            creds.push(cred);
            // Reputation uses cred (bandwidth+diminishing adjusted).
            *rep_usage.entry(row.work.clone()).or_insert(0) =
                rep_usage[&row.work].checked_add(cred).ok_or(DaprError::Overflow)?;
            rep_users.entry(row.work.clone()).or_default().insert(user);
        }

        // No attributable raw value → whole fee unallocated.
        if rw_u == 0 || sum_cred == 0 {
            unallocated = unallocated.checked_add(*fee).ok_or(DaprError::Overflow)?;
            continue;
        }

        // Amount actually paid to works ≤ fee; the rest (bandwidth discount) unallocated.
        let target = mul_div(*fee, sum_cred, rw_u)?;
        unallocated = unallocated
            .checked_add(fee.checked_sub(target).ok_or(DaprError::Overflow)?)
            .ok_or(DaprError::Overflow)?;

        // Split `target` across rows by cred, exactly (largest remainder).
        let shares = apportion(target, &creds, sum_cred)?;
        for (row, share) in rows.iter().zip(shares) {
            let e = per_work.entry(row.work.clone()).or_insert(0);
            *e = e.checked_add(share).ok_or(DaprError::Overflow)?;
        }
    }

    let reputation = rep_usage.into_iter().map(|(w, wu)| {
        let du = rep_users.get(&w).map(|s| s.len() as u64).unwrap_or(0);
        (w, Reputation { distinct_users: du, weighted_usage: wu })
    }).collect();

    Ok(Payouts { per_work, unallocated, reputation })
}
```

> Note: the `rep_usage[&row.work]` re-read is illustrative; implement without a double-borrow (accumulate into a local then insert). Keep `apportion` exactly as-is.

Run: `cargo test -p cwe-dapr` → all `dr_tests` + `h3_tests` + the **existing** fixture tests PASS (the existing tests build `UsageRow` — update their constructor to pass `plays: 1`, and any `Dataset` literal to add `bandwidth_ppm: BTreeMap::new()`; their asserted values must be unchanged, proving neutral-default equivalence).

- [ ] **Step 6: Keep settlement compiling (neutral plays; real plays come in Task 2)**

In `services/settlement/src/chain.rs`, the `UsageRow { .. }` construction gains `plays: 1` (a neutral literal — Task 2 replaces it with the opening's real plays), and the `Dataset { tier_fees, usage }` construction gains `bandwidth_ppm: BTreeMap::new()`. In `services/settlement/src/settle.rs`, the `allocate(dataset)` call becomes `allocate(dataset, &cwe_dapr::DaprParams::default())` (and update the settle tests' `Dataset`/`UsageRow` literals identically). Add a `// TODO(H3 Task 2): carry the opening's real play count` note at the `plays: 1` site.

Run: `cargo test -p cwe-dapr -p cwe-settlement && cargo clippy -p cwe-dapr -p cwe-settlement -- -D warnings && cargo fmt -p cwe-dapr -p cwe-settlement -- --check` → all green.

- [ ] **Step 7: Commit**

```bash
git add sims/ services/settlement/ Cargo.lock
git commit -m "dapr: user-centric DAPR model — diminishing returns, bandwidth discount, reputation"
```

---

## Task 2: bind `plays` end-to-end

**Files:**
- Modify: `libs/wallet-zk/src/commit.rs`, `libs/wallet-zk/src/zk.rs`, `libs/wallet-zk/src/session.rs`
- Modify: `services/settlement/src/chain.rs` (+ disclosure), `services/settlement/src/disclosure.rs` if it names opening fields
- Modify: `clients/browser-ext/core/src/lib.rs`, `clients/browser-ext/src/background.js`
- Modify: `clients/player-plugin/src/settle.rs`
- Modify: `ops/demo/run_demo.sh` and any other demo that builds a commitment/openings

**Interfaces:**
- `Opening::new(work_id, minutes, plays, salt)`; pre-image is 128 bytes `work‖minutes_be32‖plays_be32‖salt`; `Opening.plays: u64`.
- `UsageEntry.plays: u64`; `SessionStore::flush` returns entries carrying `plays`; a `start` on a work counts one play.

- [ ] **Step 1: `Opening` binds plays (TDD)**

Add a test to `commit.rs` first:

```rust
#[test]
fn commitment_binds_plays() {
    let o1 = Opening::new(Bytes32([1;32]), 60, 3, Bytes32([9;32]));
    let o2 = Opening::new(Bytes32([1;32]), 60, 4, Bytes32([9;32])); // only plays differ
    assert_ne!(o1.commit(), o2.commit(), "plays must be bound");
    assert!(o1.verify(&o1.commit()));
}
```

Then: add `pub plays: u64,` to `Opening`; change `new` to `new(work_id, minutes, plays, salt)`; extend the pre-image to 128 bytes:

```rust
let mut preimage = [0u8; 128];
preimage[0..32].copy_from_slice(self.work_id.as_bytes());
preimage[56..64].copy_from_slice(&self.minutes.to_be_bytes());  // minutes in word 2 low bytes
preimage[88..96].copy_from_slice(&self.plays.to_be_bytes());    // plays in word 3 low bytes
preimage[96..128].copy_from_slice(self.salt.as_bytes());        // salt in word 4
```

Update the module doc comment to describe the 128-byte layout. Update every `Opening::new(...)` call in `wallet-zk`'s own tests to pass a plays argument.

- [ ] **Step 2: `UsageEntry.plays` + session play-count (TDD)**

Add a test to `session.rs`: two `start`s on the same work then flush yields `plays == 2`; minutes still floor correctly. Then:
- `zk.rs`: add `pub plays: u64,` to `UsageEntry`.
- `session.rs`: add `#[serde(default)] pub per_work_plays: BTreeMap<Bytes32, u64>` to `SessionState`; in `start`, increment `per_work_plays[work] += 1` (each start is one play); in `flush`, pair each work's floored minutes with its play count into `UsageEntry { work_id, minutes, plays }` and clear both maps.

Run: `cargo test -p cwe-wallet-zk` → green (including the new tests).

- [ ] **Step 3: settlement carries real plays**

In `chain.rs`, replace the Task-1 `plays: 1` with the opening's `opening.plays` (the disclosure openings now carry it via `Opening`'s serde). Confirm the commitment check still passes (it recomputes `opening.commit()`, which now binds plays). Update the settlement fixtures/tests that build `Opening`/`UsageEntry` to include plays. Neutral case (plays=1) still reproduces prior payouts.

Run: `cargo test -p cwe-settlement` → green.

- [ ] **Step 4: clients carry plays**

- `clients/browser-ext/core/src/lib.rs`: `commitment(work_id_hex, minutes, plays, salt_hex)` (add `plays: u32`, widen to u64 internally, pass to `Opening::new`); `WasmSession::flush` already returns `{work_id, minutes, plays}` from the updated store — ensure the JSON includes plays.
- `clients/browser-ext/src/background.js`: in `handleSettle`, read `u.plays` from the flushed usage and pass it to `commitment(...)`; include `plays` in each `openings` entry.
- `clients/player-plugin/src/settle.rs`: `build_openings` maps `UsageEntry.plays` into `Opening::new(work, minutes, plays, salt)`. (`UsageEntry` now carries plays, so this is a one-field threading.)

Run: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + rebuild the wasm core (`cd clients/browser-ext && npm run build && npm test`) → green.

- [ ] **Step 5: update the demos' commitment/openings shape**

In `ops/demo/run_demo.sh`, the `commit()` helper hashes `work ‖ minutes_be32 ‖ salt`; change it to `work ‖ minutes_be32 ‖ plays_be32 ‖ salt` (a 128-byte pre-image via `cast keccak $(cast concat-hex work $(cast to-uint256 minutes) $(cast to-uint256 plays) salt)`), pass a plays value per usage, and add `"plays": <n>` to each opening in the disclosure JSON. Apply the same change to any other demo that builds a commitment or an openings/disclosure file (grep `submitConsumption`/`disclosure`/`commit` across `ops/demo/`).

Run each touched demo (`make -C ops demo`, and any other affected) → all print their PASS line. Do not proceed until green.

- [ ] **Step 6: Commit**

```bash
git add libs/wallet-zk/ services/settlement/ clients/ ops/demo/ Cargo.lock
git commit -m "Bind plays in the usage commitment and thread it through settlement and clients"
```

---

## Task 3: anti-fraud demo + Makefile + CI + docs

**Files:**
- Create: `sims/src/bin/antifraud_demo.rs` (a deterministic Rust demo over `cwe-dapr`), `ops/demo/run_antifraud_demo.sh` (wrapper), or a single `run_antifraud_demo.sh` calling the bin
- Modify: `ops/Makefile`, `.github/workflows/ci.yml`, `sims/README.md` (if present) or a short doc

**Interfaces:** consumes `cwe-dapr` (`allocate`, `DaprParams`, `Dataset`, `UsageRow`, `Reputation`).

- [ ] **Step 1: Write the anti-fraud demo**

A deterministic demo (a `cwe-dapr` bin is simplest and needs no chain) that constructs datasets and asserts the three properties, printing each result:

1. **Fraud is capped (neutral bandwidth):** a creator's work `wF`; `N` puppet users each pay fee `F` and play only `wF` (looping, `plays` high), neutral bandwidth. Assert `per_work["wF"] == N·F` (break-even) and `unallocated == 0` — the creator recovers exactly the puppets' fees, never more.
2. **Fake plays are a strict loss (bandwidth input):** same dataset but `bandwidth_ppm["wF"] = LOW` (e.g. `100_000`). Assert `per_work["wF"] < N·F` (strict loss) and `unallocated == N·F − per_work["wF"]` (the discount is conserved, not lost).
3. **Honest fairness preserved:** a genuine superfan (single niche work, full bandwidth, many plays) still routes their full fee to the artist (`per_work == fee`); a broad work out-ranks a deep one in `reputation.weighted_usage`; `Σ per_work + unallocated == Σ fees` in every case.

Print `✅ ANTIFRAUD DEMO PASSED` on success; a clear `FAIL:` + non-zero exit otherwise.

- [ ] **Step 2: Makefile target + CI job**

`ops/Makefile`: add `antifraud-demo` to `.PHONY` and a target that runs the demo (`cargo run -p cwe-dapr --bin antifraud_demo` or `bash demo/run_antifraud_demo.sh`). `.github/workflows/ci.yml`: add an `antifraud-e2e` job (checkout, Rust toolchain, rust-cache, run the demo) — it needs no Foundry/jq (pure Rust), so it is lighter than the chain demos.

- [ ] **Step 3: Full gate + docs**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && make -C ops antifraud-demo && make -C ops demo && make -C ops hub-demo && make -C ops ownership-demo && make -C ops player-demo && make -C ops arbitration-demo` — all green. (Foundry at `$HOME/.foundry/bin`.)

Add a short note to `sims/README.md` (or create one) describing the H3 model: user-centric split, diminishing returns (`k`), the bandwidth-credibility discount + neutral default, the reputation signal, and the deferred seams (real bandwidth receipts = H5; reputation-into-hub-ranking = fast-follow). Scan every new/changed file for stray agent/assistant attributions.

- [ ] **Step 4: Commit**

```bash
git add sims/ ops/ .github/workflows/ci.yml
git commit -m "Add anti-fraud demo, CI job, and DAPR model docs"
```

---

## Self-Review

**Spec coverage:** the user-centric enriched model — diminishing returns via `plays` (T1 `d_ppm` + `allocate`), bandwidth-credibility discount with neutral default (T1), reputation/diversity output (T1), governance `k` (T1 `DaprParams`), deterministic integer math via `mul_div` (T1); `plays` bound in the commitment + threaded through session/settlement/clients (T2); the anti-fraud demo proving capped-fraud + bandwidth-loss + superfan-fairness (T3). Deferred seams (real bandwidth receipts, reputation-into-hub-ranking, governance influence exponent) are stated, not built — matching the spec.

**Placeholder scan:** the crux math (`mul_div`, `d_ppm`, `allocate`) carries full code; the tests assert concrete values (750000, break-even `N·F`, discounted remainder) computed by hand from the formula; the plumbing is precise per-file edits; the demo is an explicit numbered property list. No "TBD"/"add error handling"/"write tests for the above" remain.

**Type consistency:** `DaprParams`/`UsageRow.plays`/`Dataset.bandwidth_ppm`/`Payouts.reputation` (T1) are consumed by settlement (T1 neutral, T2 real) and the demo (T3); `Opening::new(work, minutes, plays, salt)` and the 128-byte pre-image (T2) are used identically by `wallet-zk`, settlement, both clients, and the demos; `UsageEntry.plays` (T2) flows from the session store through `build_openings`/`handleSettle`; `allocate(&Dataset, &DaprParams)` (T1) is called with `DaprParams::default()` by settlement and with explicit params by the demo. Neutral defaults (`plays=1`, `bandwidth_ppm` absent) reproduce the existing fixtures bit-for-bit, the invariant every task re-checks.
