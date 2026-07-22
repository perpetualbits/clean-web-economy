//! `simulate` — the DAPR simulator command-line tool.
//!
//! Usage:
//!
//! ```text
//! simulate <fixture.json>
//! ```
//!
//! It loads a [`Dataset`] from the given JSON fixture, runs the reference payout
//! math in [`cwe_dapr::allocate`], prints a human-readable per-work summary, and
//! writes the canonical result next to the input as `<fixture>_expected.json`.
//! That expected-output file is the oracle WP5's settlement job is diff-tested
//! against, so the on-chain payouts must reproduce it exactly.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cwe_dapr::{allocate, DaprParams, Dataset, Payouts};

/// Entry point. Returns a non-zero exit code on any error so the tool is usable
/// as a CI gate rather than failing silently.
fn main() -> ExitCode {
    // Collect arguments, skipping argv[0] (the program name).
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Exactly one argument — the fixture path — is required.
    let [fixture_path] = args.as_slice() else {
        eprintln!("usage: simulate <fixture.json>");
        return ExitCode::FAILURE;
    };

    // Run the pipeline and map any failure to a clear message + failure code.
    match run(Path::new(fixture_path)) {
        Ok(output_path) => {
            println!("wrote {}", output_path.display());
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// Load the fixture, compute payouts, print a summary, and write the expected
/// JSON. Returns the path written on success, or a human-readable error string.
fn run(fixture_path: &Path) -> Result<PathBuf, String> {
    // Read and parse the dataset. Errors are stringified with the path for context.
    let raw = std::fs::read_to_string(fixture_path)
        .map_err(|e| format!("reading {}: {e}", fixture_path.display()))?;
    let dataset: Dataset = serde_json::from_str(&raw)
        .map_err(|e| format!("parsing {}: {e}", fixture_path.display()))?;

    // Run the reference allocation with the default (neutral) DAPR parameters;
    // the simulator has no governance input to override them from.
    let payouts = allocate(&dataset, &DaprParams::default()).map_err(|e| e.to_string())?;

    // Show the operator what happened before writing anything.
    print_summary(&dataset, &payouts);

    // Write `<stem>_expected.json` beside the input fixture. Pretty-printed so the
    // committed oracle is reviewable in diffs.
    let output_path = expected_output_path(fixture_path);
    let serialized =
        serde_json::to_string_pretty(&payouts).map_err(|e| format!("serialising result: {e}"))?;
    std::fs::write(&output_path, serialized + "\n")
        .map_err(|e| format!("writing {}: {e}", output_path.display()))?;

    Ok(output_path)
}

/// Derive the expected-output path `<dir>/<stem>_expected.json` from a fixture
/// path like `<dir>/<stem>.json`.
fn expected_output_path(fixture_path: &Path) -> PathBuf {
    // Fall back to the literal "fixture" stem if the path has none (unusual).
    let stem = fixture_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("fixture");
    let file_name = format!("{stem}_expected.json");
    // Place the output in the same directory as the input.
    match fixture_path.parent() {
        Some(dir) => dir.join(file_name),
        None => PathBuf::from(file_name),
    }
}

/// Print a per-work payout table plus the conservation check, so a human running
/// the simulator can eyeball that fees were fully and fairly distributed.
fn print_summary(dataset: &Dataset, payouts: &Payouts) {
    println!("DAPR simulation");
    println!("  users:        {}", dataset.tier_fees.len());
    println!("  usage rows:   {}", dataset.usage.len());
    println!("  total fees:   {}", dataset.total_fees());
    println!("  per-work payouts:");
    for (work, credit) in &payouts.per_work {
        println!("    {work:<12} {credit}");
    }
    if payouts.unallocated > 0 {
        println!("  unallocated:  {}", payouts.unallocated);
    }
    // Restate the invariant so a broken build is obvious in the output itself.
    println!(
        "  check:        {} (works) + {} (unallocated) = {}",
        payouts.total_to_works(),
        payouts.unallocated,
        payouts.total_to_works() + payouts.unallocated,
    );
}
