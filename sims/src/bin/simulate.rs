//! `simulate` — DAPR simulator command-line entry point (WP2 skeleton).
//!
//! When complete, this binary will: load a usage CSV (e.g. `sims/sample_usage.csv`),
//! run the reference math in [`cwe_dapr`], print a per-work allocation summary,
//! and write the canonical `*_expected.json` oracle that WP5's differential test
//! checks the Rust settlement job against.
//!
//! For now it is a placeholder so the workspace builds; WP2 fills in the body.

/// Program entry point. Currently prints a notice that the simulator is not yet
/// implemented so an accidental early run is unambiguous rather than silent.
fn main() {
    // A clear, non-zero-signal message beats an empty run while WP2 is pending.
    eprintln!("cwe-dapr simulate: not yet implemented (WP2). See the Phase 1 plan.");
}
