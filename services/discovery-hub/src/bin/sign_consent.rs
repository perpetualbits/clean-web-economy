//! `sign-consent` — a payee's consent signature over their split share.
//!
//! Reads `work_id`, `content_id`, `payee`, and `share` (as positional args, falling
//! back to the `WORK_ID`/`CONTENT_ID`/`PAYEE`/`SHARE` env vars), signs the consent
//! digest with the key in `PRIVATE_KEY`, and prints the `0x`-hex EIP-191 signature
//! ready for the registrant to submit alongside the split table.

use std::process::ExitCode;

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use cwe_discovery_hub::manifest::consent_digest;
use cwe_wallet_zk::Bytes32;

/// Entry point: parse inputs, sign, print, or fail with a clear message.
fn main() -> ExitCode {
    match run() {
        Ok(signature) => {
            println!("{signature}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Read the four consent fields (args, then env), sign the digest, and return
/// the `0x`-hex signature string.
fn run() -> Result<String, String> {
    let key = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set".to_string())?;
    let signer: PrivateKeySigner = key.parse().map_err(|_| "invalid PRIVATE_KEY".to_string())?;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let work_id = field(&args, 0, "WORK_ID")?
        .parse::<Bytes32>()
        .map_err(|e| format!("invalid work_id: {e}"))?;
    let content_id = field(&args, 1, "CONTENT_ID")?
        .parse::<Bytes32>()
        .map_err(|e| format!("invalid content_id: {e}"))?;
    let payee = field(&args, 2, "PAYEE")?
        .parse::<Address>()
        .map_err(|e| format!("invalid payee: {e}"))?;
    let share = field(&args, 3, "SHARE")?
        .parse::<u64>()
        .map_err(|e| format!("invalid share: {e}"))?;

    let digest = consent_digest(work_id, content_id, payee, share);
    let sig = signer
        .sign_message_sync(&digest)
        .map_err(|e| e.to_string())?;
    Ok(format!("0x{}", hex::encode(sig.as_bytes())))
}

/// Read the `index`-th positional argument, falling back to the `env_var`
/// environment variable when no positional args were given.
fn field(args: &[String], index: usize, env_var: &str) -> Result<String, String> {
    if let Some(v) = args.get(index) {
        return Ok(v.clone());
    }
    std::env::var(env_var).map_err(|_| format!("{env_var} not set (or pass positional args)"))
}
