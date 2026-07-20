//! `sign-manifest` — sign a work manifest for submission to the Discovery Hub.
//!
//! Reads a manifest JSON object on stdin, signs its canonical form with the key in
//! `PRIVATE_KEY`, and prints `{ "manifest": <manifest>, "signature": "0x<hex>" }`
//! ready to POST to `/manifests`.

use std::io::Read;
use std::process::ExitCode;

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use cwe_discovery_hub::manifest::WorkManifest;

/// Entry point: read stdin, sign, print, or fail with a clear message.
fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Read the manifest from stdin, sign it, and return the JSON envelope string.
fn run() -> Result<String, String> {
    // Load the signing key from the environment (devnet use only).
    let key = std::env::var("PRIVATE_KEY").map_err(|_| "PRIVATE_KEY not set".to_string())?;
    let signer: PrivateKeySigner = key.parse().map_err(|_| "invalid PRIVATE_KEY".to_string())?;

    // Parse the manifest from stdin.
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    let manifest: WorkManifest = serde_json::from_str(&input).map_err(|e| e.to_string())?;

    // Sign the canonical bytes (EIP-191 personal-sign).
    let bytes = manifest.canonical_bytes().map_err(|e| e.to_string())?;
    let sig = signer
        .sign_message_sync(&bytes)
        .map_err(|e| e.to_string())?;
    let signature = format!("0x{}", hex::encode(sig.as_bytes()));

    // Emit the POST-ready envelope.
    let envelope = serde_json::json!({ "manifest": manifest, "signature": signature });
    serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())
}
