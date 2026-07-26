//! Turning co-signed bandwidth receipts into per-row bandwidth credibility.
//!
//! This is the aggregator's half of the H5 receipt protocol. `cwe-receipt` proves
//! a receipt was co-signed and unaltered; everything that makes a receipt
//! *count* lives here: it must be bound to the epoch being settled, its node must
//! hold a valid storage-node credential, and its `(consumer, work_id,
//! chunk_index)` content-position key must not have been seen before. Surviving
//! receipts are summed per (user, work), compared against what that row's
//! proven weight implies the user should have had to download, and then
//! discounted again by each user's absolute per-epoch evidence floor.

use std::collections::{BTreeMap, BTreeSet};

use cwe_dapr::RawRow;
use cwe_receipt::{normalize_addr, Receipt, ReceiptBundle, SignedReceipt, CHUNK_SIZE};

/// The denominator of the `RATE(W)` rate constant: a rate is expressed as bytes
/// per `RATE_SCALE` units of proven weight.
///
/// Proven weights are `minutes · price_ppm · region_ppm · D(plays) / 1e6`, so
/// they run to ~10^13 for an hour of content — an integer "bytes per unit
/// weight" would always floor to zero. 10^12 of weight is exactly one minute of
/// a full-price (`price_ppm = 1e6`), neutral-region, first-play work, which makes
/// a rate read naturally as **bytes per minute of full-price content** (128 kbps
/// audio ≈ 960_000).
pub const RATE_SCALE: u128 = 1_000_000_000_000;

/// Verify a receipt bundle and sum the accepted bytes per (consumer, work).
///
/// A receipt is counted only if ALL of the following hold:
/// 1. both signatures recover to the addresses it names (`SignedReceipt::verify`);
/// 2. it is bound to `epoch` — a receipt from another epoch is a replay attempt;
/// 3. its node satisfies `credentialed`, i.e. holds a valid, non-revoked
///    storage-node credential. This is what stops a fraudster standing up their
///    own "node" and co-signing fabricated receipts with a colluding client;
/// 4. its `(consumer, work_id, chunk_index)` content-position key has not
///    already been counted in this bundle.
///
/// Rejections are reported on stderr and simply not counted; one bad receipt
/// never invalidates a bundle, it just fails to earn credibility. Keys are the
/// lowercase `0x` (consumer, work) pair, matching [`RawRow`]'s `user`/`work`.
///
/// Since Task 1 redefined [`Receipt::dedup_key`] to `(consumer, work_id,
/// chunk_index)` — content position rather than session position — this
/// function's behaviour changed with no signature edit needed here: the same
/// block served by two different nodes now collides (it is the same content
/// either way), and re-fetching one block under a fresh request can no longer
/// mint additional evidence. See the `same_chunk_from_two_nodes_counts_once`,
/// `refetching_one_chunk_credits_it_once` and `distinct_chunks_accumulate`
/// tests below.
///
/// Each accepted receipt's `bytes` is clamped to [`CHUNK_SIZE`] before it is
/// added to the running total. Dedup caps the number of DISTINCT chunk
/// indices a (consumer, work) pair can accumulate, but `chunk_index` and
/// `bytes` are both attacker-controlled `u64`s with nothing else constraining
/// their relationship — without this clamp, one credentialed-but-colluding
/// node could co-sign a single receipt naming `chunk_index: 0` and an
/// arbitrarily large `bytes`, clearing the entire per-work cap (and the
/// absolute per-epoch evidence floor) in one receipt. An honest chunk's true
/// size never exceeds `CHUNK_SIZE` (only a work's final chunk is ever
/// SHORTER), so the clamp costs nothing on any genuine receipt and is what
/// makes the "total credit is capped at the work's content size" property
/// (spec §3.1) real rather than nominal.
pub fn accept_receipts(
    bundle: &ReceiptBundle,
    epoch: u64,
    credentialed: &dyn Fn(&str) -> bool,
) -> BTreeMap<(String, String), u128> {
    let mut totals: BTreeMap<(String, String), u128> = BTreeMap::new();
    // Every dedup key counted so far this bundle; a repeat is dropped outright.
    let mut seen: BTreeSet<(String, String, u64)> = BTreeSet::new();

    for signed in &bundle.receipts {
        let r: &Receipt = &signed.receipt;

        // (1) The receipt must be intact and co-signed by exactly the two
        // parties it names.
        if let Err(e) = SignedReceipt::verify(signed) {
            eprintln!("warning: dropping receipt (bad signature): {e}");
            continue;
        }

        // (2) Epoch binding: a receipt earned in another epoch cannot be
        // re-spent in this one.
        if r.epoch != epoch {
            eprintln!(
                "warning: dropping receipt from node {} (epoch {} != settlement epoch {epoch})",
                r.node, r.epoch
            );
            continue;
        }

        // (3) Only credentialed storage nodes' attestations count.
        if !credentialed(&normalize_addr(&r.node)) {
            eprintln!(
                "warning: dropping receipt from node {} (no valid storage-node credential)",
                r.node
            );
            continue;
        }

        // (4) Anti-replay within the bundle: a repeat of the same content block
        // for the same consumer, whichever node re-served it.
        let key = r.dedup_key();
        if !seen.insert(key) {
            eprintln!(
                "warning: dropping replayed receipt (consumer {}, work {}, chunk {})",
                r.consumer, r.work_id, r.chunk_index
            );
            continue;
        }

        // Accepted: attribute the bytes to this (consumer, work) pair, clamped
        // to one chunk's worth. Without this clamp `bytes` is unbounded and
        // unrelated to `chunk_index`, so a single colluding receipt could claim
        // an arbitrarily large amount and blow through the per-work cap dedup
        // is supposed to enforce.
        let entry = totals
            .entry((normalize_addr(&r.consumer), r.work_id.to_ascii_lowercase()))
            .or_insert(0);
        *entry = entry.saturating_add(r.bytes.min(CHUNK_SIZE) as u128);
    }

    totals
}

/// Each user's per-epoch evidence factor in ppm, from their DEDUPED verified
/// bytes summed across every work.
///
/// ```text
/// epoch_factor(U) = clamp(total_verified_bytes(U) · 1e6 / MIN_EPOCH_BYTES, 0, 1e6)
/// ```
///
/// This is the absolute floor beneath the per-row ratio, and it is what closes
/// the dust-weight gap: the ratio alone can be satisfied by a single byte when
/// the claimed weight is small, because expectation scales with weight. An
/// absolute floor does not scale, so a claimant routing a whole tier fee must
/// have received a real quantity of content this epoch whatever they claimed.
///
/// A genuinely light user is unaffected — one 30-second play moves far more than
/// the floor — which is the point: a light subscriber is legitimate, and the
/// deterrent must fall on claims backed by nothing, not on modest consumption.
pub fn epoch_factor_ppm(
    verified: &BTreeMap<(String, String), u128>,
    min_epoch_bytes: u64,
) -> BTreeMap<String, u64> {
    // Sum each user's evidence across all their works.
    let mut totals: BTreeMap<String, u128> = BTreeMap::new();
    for ((user, _work), bytes) in verified {
        let entry = totals.entry(normalize_addr(user)).or_insert(0);
        *entry = entry.saturating_add(*bytes);
    }

    // A zero floor would divide by zero; treat it as "no floor configured" and
    // leave every user neutral rather than crediting or burning arbitrarily.
    if min_epoch_bytes == 0 {
        return totals.into_keys().map(|u| (u, 1_000_000)).collect();
    }

    totals
        .into_iter()
        .map(|(user, bytes)| {
            let ppm = mul_div_floor(bytes, 1_000_000, min_epoch_bytes as u128);
            (user, std::cmp::min(ppm, 1_000_000) as u64)
        })
        .collect()
}

/// Compute each row's bandwidth credibility in ppm from the verified byte totals.
///
/// Two stages apply in sequence:
///
/// 1. **Per-row ratio** — does this row's own claimed weight match the bytes
///    verified for its (user, work) pair?
///    ```text
///    expected_bytes = raw · rate(work) / RATE_SCALE
///    row_ratio      = clamp(verified_bytes · 1e6 / expected_bytes, 0, 1e6)
///    ```
/// 2. **Per-user absolute floor** — regardless of how well a row's ratio is
///    satisfied, it is scaled again by [`epoch_factor_ppm`]: a user whose total
///    deduped evidence this epoch falls short of `MIN_EPOCH_BYTES` has every row
///    discounted further, because a small claimed weight can satisfy stage 1
///    with almost no real bytes.
///
/// Two edge cases in stage 1 matter, and they resolve in OPPOSITE directions on
/// purpose (spec §4.1):
///
/// * A work with **no configured rate, or a rate of zero**, FAILS CLOSED to
///   credibility `0`. Treating it as neutral would mean whoever controls the
///   rate could switch the bandwidth discount off entirely — a puppet-work
///   fraudster would simply publish `rate = 0` and collect in full. A
///   misconfiguration must cost the claimant, not the system, and it is logged
///   so it is loud rather than silent.
/// * A **genuinely zero-weight row** has no claim of its own to discount, but it
///   still belongs to a user whose epoch evidence may be thin — so instead of a
///   flat neutral it takes exactly that user's `epoch_factor`, folding in stage
///   2 the same as every other row. A NONZERO row's expectation is floored at
///   `1` byte rather than allowed to round down to zero — otherwise a
///   dust-weight claim (too small for the integer division to register any
///   expected bytes) would slip through as neutral while contributing nothing,
///   inverting the fee-conservation property this module exists to protect:
///   the optimal fraud would become "claim less", not "claim honestly."
///
/// A user ABSENT from `epoch_factor` entirely has supplied no verified bytes at
/// all this epoch and fails closed to `0` — never a default neutral — on every
/// one of their rows, including the missing/zero-rate early return (already `0`)
/// and the zero-weight row (which now reads `0` from the missing lookup rather
/// than assuming neutral).
///
/// The clamp at neutral in stage 1 means over-serving buys no extra credit —
/// bandwidth can only ever discount a payout, never inflate one.
pub fn row_credibility_ppm(
    rows: &[RawRow],
    verified: &BTreeMap<(String, String), u128>,
    rates: &BTreeMap<String, u64>,
    epoch_factor: &BTreeMap<String, u64>,
) -> Vec<u64> {
    rows.iter()
        .map(|row| {
            let work = row.work.to_ascii_lowercase();
            // This user's absolute per-epoch evidence factor. Absent means no
            // verified bytes were attributed to them at all this epoch, so they
            // fail closed at zero rather than defaulting to neutral.
            let factor = epoch_factor
                .get(&normalize_addr(&row.user))
                .copied()
                .unwrap_or(0);

            // Fail closed on a missing or zero rate.
            let rate = match rates.get(&work).copied() {
                Some(r) if r > 0 => r as u128,
                _ => {
                    eprintln!(
                        "warning: work {work} has no configured bandwidth rate; \
                         crediting row for user {} as zero-credibility",
                        row.user
                    );
                    return 0;
                }
            };

            // A genuinely zero-weight row has nothing of its own to discount,
            // but it still belongs to a user whose epoch evidence may be thin —
            // so it takes that user's epoch factor rather than a flat neutral.
            if row.raw == 0 {
                return factor;
            }
            // Any NONZERO claim must be backed by at least one byte: floor the
            // expectation but never to zero, or a dust-weight claim slips below
            // the resolution of the integer division and collects full credit
            // having moved nothing at all.
            let expected = mul_div_floor(row.raw, rate, RATE_SCALE).max(1);

            let bytes = verified
                .get(&(normalize_addr(&row.user), work))
                .copied()
                .unwrap_or(0);
            // Ratio in ppm, clamped at neutral.
            let ppm = mul_div_floor(bytes, 1_000_000, expected);
            let row_ppm = std::cmp::min(ppm, 1_000_000) as u64;
            // Fold in the user's absolute per-epoch evidence factor. A user with
            // no entry has supplied no verified bytes at all, so they fail
            // closed at zero rather than defaulting to neutral.
            mul_div_floor(row_ppm as u128, factor as u128, 1_000_000) as u64
        })
        .collect()
}

/// `floor(a · b / d)`, computed as `(a/d)·b + ((a%d)·b)/d` to avoid the
/// overflow a plain `a * b` would hit when `a` is a proven weight (~2^120) and
/// `b` a rate (~2^64).
///
/// This is a HELPER FOR THIS MODULE'S FOUR CALL SITES, not a general-purpose
/// `mul_div`: the identity `(a/d)·b + ((a%d)·b)/d` is only an exact floor while
/// `(a mod d)·b < 2^128`, and both intermediate `saturating_mul`s can silently
/// clip on arbitrary inputs where that does not hold (a bignum-reference fuzz
/// found ~24% of unconstrained `(a, b, d)` triples computed wrong). All four
/// current callers satisfy the precondition: `row_credibility_ppm`'s `expected`
/// computation has `r = a % RATE_SCALE < 1e12` and `b = rate ≤ 2^64`; its `ppm`
/// computation has `r = bytes % expected ≤ bytes` and `b = 1_000_000`; its final
/// fold has `r = row_ppm % 1_000_000 < 1e6` (both operands already clamped to
/// `≤ 1e6`) and `b = factor ≤ 1e6`; `epoch_factor_ppm`'s ratio has
/// `r = bytes % min_epoch_bytes < min_epoch_bytes ≤ 2^64` and `b = 1_000_000`.
/// Saturating arithmetic on the outer add means an absurd (but
/// precondition-respecting) input degrades to `u128::MAX` — for the `expected`
/// call site specifically, that reads as an unmeetable expectation and so zero
/// credibility, not a wrapped small number that would hand out free credit —
/// but this degrade-to-`MAX` reading does NOT generalise to arbitrary callers;
/// do not reuse this function outside `row_credibility_ppm` and
/// `epoch_factor_ppm` without re-deriving the precondition.
fn mul_div_floor(a: u128, b: u128, d: u128) -> u128 {
    let q = a / d;
    let r = a % d;
    q.saturating_mul(b).saturating_add(r.saturating_mul(b) / d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    /// Lowercase hex of a byte slice.
    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A canonical work id for the tests.
    fn work() -> String {
        format!("0x{}", "aa".repeat(32))
    }

    /// Co-sign a receipt for `bytes` at `chunk_index` in `epoch`.
    fn signed(
        node: &PrivateKeySigner,
        consumer: &PrivateKeySigner,
        bytes: u64,
        epoch: u64,
        chunk_index: u64,
    ) -> SignedReceipt {
        let receipt = Receipt {
            work_id: work(),
            consumer: format!("{:#x}", consumer.address()),
            node: format!("{:#x}", node.address()),
            chunk_index,
            bytes,
            epoch,
        };
        let msg = receipt.canonical_bytes().unwrap();
        SignedReceipt {
            node_sig: format!(
                "0x{}",
                hex_of(&node.sign_message_sync(&msg).unwrap().as_bytes())
            ),
            consumer_sig: format!(
                "0x{}",
                hex_of(&consumer.sign_message_sync(&msg).unwrap().as_bytes())
            ),
            receipt,
        }
    }

    /// Accept every node (the "all credentialed" predicate).
    fn all_ok(_node: &str) -> bool {
        true
    }

    /// Valid receipts from a credentialed node sum per (user, work).
    #[test]
    fn sums_verified_bytes_per_user_and_work() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 2000, 5, 1),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(3000));
    }

    /// A receipt from an UNCREDENTIALED node contributes nothing — the check
    /// that stops a fraudster spinning up their own node to co-sign fakes.
    #[test]
    fn drops_receipts_from_an_uncredentialed_node() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 1000, 5, 0)],
        };
        let got = accept_receipts(&bundle, 5, &|_n: &str| false);
        assert!(got.is_empty());
    }

    /// Two receipts for the SAME chunk of the same work to the same consumer,
    /// served by DIFFERENT nodes, count once — it is the same content. Cycle 1
    /// keyed on the node and double-counted this.
    #[test]
    fn same_chunk_from_two_nodes_counts_once() {
        let node_a = PrivateKeySigner::random();
        let node_b = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node_a, &consumer, 1000, 5, 0),
                signed(&node_b, &consumer, 1000, 5, 0),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// Re-fetching the same chunk cannot amplify credit, however many receipts
    /// are produced.
    #[test]
    fn refetching_one_chunk_credits_it_once() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let receipts = (0..50)
            .map(|_| signed(&node, &consumer, 1000, 5, 0))
            .collect();
        let got = accept_receipts(&ReceiptBundle { epoch: 5, receipts }, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// Distinct chunks are distinct evidence and accumulate.
    #[test]
    fn distinct_chunks_accumulate() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 1000, 5, 1),
                signed(&node, &consumer, 500, 5, 2),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(2500));
    }

    /// A receipt claiming more than one chunk's worth of bytes is clamped to
    /// `CHUNK_SIZE`. Without this clamp, a single colluding receipt naming
    /// `chunk_index: 0` and a huge `bytes` could claim far more than a work's
    /// actual size in one shot — dedup only bounds the number of DISTINCT
    /// chunk indices, not what each one is allowed to claim.
    #[test]
    fn oversized_chunk_claim_is_clamped_to_chunk_size() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 131_072_000, 5, 0)],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(
            got.get(&key).copied(),
            Some(cwe_receipt::CHUNK_SIZE as u128)
        );
    }

    /// A replayed chunk (same `(consumer, work_id, chunk_index)` dedup key) is
    /// counted once, even if the replay claims more bytes. The node is
    /// deliberately not part of the key, so this holds however many times, and
    /// from whichever node, the same block is re-served.
    #[test]
    fn drops_a_replayed_chunk() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 9_000_000, 5, 0), // same dedup key
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// A receipt bound to a different epoch is dropped.
    #[test]
    fn drops_a_receipt_from_another_epoch() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 1000, 4, 0)],
        };
        assert!(accept_receipts(&bundle, 5, &all_ok).is_empty());
    }

    /// A receipt whose signature no longer matches its contents is dropped.
    #[test]
    fn drops_a_tampered_receipt() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let mut r = signed(&node, &consumer, 1000, 5, 0);
        r.receipt.bytes = 5_000_000; // inflate after signing
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![r],
        };
        assert!(accept_receipts(&bundle, 5, &all_ok).is_empty());
    }

    /// Bytes meeting or exceeding expectation give full credibility; the clamp
    /// means over-serving buys no extra credit.
    #[test]
    fn full_credibility_when_bytes_meet_expectation() {
        // raw = 1e12 → expected = rate bytes exactly.
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &full),
            vec![1_000_000]
        );

        // Ten times the bytes still clamps to neutral.
        let mut over = BTreeMap::new();
        over.insert(("0xu".to_string(), work()), 81_920u128);
        assert_eq!(
            row_credibility_ppm(&rows, &over, &rates, &full),
            vec![1_000_000]
        );
    }

    /// Half the expected bytes gives half credibility.
    #[test]
    fn partial_bytes_give_proportional_credibility() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 4096u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &full),
            vec![500_000]
        );
    }

    /// No receipts at all → zero credibility → a strict loss downstream.
    #[test]
    fn no_bytes_gives_zero_credibility() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates, &full),
            vec![0]
        );
    }

    /// A MISSING or ZERO rate FAILS CLOSED (credibility 0), never neutral —
    /// otherwise whoever controls the rate could switch the discount off (§4.1).
    #[test]
    fn missing_or_zero_rate_fails_closed() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 99_999u128);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);

        // Missing entirely.
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &BTreeMap::new(), &full),
            vec![0]
        );

        // Explicitly zero.
        let mut zero = BTreeMap::new();
        zero.insert(work(), 0u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &zero, &full), vec![0]);
    }

    /// A zero-WEIGHT row has no expectation to fall short of, so it stays
    /// neutral (there is nothing to discount) — as long as the user's own
    /// epoch evidence is neutral too; see `epoch_factor_discounts_a_satisfied_row`
    /// and `missing_epoch_factor_fails_closed` for the case where it is not.
    #[test]
    fn zero_weight_row_is_neutral() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 0,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates, &full),
            vec![1_000_000]
        );
    }

    /// A zero-WEIGHT row belonging to a user with NO epoch evidence at all
    /// takes that user's (missing → zero) epoch factor, not a flat neutral.
    /// This pins the `return factor` branch introduced alongside
    /// `epoch_factor_ppm`: `zero_weight_row_is_neutral` above cannot tell this
    /// apart from the old `return 1_000_000` behaviour because it passes an
    /// all-neutral factor map, under which both branches produce the same
    /// number. This test uses an EMPTY factor map instead, so it fails if the
    /// zero-weight branch is ever reverted to a flat neutral.
    #[test]
    fn zero_weight_row_with_no_epoch_evidence_is_zero() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 0,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates, &BTreeMap::new()),
            vec![0]
        );
    }

    /// A DUST-WEIGHT (nonzero) row whose expectation would floor to zero under
    /// plain integer division must NOT collect full credit with no evidence —
    /// that would make "claim less weight" the optimal fraud, inverting the
    /// whole point of this module. With a realistic rate (128 kbps audio,
    /// `960_000` bytes per minute of full-price content) and `raw = 1_000_000`
    /// (well under one weight-unit's worth of minutes), the naive
    /// `floor(raw · rate / RATE_SCALE)` is `0`; the fix floors `expected` at
    /// `1` instead, so zero verified bytes still yields zero credibility.
    #[test]
    fn dust_weight_row_with_no_evidence_is_not_neutral() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 960_000u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates, &full),
            vec![0]
        );
    }

    /// At this weight, `expected` floors (via the `.max(1)` guard) to a single
    /// byte, so ONE byte of evidence already meets the whole expectation and
    /// earns full (`1_000_000` ppm) credibility — not a small discount. This
    /// is a KNOWN LIMITATION of the linear ratio model, not a desirable
    /// property: below the point where expected bytes would floor to zero,
    /// the model cannot express "the claimant provided less evidence than a
    /// full-weight claim would need," because there is no room left under one
    /// byte to discount into. This residual is what `epoch_factor_ppm` closes
    /// from the caller's side (see `epoch_factor_discounts_a_satisfied_row`
    /// below): this test passes a NEUTRAL (`&full`) factor on purpose, to pin
    /// the per-row ratio's own boundary behaviour in isolation from that
    /// absolute floor, so a future change to either stage cannot silently drift.
    #[test]
    fn dust_weight_expectation_floors_to_one_byte() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 1u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 960_000u64);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &full),
            vec![1_000_000],
            "one byte meets the floored-at-1 expectation and earns full credit"
        );
    }

    /// Credibility is per-(user, work): one user's bytes do not credit another's
    /// row on the same work.
    #[test]
    fn credibility_is_per_user_and_work() {
        let rows = vec![
            RawRow {
                user: "0xu1".into(),
                work: work(),
                raw: 1_000_000_000_000,
            },
            RawRow {
                user: "0xu2".into(),
                work: work(),
                raw: 1_000_000_000_000,
            },
        ];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu1".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        let mut full = BTreeMap::new();
        full.insert("0xu1".to_string(), 1_000_000u64);
        full.insert("0xu2".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &full),
            vec![1_000_000, 0]
        );
    }

    /// A very large weight does not overflow the expected-bytes computation.
    #[test]
    fn large_weight_does_not_overflow() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1u128 << 118,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), u64::MAX);
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        // Expected is astronomically larger than any real byte count, so this is
        // a strict loss — but it must COMPUTE, not panic or wrap.
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates, &full),
            vec![0]
        );
    }

    /// A user meeting the epoch floor is undiscounted; one below it is scaled
    /// proportionally; one with nothing is zero.
    #[test]
    fn epoch_factor_scales_below_the_floor() {
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xfull".to_string(), work()), 131_072);
        verified.insert(("0xhalf".to_string(), work()), 65_536);
        verified.insert(("0xdust".to_string(), work()), 1);

        let f = epoch_factor_ppm(&verified, 131_072);
        assert_eq!(f.get("0xfull").copied(), Some(1_000_000));
        assert_eq!(f.get("0xhalf").copied(), Some(500_000));
        assert_eq!(f.get("0xdust").copied(), Some(7)); // 1e6/131072, floored
    }

    /// Evidence sums ACROSS works: a user who consumed several works reaches the
    /// floor on their combined bytes, not per work.
    #[test]
    fn epoch_factor_sums_across_works() {
        let other = format!("0x{}", "bb".repeat(32));
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 65_536);
        verified.insert(("0xu".to_string(), other), 65_536);
        assert_eq!(
            epoch_factor_ppm(&verified, 131_072).get("0xu").copied(),
            Some(1_000_000)
        );
    }

    /// Over-delivering does not earn above neutral.
    #[test]
    fn epoch_factor_clamps_at_neutral() {
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 10_000_000);
        assert_eq!(
            epoch_factor_ppm(&verified, 131_072).get("0xu").copied(),
            Some(1_000_000)
        );
    }

    /// The epoch factor multiplies the per-row ratio: a row that satisfies its
    /// own expectation is still discounted if the user's epoch evidence is thin.
    /// This is what closes the dust-weight gap.
    #[test]
    fn epoch_factor_discounts_a_satisfied_row() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);

        // Row ratio alone would be full credit...
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &full),
            vec![1_000_000]
        );

        // ...but with only 8192 of the 131072-byte floor, it is scaled to 6.25%.
        let factor = epoch_factor_ppm(&verified, 131_072);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &factor),
            vec![62_500]
        );
    }

    /// A user absent from the factor map earns nothing — fail closed.
    #[test]
    fn missing_epoch_factor_fails_closed() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 1_000_000_000_000,
        }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &BTreeMap::new()),
            vec![0]
        );
    }
}
