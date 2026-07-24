//! `zk_submit` — generate a REAL Groth16 usage proof and submit it on-chain.
//!
//! This is the submitter half of the `make zk-demo` capstone: it stands in for
//! a user's wallet/extension, producing a genuine zero-knowledge usage proof
//! with the devnet proving key and driving `CWEConsumption.submitConsumption`
//! against the real on-chain `Groth16Verifier`. Three modes exercise the three
//! acts of the demo:
//!
//! * `--mode honest` — prove an honest sample of usage and submit it. The
//!   on-chain verifier accepts the proof and the submission is recorded; the
//!   emitted `ConsumptionSubmitted` event carries the proven per-work weights
//!   that settlement (event mode) then pays from. Exits 0 on success.
//! * `--mode tamper-digest` — prove honestly, but flip a byte of the SUBMITTED
//!   digest (the proof is left intact). The on-chain verifier recomputes its
//!   pairing against the tampered public input, the pairing fails, and
//!   `submitConsumption` reverts `ProofRejected`. Observing that revert is the
//!   expected outcome, so it exits 0; an unexpected SUCCESS exits non-zero. This
//!   is the "inflation rejected" act: you cannot claim weights the proof does
//!   not attest to.
//! * `--mode row-split` — build two usage rows carrying the SAME `work_id`, an
//!   attempt to bypass the per-work diminishing-returns cap by splitting one
//!   work across rows. The circuit's per-work uniqueness constraint makes that
//!   witness unsatisfiable, so `prove` either errors or yields a proof that
//!   fails self-verification against the devnet verifying key. Either refusal is
//!   the expected outcome (exit 0); a genuinely valid proof would be a soundness
//!   break and exits non-zero. Nothing is ever submitted in this mode.
//!
//! Configuration (environment):
//! * `RPC_URL` — JSON-RPC endpoint (default `http://127.0.0.1:8545`).
//! * `PRIVATE_KEY` — the submitting user's key (required).
//! * `DEPLOYMENTS` — deployment address map (default
//!   `chain/deployments/localhost.json`); `consumption` and `beacon` are read.
//! * `TIER` — the subscription tier id (`bytes32` hex) the demo funded; bound
//!   into the proof and emitted so settlement can read the user's tier fee.
//! * `WORK_ID` — the `bytes32` hex work id the sample usage is attributed to
//!   (default: `cast format-bytes32-string "zkwork"`), matching the work the
//!   demo registered so the proven weight pays a real creator.

// The `sol!`-generated `submitConsumption` binding takes seven parameters (it
// mirrors the contract's seven-arg signature); that is inherent to the ABI, not
// a design smell, so the arity lint is silenced for this bin.
#![allow(clippy::too_many_arguments)]

use std::error::Error;
use std::process::ExitCode;
use std::str::FromStr;

use alloy::primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;

use cwe_zk_circuits::evm::proof_to_evm_calldata;
use cwe_zk_circuits::prove::{prove, verify, UsageRowInput};
use cwe_zk_circuits::setup::{load_pk, load_vk};

// The minimal on-chain surface this submitter touches. `#[sol(rpc)]` generates
// the typed call builders and return decoders.
sol! {
    #[sol(rpc)]
    contract Consumption {
        function currentEpoch() external view returns (uint256);
        function submitConsumption(
            bytes32 tierId,
            bytes32[] commitments,
            bytes32[] pseudonyms,
            bytes32[] workIds,
            uint256[] weights,
            bytes32 digest,
            bytes proof
        ) external;
    }
    #[sol(rpc)]
    contract Beacon {
        function keyFor(uint256 epoch) external view returns (bytes32);
    }
}

/// A boxed error alias keeping signatures readable.
type BoxErr = Box<dyn Error + Send + Sync>;

/// Paths and keys the committed devnet proving/verifying keys live at.
const PK_PATH: &str = "chain/zk/proving_key.bin";
const VK_PATH: &str = "chain/zk/verifying_key.bin";

/// The DR curvature parameter the proof is built with; unit (`1_000_000` ppm)
/// matches the devnet setup and the settlement math's neutral default.
const K_PPM: u64 = 1_000_000;

/// Parse a `0x`-prefixed 32-byte hex string into a fixed byte array, erroring
/// on any malformed value.
fn parse_bytes32(s: &str) -> Result<[u8; 32], BoxErr> {
    let bytes = hex::decode(s.trim_start_matches("0x"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()).into());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Read a required environment variable or fail with a clear message.
fn req_env(name: &str) -> Result<String, BoxErr> {
    std::env::var(name).map_err(|_| format!("missing required environment variable: {name}").into())
}

/// Build the fixed sample of usage rows this demo proves.
///
/// A single honest row attributes some play activity to `work_id`; the values
/// are arbitrary but fixed so the demo is reproducible. `row-split` mode reuses
/// this row twice with the same `work_id` to probe the uniqueness cap.
fn sample_row(work_id: [u8; 32]) -> UsageRowInput {
    UsageRowInput {
        work_id,
        minutes: 60,
        plays: 2,
        salt: [9u8; 32],
        price_ppm: 1_000_000,
        region_ppm: 1_000_000,
    }
}

/// Convert a slice of 32-byte arrays into the `Vec<B256>` alloy's `bytes32[]`
/// call arguments expect.
fn to_b256_vec(items: impl IntoIterator<Item = [u8; 32]>) -> Vec<B256> {
    items.into_iter().map(FixedBytes::from).collect()
}

/// The parsed runtime configuration for one `zk_submit` invocation.
struct Config {
    /// The proving mode (`honest` / `tamper-digest` / `row-split`).
    mode: String,
    /// JSON-RPC endpoint.
    rpc_url: String,
    /// The submitting user's signing key (hex).
    private_key: String,
    /// The `CWEConsumption` address.
    consumption: Address,
    /// The `CWEEpochBeacon` address.
    beacon: Address,
    /// The subscription tier id bound into the proof.
    tier: [u8; 32],
    /// The work id the sample usage is attributed to.
    work_id: [u8; 32],
}

impl Config {
    /// Assemble the configuration from args (`--mode <m>`) and the environment.
    fn parse() -> Result<Config, BoxErr> {
        // Hand-parse `--mode <value>`; settlement uses no arg-parsing crate, so
        // neither does this sibling bin.
        let mut mode = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--mode" => mode = args.next(),
                other => return Err(format!("unexpected argument: {other}").into()),
            }
        }
        let mode = mode.ok_or("missing required --mode <honest|tamper-digest|row-split>")?;

        let rpc_url =
            std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        let private_key = req_env("PRIVATE_KEY")?;

        // Read the deployment address map for the two contracts we touch.
        let deployments_path = std::env::var("DEPLOYMENTS")
            .unwrap_or_else(|_| "chain/deployments/localhost.json".to_string());
        let raw = std::fs::read_to_string(&deployments_path)
            .map_err(|e| format!("loading {deployments_path}: {e}"))?;
        let map: serde_json::Value = serde_json::from_str(&raw)?;
        let consumption = Address::from_str(
            map["consumption"]
                .as_str()
                .ok_or("deployments: missing `consumption`")?,
        )?;
        let beacon = Address::from_str(
            map["beacon"]
                .as_str()
                .ok_or("deployments: missing `beacon`")?,
        )?;

        // The tier id is required (the demo sets and funds it); the work id has a
        // stable default matching `cast format-bytes32-string "zkwork"`.
        let tier = parse_bytes32(&req_env("TIER")?)?;
        let work_id = match std::env::var("WORK_ID") {
            Ok(s) => parse_bytes32(&s)?,
            Err(_) => {
                // "zkwork" as a right-padded bytes32 string.
                let mut w = [0u8; 32];
                w[..6].copy_from_slice(b"zkwork");
                w
            }
        };

        Ok(Config {
            mode,
            rpc_url,
            private_key,
            consumption,
            beacon,
            tier,
            work_id,
        })
    }
}

/// Submit one honest proof and require the on-chain verifier to accept it.
///
/// Reads the live epoch and its beacon key, proves the sample row, ABI-encodes
/// the proof for the Solidity verifier, and submits ONLY the active rows'
/// fields alongside the bundle's unmodified digest. Errors if the transaction
/// does not mine successfully — an honest proof against the matching devnet
/// verifying key must pass.
async fn run_honest(cfg: &Config) -> Result<(), BoxErr> {
    let pk = load_pk(PK_PATH)?;

    let signer = PrivateKeySigner::from_str(&cfg.private_key)?;
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(cfg.rpc_url.parse()?);

    let consumption = Consumption::new(cfg.consumption, &provider);
    let beacon = Beacon::new(cfg.beacon, &provider);

    // The proof binds the LIVE epoch and its published beacon key, so the digest
    // settlement recomputes (from the same epoch/tier/key) matches this bundle.
    let epoch: u64 = consumption.currentEpoch().call().await?.try_into()?;
    let k_epoch: [u8; 32] = beacon.keyFor(U256::from(epoch)).call().await?.0;

    let inputs = [sample_row(cfg.work_id)];
    let bundle = prove(&pk, epoch, &cfg.tier, &k_epoch, K_PPM, &inputs)?;

    // Submit ONLY the active rows (settlement pads to MAX_WORKS off-chain).
    let n = inputs.len();
    let active = &bundle.rows[..n];
    let commitments = to_b256_vec(active.iter().map(|r| r.commitment));
    let pseudonyms = to_b256_vec(active.iter().map(|r| r.pseudonym));
    let work_ids = to_b256_vec(active.iter().map(|r| r.work_id));
    let weights: Vec<U256> = active.iter().map(|r| U256::from(r.weight)).collect();

    // Re-encode the proof into the (a, b, c) calldata the Groth16Verifier decodes.
    let proof = proof_to_evm_calldata(&bundle.proof)
        .ok_or("failed to ABI-encode proof for the on-chain verifier")?;

    println!("honest: epoch={epoch} weights={:?}", &weights);
    let receipt = consumption
        .submitConsumption(
            FixedBytes::from(cfg.tier),
            commitments,
            pseudonyms,
            work_ids,
            weights,
            FixedBytes::from(bundle.digest),
            Bytes::from(proof),
        )
        .send()
        .await?
        .get_receipt()
        .await?;

    if !receipt.status() {
        return Err("honest submit reverted unexpectedly".into());
    }
    println!(
        "honest: submitted in tx {:#x} (status ok)",
        receipt.transaction_hash
    );
    Ok(())
}

/// Prove honestly but submit a TAMPERED digest; require the on-chain verifier to
/// reject it. Returns `true` iff the submission was rejected (the expected
/// outcome), whether the revert surfaced at gas-estimation/send time or as a
/// mined-but-reverted receipt.
async fn run_tamper_digest(cfg: &Config) -> Result<bool, BoxErr> {
    let pk = load_pk(PK_PATH)?;

    let signer = PrivateKeySigner::from_str(&cfg.private_key)?;
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(cfg.rpc_url.parse()?);

    let consumption = Consumption::new(cfg.consumption, &provider);
    let beacon = Beacon::new(cfg.beacon, &provider);

    let epoch: u64 = consumption.currentEpoch().call().await?.try_into()?;
    let k_epoch: [u8; 32] = beacon.keyFor(U256::from(epoch)).call().await?.0;

    let inputs = [sample_row(cfg.work_id)];
    let bundle = prove(&pk, epoch, &cfg.tier, &k_epoch, K_PPM, &inputs)?;

    let n = inputs.len();
    let active = &bundle.rows[..n];
    let commitments = to_b256_vec(active.iter().map(|r| r.commitment));
    let pseudonyms = to_b256_vec(active.iter().map(|r| r.pseudonym));
    let work_ids = to_b256_vec(active.iter().map(|r| r.work_id));
    let weights: Vec<U256> = active.iter().map(|r| U256::from(r.weight)).collect();
    let proof = proof_to_evm_calldata(&bundle.proof)
        .ok_or("failed to ABI-encode proof for the on-chain verifier")?;

    // Flip the top byte of the digest we SUBMIT; the proof stays intact, so the
    // public input no longer matches what the proof attests to.
    let mut tampered = bundle.digest;
    tampered[0] ^= 0xff;

    let send = consumption
        .submitConsumption(
            FixedBytes::from(cfg.tier),
            commitments,
            pseudonyms,
            work_ids,
            weights,
            FixedBytes::from(tampered),
            Bytes::from(proof),
        )
        .send()
        .await;

    match send {
        // The revert surfaced at gas-estimation/send time — the common case.
        Err(e) => {
            println!("tamper-digest: rejected as expected ({e})");
            Ok(true)
        }
        // Mined: inspect the receipt. A false status is a mined-then-reverted tx.
        Ok(pending) => {
            let receipt = pending.get_receipt().await?;
            if receipt.status() {
                println!(
                    "tamper-digest: UNEXPECTEDLY accepted in tx {:#x}",
                    receipt.transaction_hash
                );
                Ok(false)
            } else {
                println!(
                    "tamper-digest: rejected as expected (reverted in tx {:#x})",
                    receipt.transaction_hash
                );
                Ok(true)
            }
        }
    }
}

/// Attempt to build a row-split proof and require the machinery to refuse it.
///
/// Two rows share a `work_id`, violating the circuit's per-work uniqueness
/// constraint. Returns `true` iff the cap held — either `prove` errored, or it
/// produced a proof that fails self-verification against the devnet verifying
/// key. A `prove` that returns a genuinely valid proof would be a soundness
/// break and returns `false`. Nothing is submitted on-chain.
fn run_row_split(cfg: &Config) -> Result<bool, BoxErr> {
    let pk = load_pk(PK_PATH)?;

    // Two active rows carrying the SAME work id — the split the DR cap forbids.
    let inputs = [sample_row(cfg.work_id), sample_row(cfg.work_id)];

    // The epoch/key values are irrelevant here: the witness is unsatisfiable
    // regardless, so fixed placeholders are fine (nothing is submitted).
    match prove(&pk, 1, &cfg.tier, &[0x11u8; 32], K_PPM, &inputs) {
        Err(e) => {
            println!("row-split: prove refused as expected ({e})");
            Ok(true)
        }
        Ok(bundle) => {
            // `prove` returned a bundle; it MUST NOT verify (the constraints are
            // unsatisfiable). Confirm the proof is invalid before declaring the
            // cap intact.
            let vk = load_vk(VK_PATH)?;
            if verify(&vk, &bundle.digest, &bundle.proof) {
                println!("row-split: UNEXPECTED valid proof for duplicate work ids");
                Ok(false)
            } else {
                println!("row-split: proof invalid as expected (self-verify failed)");
                Ok(true)
            }
        }
    }
}

/// Async entry point: parse the config, dispatch to the selected mode, and map
/// the mode's expected outcome to exit code 0 (anything unexpected → non-zero).
#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match Config::parse() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("zk_submit: configuration error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = match cfg.mode.as_str() {
        "honest" => run_honest(&cfg).await.map(|()| true),
        "tamper-digest" => run_tamper_digest(&cfg).await,
        "row-split" => run_row_split(&cfg),
        other => {
            eprintln!("zk_submit: unknown mode '{other}'");
            return ExitCode::FAILURE;
        }
    };

    match result {
        // The mode's EXPECTED outcome happened.
        Ok(true) => ExitCode::SUCCESS,
        // The mode ran but observed the WRONG outcome (e.g. a tampered submit
        // that was accepted, or a valid row-split proof).
        Ok(false) => {
            eprintln!(
                "zk_submit: mode '{}' produced an UNEXPECTED outcome",
                cfg.mode
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("zk_submit: {e}");
            ExitCode::FAILURE
        }
    }
}
