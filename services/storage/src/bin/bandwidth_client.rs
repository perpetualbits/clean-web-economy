//! The consumer half of the bandwidth-receipt path (H5 cycle 2).
//!
//! Talks to a `cwe-storage` node in three modes, selected with `--mode`:
//!
//! * `honest` — fetch every configured chunk in full, ask the node to attest
//!   each one, verify the node's signature and the fields against what was
//!   requested, and counter-sign. This is the only mode a real consumer runs.
//! * `no-download` — download nothing at all, then ask the node to attest
//!   every chunk anyway. The node holds no ledger entry for content it never
//!   delivered, so it must refuse every request; this mode exists purely to
//!   demonstrate that refusal deterministically, ending with zero receipts.
//! * `refetch` — fetch the SAME chunk (index 0) over and over and co-sign
//!   every delivery. Each delivery is genuine, so the node signs each one; it
//!   is the aggregator's dedup key, not anything the client does, that
//!   collapses repeated evidence for one content position down to a single
//!   chunk's worth of credit.
//!
//! There is deliberately no "abandon the transfer mid-stream" mode here. Over
//! loopback the kernel absorbs a whole 128 KiB chunk before an abort is even
//! noticed, so the stream completes legitimately and the chunk is credited —
//! correct behaviour, but it makes any byte-exact assertion over HTTP a race.
//! That trust property is instead proven deterministically at the unit level
//! by the storage crate's `abandoned_stream_credits_nothing` test, which
//! drives the delivery stream directly with no sockets involved.
//!
//! The key used here MUST be the same wallet key that submits the matching
//! usage on-chain: the aggregator joins receipts to DAPR rows by consumer
//! address, so a different key would leave the usage looking entirely
//! unbacked by bytes.
//!
//! Configuration (environment):
//! * `STORAGE_URL` — node base URL (default `http://127.0.0.1:8546`)
//! * `WORK_ID`     — the work to download, `0x` + 64 hex characters (required)
//! * `PRIVATE_KEY` — the consumer's wallet key (required)
//! * `EPOCH`       — the settlement epoch (required)
//! * `CHUNKS`      — how many chunks to fetch (default 4)
//! * `OUT`         — where to write the receipt bundle JSON (required)
//! * `REPEATS`     — how many times to refetch chunk 0 (default 100, `refetch`
//!   mode only)

// `co_sign_chunk` takes eight parameters: the receipt's five bound fields plus
// the http client, signer and expected-byte-count it needs to verify and sign.
// Splitting those into a struct would only hide the same information behind an
// extra type; the sibling `zk_submit` binary silences the same lint for the
// same reason.
#![allow(clippy::too_many_arguments)]

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use cwe_receipt::{Receipt, ReceiptBundle, SignedReceipt};

/// Boxed error alias, matching the style of the sibling settlement binaries.
type BoxErr = Box<dyn std::error::Error>;

/// The node's `POST /receipt` response.
#[derive(serde::Deserialize)]
struct ReceiptResponse {
    /// The node's statement about what it served.
    receipt: Receipt,
    /// The node's signature over that statement.
    node_sig: String,
}

/// Read a required environment variable or fail with a clear message.
fn req_env(name: &str) -> Result<String, BoxErr> {
    std::env::var(name).map_err(|_| format!("missing required environment variable: {name}").into())
}

/// Read an optional environment variable, parsed, with a default.
fn opt_env<T: std::str::FromStr>(name: &str, default: T) -> Result<T, BoxErr>
where
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(v) => v.parse().map_err(|e| format!("invalid {name}: {e}").into()),
        Err(_) => Ok(default),
    }
}

/// Lowercase hex of a byte slice.
fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Fetch one chunk in full and return the bytes actually received.
///
/// Used by `honest` and `refetch`. The node credits the chunk only when the
/// whole body has been delivered, so this must drain the response completely.
async fn fetch_chunk_fully(
    http: &reqwest::Client,
    base: &str,
    work_id: &str,
    consumer: &str,
    chunk_index: u64,
) -> Result<u64, BoxErr> {
    let body = http
        .get(format!("{base}/content/{work_id}"))
        .query(&[
            ("consumer", consumer.to_string()),
            ("chunk_index", chunk_index.to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(body.len() as u64)
}

// NOTE: there is deliberately no "download" helper for the `no-download` mode —
// that mode's whole point is that it never requests content at all, and goes
// straight to asking the node to attest chunks it never delivered.

/// Ask the node to attest a delivered chunk, verify its signature and the
/// fields we asked for, and counter-sign.
///
/// Returns `None` when the node refuses (404) — which is the expected outcome
/// for a chunk that was never fully delivered, not an error.
async fn co_sign_chunk(
    http: &reqwest::Client,
    base: &str,
    signer: &PrivateKeySigner,
    work_id: &str,
    consumer: &str,
    epoch: u64,
    chunk_index: u64,
    expected_bytes: Option<u64>,
) -> Result<Option<SignedReceipt>, BoxErr> {
    let resp = http
        .post(format!("{base}/receipt"))
        .json(&serde_json::json!({
            "work_id": work_id,
            "chunk_index": chunk_index,
            "consumer": consumer,
        }))
        .send()
        .await?;

    // A refusal is a legitimate answer: the node will not attest a chunk it did
    // not fully deliver.
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp: ReceiptResponse = resp.error_for_status()?.json().await?;

    // Check the node's signature BEFORE counter-signing: putting our name on a
    // receipt the node did not properly sign only burns our own credibility.
    let recovered = resp.receipt.recover_signer(&resp.node_sig)?;
    if format!("{recovered:#x}") != resp.receipt.node.to_ascii_lowercase() {
        return Err(
            format!("chunk {chunk_index}: node signature does not match its address").into(),
        );
    }

    // Verify the receipt describes exactly what we asked for. A node running a
    // stale epoch, or attesting another work, would have us counter-sign
    // receipts settlement later drops — silently burning our whole tier fee.
    if resp.receipt.epoch != epoch {
        return Err(format!(
            "chunk {chunk_index}: node attested epoch {} but we are settling {epoch}",
            resp.receipt.epoch
        )
        .into());
    }
    if resp.receipt.work_id != work_id {
        return Err(format!(
            "chunk {chunk_index}: node attested work {} but we requested {work_id}",
            resp.receipt.work_id
        )
        .into());
    }
    if !resp.receipt.consumer.eq_ignore_ascii_case(consumer) {
        return Err(format!(
            "chunk {chunk_index}: node attested consumer {} but we are {consumer}",
            resp.receipt.consumer
        )
        .into());
    }
    if resp.receipt.chunk_index != chunk_index {
        return Err(format!(
            "node attested chunk {} but we requested {chunk_index}",
            resp.receipt.chunk_index
        )
        .into());
    }
    if let Some(want) = expected_bytes {
        if resp.receipt.bytes != want {
            return Err(format!(
                "chunk {chunk_index}: node attested {} bytes but delivered {want}",
                resp.receipt.bytes
            )
            .into());
        }
    }

    let msg = resp.receipt.canonical_bytes()?;
    let consumer_sig = format!(
        "0x{}",
        hex_string(&signer.sign_message_sync(&msg)?.as_bytes())
    );
    Ok(Some(SignedReceipt {
        receipt: resp.receipt,
        node_sig: resp.node_sig,
        consumer_sig,
    }))
}

/// Parse `--mode`, run the requested download/attest pattern, and write the
/// resulting receipt bundle to `OUT`.
#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    // Hand-parse `--mode <value>`; the sibling binaries use no arg-parsing crate.
    let mut mode = "honest".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => mode = args.next().ok_or("missing value for --mode")?,
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }

    let base = std::env::var("STORAGE_URL").unwrap_or_else(|_| "http://127.0.0.1:8546".to_string());
    let work_id = req_env("WORK_ID")?.to_ascii_lowercase();
    let signer: PrivateKeySigner = req_env("PRIVATE_KEY")?.parse()?;
    let epoch: u64 = req_env("EPOCH")?.parse()?;
    let chunks: u64 = opt_env("CHUNKS", 4u64)?;
    let repeats: u64 = opt_env("REPEATS", 100u64)?;
    let out = req_env("OUT")?;

    let consumer = format!("{:#x}", signer.address());
    let http = reqwest::Client::new();
    let mut signed: Vec<SignedReceipt> = Vec::new();

    match mode.as_str() {
        // Deliver every chunk in full and co-sign each.
        "honest" => {
            for chunk_index in 0..chunks {
                let got = fetch_chunk_fully(&http, &base, &work_id, &consumer, chunk_index).await?;
                if got == 0 {
                    break; // past the end of the content
                }
                if let Some(sr) = co_sign_chunk(
                    &http,
                    &base,
                    &signer,
                    &work_id,
                    &consumer,
                    epoch,
                    chunk_index,
                    Some(got),
                )
                .await?
                {
                    signed.push(sr);
                }
            }
        }
        // Download nothing at all, then ask the node to attest every chunk
        // anyway. The node holds no ledger entry for any of them, so it must
        // refuse each request and this mode must end with ZERO receipts.
        "no-download" => {
            for chunk_index in 0..chunks {
                if let Some(sr) = co_sign_chunk(
                    &http,
                    &base,
                    &signer,
                    &work_id,
                    &consumer,
                    epoch,
                    chunk_index,
                    None,
                )
                .await?
                {
                    signed.push(sr);
                }
            }
        }
        // Fetch the SAME chunk over and over. Each delivery is real, but they
        // are all the same content position, so the aggregator must count them
        // once however many receipts are produced.
        "refetch" => {
            for _ in 0..repeats {
                let got = fetch_chunk_fully(&http, &base, &work_id, &consumer, 0).await?;
                if let Some(sr) = co_sign_chunk(
                    &http,
                    &base,
                    &signer,
                    &work_id,
                    &consumer,
                    epoch,
                    0,
                    Some(got),
                )
                .await?
                {
                    signed.push(sr);
                }
            }
        }
        other => return Err(format!("unknown mode '{other}'").into()),
    }

    let bundle = ReceiptBundle {
        epoch,
        receipts: signed,
    };
    std::fs::write(&out, bundle.to_json()?)?;
    println!(
        "bandwidth-client[{mode}]: {} receipts for work {work_id} → {out}",
        bundle.receipts.len()
    );
    Ok(())
}
