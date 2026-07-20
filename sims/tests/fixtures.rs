//! Fixture-driven tests for the DAPR reference math.
//!
//! For every input fixture under `sims/fixtures/*.json` (excluding the generated
//! `*_expected.json` oracles) this test:
//!
//! 1. checks the fairness invariant `Σ payouts + unallocated == Σ fees`, and
//! 2. asserts the freshly-computed result equals the committed `*_expected.json`.
//!
//! Point (2) is what keeps the oracle trustworthy: if anyone changes the payout
//! math without regenerating the expected files (`cargo run -p cwe-dapr --bin
//! simulate -- sims/fixtures/<name>.json`), this test fails. WP5's settlement job
//! is diff-tested against the very same oracle files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cwe_dapr::{allocate, Dataset, Payouts};

/// Absolute path to the crate's `fixtures/` directory, resolved from the manifest
/// dir so the test works regardless of the process's current directory.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Return every input fixture path (the `*.json` files that are not oracles).
fn input_fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures directory must exist")
        .map(|entry| entry.expect("readable dir entry").path())
        .filter(|p| {
            // Keep `*.json` inputs; skip the generated `*_expected.json` oracles.
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".json") && !name.ends_with("_expected.json")
        })
        .collect();
    // Sort for a deterministic, readable test order.
    paths.sort();
    assert!(!paths.is_empty(), "expected at least one input fixture");
    paths
}

/// Load and parse a JSON file into any deserialisable type, with a clear panic
/// message identifying the offending file if parsing fails.
fn load<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {path:?}: {e}"))
}

/// Every fixture must conserve value: the fee total is fully distributed to works
/// plus the unallocated bucket, with nothing minted or lost.
#[test]
fn every_fixture_conserves_fees() {
    for path in input_fixtures() {
        let dataset: Dataset = load(&path);
        let payouts = allocate(&dataset).unwrap_or_else(|e| panic!("allocate {path:?}: {e}"));
        assert_eq!(
            payouts.total_to_works() + payouts.unallocated,
            dataset.total_fees(),
            "fairness invariant violated for {path:?}",
        );
    }
}

/// The committed `*_expected.json` oracle for each fixture must exactly match a
/// fresh computation, guarding against silent drift between the math and the
/// oracle that WP5 relies on.
#[test]
fn committed_oracles_match_fresh_computation() {
    for path in input_fixtures() {
        let dataset: Dataset = load(&path);
        let fresh = allocate(&dataset).unwrap_or_else(|e| panic!("allocate {path:?}: {e}"));

        // Derive the sibling `<stem>_expected.json` path.
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("fixture stem");
        let oracle_path = fixtures_dir().join(format!("{stem}_expected.json"));
        assert!(
            oracle_path.exists(),
            "missing oracle {oracle_path:?}; regenerate with the simulate binary",
        );

        let committed: Payouts = load(&oracle_path);
        assert_eq!(fresh, committed, "oracle out of date for {path:?}");
    }
}

/// Per-user apportionment sanity: no work is ever credited a negative or absurd
/// amount, and the map is well-formed. (`u128` precludes negatives; this asserts
/// the map is non-empty for any fixture that has attributable value.)
#[test]
fn payout_maps_are_well_formed() {
    for path in input_fixtures() {
        let dataset: Dataset = load(&path);
        let payouts = allocate(&dataset).unwrap();
        // Reconstruct which works had any positive value; each must appear iff it
        // received credit, and credited works must be a subset of used works.
        let used_works: BTreeMap<&String, ()> =
            dataset.usage.iter().map(|r| (&r.work, ())).collect();
        for work in payouts.per_work.keys() {
            assert!(
                used_works.contains_key(work),
                "credited unused work {work:?} in {path:?}"
            );
        }
    }
}
