//! `cwe-settlement` — command-line entry point for the epoch settlement job.
//!
//! Reads its configuration from the environment (see [`cwe_settlement::config`]),
//! runs one settlement against the configured chain, and exits non-zero on any
//! error so it is safe to use as a scripted devnet/CI step.

use std::process::ExitCode;

use cwe_settlement::chain::run;
use cwe_settlement::config::Config;

/// Async entry point on the Tokio runtime (alloy's provider is async).
#[tokio::main]
async fn main() -> ExitCode {
    // Assemble configuration, then run the settlement, mapping any failure to a
    // clear message and a non-zero exit code.
    match Config::from_env() {
        Ok(cfg) => match run(&cfg).await {
            Ok(settlement) => {
                println!(
                    "settled epoch {}: {} works credited, total {}",
                    settlement.epoch,
                    settlement.entries.len(),
                    settlement.total_credits
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("settlement failed: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("configuration error: {e}");
            ExitCode::FAILURE
        }
    }
}
