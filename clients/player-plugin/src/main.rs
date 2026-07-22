//! `cwe-player` — the desktop player agent CLI.
//!
//! Four one-shot commands over the library modules:
//!   * `play <file>`        — decode, recognise, apply the price cap, accrue time;
//!   * `status`             — print the accrued usage without changing anything;
//!   * `settle`             — submit commitments on-chain and write the disclosure;
//!   * `fingerprint <file>` — print the perceptual fingerprint of a local file.
//!
//! Session state persists to `STATE` between invocations.

use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use cwe_player::config::PlayerConfig;
use cwe_player::recognize::{recognize, ReqwestTransport, Tier};
use cwe_player::session::Session;
use cwe_player::{decode, policy, settle};
use cwe_wallet_zk::Bytes32;

/// Wall-clock seconds since the Unix epoch, anchoring a fresh session's epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse the four subcommands, dispatch to a handler, and map any error to a
/// non-zero exit code — the process itself never panics on a bad invocation.
fn main() -> ExitCode {
    // Manual arg parse keeps the dependency surface small (no clap), matching the
    // other workspace binaries.
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    let result = match cmd {
        Some("play") => match args.get(2) {
            Some(file) => cmd_play(PathBuf::from(file)),
            None => Err("usage: cwe-player play <file>".to_string()),
        },
        Some("status") => cmd_status(),
        Some("settle") => cmd_settle(),
        Some("fingerprint") => match args.get(2) {
            Some(file) => cmd_fingerprint(PathBuf::from(file)),
            None => Err("usage: cwe-player fingerprint <file>".to_string()),
        },
        _ => Err("usage: cwe-player <play <file>|status|settle|fingerprint <file>>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// `play`: decode → recognise → policy → accrue.
fn cmd_play(file: PathBuf) -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let audio = decode::decode(&file).map_err(|e| e.to_string())?;

    // Recognise via the live hub; an unrecognised work is not an error.
    let transport = ReqwestTransport::new();
    let Some(work) = recognize(&cfg.hub_url, &audio, &transport) else {
        println!("unrecognised: nothing accrued for {}", file.display());
        return Ok(());
    };

    // Enforce the price cap before accruing anything.
    if !policy::allows(work.price_per_min, cfg.threshold) {
        println!(
            "blocked: {} exceeds price cap ({} > {:?})",
            work.work_id, work.price_per_min, cfg.threshold
        );
        return Ok(());
    }

    // Accrue the whole file's duration; a fingerprint match is escrow-bound.
    let work_id = Bytes32::from_str(&work.work_id).map_err(|e| e.to_string())?;
    let secs = audio.duration_secs();
    let mut session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    session.accrue(work_id, secs, matches!(work.tier, Tier::Fingerprint));
    session.save(&cfg.state_path).map_err(|e| e.to_string())?;

    let tier = match work.tier {
        Tier::Signed => "signed",
        Tier::Fingerprint => "fingerprint (escrow)",
    };
    println!(
        "accrued {secs}s to {} [{tier}], price {}/min",
        work.work_id, work.price_per_min
    );
    Ok(())
}

/// `fingerprint`: decode a file and print its `fp:<hex>` perceptual fingerprint.
///
/// A real client capability (the same fingerprint `play` computes), it also lets
/// tooling learn the exact fingerprint the agent will produce for a file — e.g.
/// so a hub manifest for an unsigned copy can be ingested with a matching fp.
fn cmd_fingerprint(file: PathBuf) -> Result<(), String> {
    let audio = decode::decode(&file).map_err(|e| e.to_string())?;
    // Reuse the shared fingerprint so this print can never drift from recognition.
    let fp = cwe_fingerprint::Fingerprint::compute(&audio.samples, audio.sample_rate);
    println!("{fp}");
    Ok(())
}

/// `status`: print the session's epoch, per-work minutes, and escrow set.
fn cmd_status() -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    let (epoch, per_work, escrow) = session.snapshot_view();
    println!("epoch {epoch}");
    if per_work.is_empty() {
        println!("  (no usage accrued)");
    }
    for (work, secs) in per_work {
        println!("  {work}: {}m ({secs}s)", secs / 60);
    }
    for work in escrow {
        println!("  escrow-bound: {work}");
    }
    Ok(())
}

/// `settle`: submit commitments on-chain and write the disclosure.
fn cmd_settle() -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let mut session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    let usage = session.flush_usage();
    if usage.is_empty() {
        return Err("nothing to settle (no usage accrued this epoch)".to_string());
    }

    // Fresh random salts hide the minutes behind each on-chain commitment.
    let openings = settle::build_openings(&usage, |_| {
        let mut s = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut s);
        Bytes32(s)
    });
    let escrow_works = session.take_escrow_works();

    // Write the disclosure BEFORE the on-chain submit. The openings carry
    // fresh random salts that live only in this file — if the disclosure
    // write happened after a successful submit and then failed (disk IO),
    // the on-chain commitments would be unclaimable forever, and the write
    // is exactly the step most likely to fail. Deriving the address up
    // front (no tx) lets the disclosure be written first, so the more
    // common failure (IO) is caught before anything irreversible happens.
    let (private_key, _consumption, _tier_id) = cfg.require_chain().map_err(|e| e.to_string())?;
    let user_addr = settle::signer_address(private_key).map_err(|e| e.to_string())?;
    settle::write_disclosure(&cfg.disclosure_path, &user_addr, &openings, &escrow_works)
        .map_err(|e| e.to_string())?;

    // Submit on-chain (async) via a small runtime, then persist the drained state.
    // Residual edge case: if the submit above succeeds but `session.save` below
    // then fails, the flushed usage is gone from memory but not yet committed to
    // disk, so a retry would re-flush stale state and re-submit the same
    // commitments on-chain. This duplicate-submit window is an accepted MVP
    // limitation; production would close it with a persisted per-epoch
    // "submitted" marker written atomically with (or before) the tx.
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (tx, _addr) = rt
        .block_on(settle::submit_consumption(&cfg, &openings))
        .map_err(|e| e.to_string())?;
    session.save(&cfg.state_path).map_err(|e| e.to_string())?;

    println!("settled {} work(s) in tx {tx}", openings.len());
    println!("disclosure -> {}", cfg.disclosure_path.display());
    Ok(())
}
