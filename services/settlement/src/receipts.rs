//! Turning co-signed bandwidth receipts into per-row bandwidth credibility.
//!
//! This is the aggregator's half of the H5 receipt protocol. `cwe-receipt` proves
//! a receipt was co-signed and unaltered; everything that makes a receipt
//! *count* lives here: it must be bound to the epoch being settled, its node must
//! hold a valid storage-node credential, and its `(node, session, chunk)` key
//! must not have been seen before. Surviving receipts are summed per
//! (user, work) and compared against what that row's proven weight implies the
//! user should have had to download.

use std::collections::{BTreeMap, BTreeSet};

use cwe_dapr::RawRow;
use cwe_receipt::{normalize_addr, Receipt, ReceiptBundle, SignedReceipt};

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
/// 4. its `(node, session_nonce, chunk_nonce)` key has not already been counted
///    in this bundle.
///
/// Rejections are reported on stderr and simply not counted; one bad receipt
/// never invalidates a bundle, it just fails to earn credibility. Keys are the
/// lowercase `0x` (consumer, work) pair, matching [`RawRow`]'s `user`/`work`.
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

        // (4) Anti-replay within the bundle.
        let key = r.dedup_key();
        if !seen.insert(key) {
            eprintln!(
                "warning: dropping replayed receipt (node {}, session {}, chunk {})",
                r.node, r.session_nonce, r.chunk_nonce
            );
            continue;
        }

        // Accepted: attribute the bytes to this (consumer, work) pair.
        let entry = totals
            .entry((normalize_addr(&r.consumer), r.work_id.to_ascii_lowercase()))
            .or_insert(0);
        *entry = entry.saturating_add(r.bytes as u128);
    }

    totals
}

/// Compute each row's bandwidth credibility in ppm from the verified byte totals.
///
/// ```text
/// expected_bytes = raw · rate(work) / RATE_SCALE
/// credibility    = clamp(verified_bytes · 1e6 / expected_bytes, 0, 1e6)
/// ```
///
/// Two edge cases matter, and they resolve in OPPOSITE directions on purpose
/// (spec §4.1):
///
/// * A work with **no configured rate, or a rate of zero**, FAILS CLOSED to
///   credibility `0`. Treating it as neutral would mean whoever controls the
///   rate could switch the bandwidth discount off entirely — a puppet-work
///   fraudster would simply publish `rate = 0` and collect in full. A
///   misconfiguration must cost the claimant, not the system, and it is logged
///   so it is loud rather than silent.
/// * A **zero-weight row** (or one whose expectation floors to zero) is neutral:
///   there is no claim to discount, so there is nothing to punish.
///
/// The clamp at neutral means over-serving buys no extra credit — bandwidth can
/// only ever discount a payout.
pub fn row_credibility_ppm(
    rows: &[RawRow],
    verified: &BTreeMap<(String, String), u128>,
    rates: &BTreeMap<String, u64>,
) -> Vec<u64> {
    rows.iter()
        .map(|row| {
            let work = row.work.to_ascii_lowercase();

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

            let expected = mul_div_floor(row.raw, rate, RATE_SCALE);
            // Nothing expected → nothing to discount.
            if expected == 0 {
                return 1_000_000;
            }

            let bytes = verified
                .get(&(normalize_addr(&row.user), work))
                .copied()
                .unwrap_or(0);
            // Ratio in ppm, clamped at neutral.
            let ppm = mul_div_floor(bytes, 1_000_000, expected);
            std::cmp::min(ppm, 1_000_000) as u64
        })
        .collect()
}

/// `floor(a · b / d)` without overflowing on large `a`.
///
/// Proven weights reach ~2^120 and rates ~2^64, so a plain `a * b` would wrap.
/// Splitting `a` into quotient and remainder against `d` keeps every
/// intermediate product small enough: `(a/d)·b + ((a%d)·b)/d`. Saturating
/// arithmetic on the outer add means an absurd input degrades to `u128::MAX`
/// (an unmeetable expectation → zero credibility) rather than wrapping to a
/// small number that would hand out free credit.
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

    /// Co-sign a receipt for `bytes` at `chunk` in `epoch`.
    fn signed(
        node: &PrivateKeySigner,
        consumer: &PrivateKeySigner,
        bytes: u64,
        epoch: u64,
        chunk: u64,
    ) -> SignedReceipt {
        let receipt = Receipt {
            work_id: work(),
            consumer: format!("{:#x}", consumer.address()),
            node: format!("{:#x}", node.address()),
            bytes,
            epoch,
            session_nonce: format!("0x{}", "5c".repeat(32)),
            chunk_nonce: chunk,
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

    /// A replayed chunk (same node, session and chunk nonce) is counted once,
    /// even if the replay claims more bytes.
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
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates),
            vec![1_000_000]
        );

        // Ten times the bytes still clamps to neutral.
        let mut over = BTreeMap::new();
        over.insert(("0xu".to_string(), work()), 81_920u128);
        assert_eq!(row_credibility_ppm(&rows, &over, &rates), vec![1_000_000]);
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
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates), vec![500_000]);
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
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates),
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

        // Missing entirely.
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &BTreeMap::new()),
            vec![0]
        );

        // Explicitly zero.
        let mut zero = BTreeMap::new();
        zero.insert(work(), 0u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &zero), vec![0]);
    }

    /// A zero-WEIGHT row has no expectation to fall short of, so it stays
    /// neutral (there is nothing to discount).
    #[test]
    fn zero_weight_row_is_neutral() {
        let rows = vec![RawRow {
            user: "0xu".into(),
            work: work(),
            raw: 0,
        }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates),
            vec![1_000_000]
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
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates),
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
        // Expected is astronomically larger than any real byte count, so this is
        // a strict loss — but it must COMPUTE, not panic or wrap.
        assert_eq!(
            row_credibility_ppm(&rows, &BTreeMap::new(), &rates),
            vec![0]
        );
    }
}
