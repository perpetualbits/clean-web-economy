//! The consumer half of the bandwidth-receipt path (H5 cycle 1).
//!
//! Downloads a work's content from a `cwe-storage` node in chunks, asks the node
//! to attest each chunk, verifies the node's signature, counter-signs with the
//! consumer's own wallet key, and writes the whole epoch's co-signed receipts out
//! as a bundle for the settlement aggregator to verify.
//!
//! The key used here MUST be the same wallet key that submits the matching usage
//! on-chain: the aggregator joins receipts to DAPR rows by consumer address, so a
//! different key would leave the usage looking entirely unbacked by bytes.
//!
//! Configuration (environment):
//! * `STORAGE_URL` — node base URL (default `http://127.0.0.1:8546`)
//! * `WORK_ID`     — the work to download, `0x` + 64 hex characters (required)
//! * `PRIVATE_KEY` — the consumer's wallet key (required)
//! * `EPOCH`       — the settlement epoch (required)
//! * `CHUNKS`      — how many chunks to fetch (default 4)
//! * `CHUNK_LEN`   — bytes per chunk (default 131072)
//! * `OUT`         — where to write the receipt bundle JSON (required)

use alloy::primitives::keccak256;
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

/// Derive this consumer's session nonce for `work_id` in `epoch`.
///
/// Deterministic (no randomness) so a demo run is reproducible and a re-run
/// produces the same anti-replay key rather than silently minting a fresh
/// session that would let the same bytes be counted twice.
fn session_nonce(consumer: &str, work_id: &str, epoch: u64) -> String {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(consumer.as_bytes());
    preimage.extend_from_slice(work_id.as_bytes());
    preimage.extend_from_slice(&epoch.to_be_bytes());
    format!("0x{}", hex_string(keccak256(preimage).as_slice()))
}

/// Download the configured chunks, co-sign a receipt for each, and write the
/// bundle.
#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    let base = std::env::var("STORAGE_URL").unwrap_or_else(|_| "http://127.0.0.1:8546".to_string());
    let work_id = req_env("WORK_ID")?.to_ascii_lowercase();
    let signer: PrivateKeySigner = req_env("PRIVATE_KEY")?.parse()?;
    let epoch: u64 = req_env("EPOCH")?.parse()?;
    let chunks: u64 = opt_env("CHUNKS", 4u64)?;
    let chunk_len: u64 = opt_env("CHUNK_LEN", 131_072u64)?;
    let out = req_env("OUT")?;

    let consumer = format!("{:#x}", signer.address());
    let session = session_nonce(&consumer, &work_id, epoch);
    let http = reqwest::Client::new();

    let mut signed: Vec<SignedReceipt> = Vec::new();
    let mut total_bytes: u64 = 0;

    for chunk in 0..chunks {
        // 1. Pull the actual bytes. The node records what it hands over.
        let url = format!("{base}/content/{work_id}");
        let body = http
            .get(&url)
            .query(&[
                ("consumer", consumer.clone()),
                ("session", session.clone()),
                ("chunk", chunk.to_string()),
                ("offset", (chunk * chunk_len).to_string()),
                ("len", chunk_len.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        // A zero-length chunk means we have reached the end of the content;
        // there is nothing to attest, so stop rather than collect empty receipts.
        if body.is_empty() {
            break;
        }
        total_bytes += body.len() as u64;

        // 2. Ask the node to attest that chunk. The node signs its OWN count.
        let resp: ReceiptResponse = http
            .post(format!("{base}/receipt"))
            .json(&serde_json::json!({
                "session_nonce": session,
                "chunk_nonce": chunk,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // 3. Check the node's signature BEFORE counter-signing: putting our name
        // on a receipt the node did not properly sign would only burn our own
        // credibility, since the aggregator drops it either way.
        let recovered = resp.receipt.recover_signer(&resp.node_sig)?;
        if format!("{recovered:#x}") != resp.receipt.node.to_ascii_lowercase() {
            return Err(format!("chunk {chunk}: node signature does not match its address").into());
        }
        // Sanity-check the node is attesting what we actually received, so a node
        // under-reporting bytes is caught here rather than silently costing us
        // credibility at settlement.
        if resp.receipt.bytes != body.len() as u64 {
            return Err(format!(
                "chunk {chunk}: node attested {} bytes but served {}",
                resp.receipt.bytes,
                body.len()
            )
            .into());
        }

        // 4. Counter-sign the identical canonical bytes.
        let msg = resp.receipt.canonical_bytes()?;
        let consumer_sig = format!(
            "0x{}",
            hex_string(&signer.sign_message_sync(&msg)?.as_bytes())
        );

        signed.push(SignedReceipt {
            receipt: resp.receipt,
            node_sig: resp.node_sig,
            consumer_sig,
        });
    }

    let bundle = ReceiptBundle {
        epoch,
        receipts: signed,
    };
    std::fs::write(&out, bundle.to_json()?)?;
    println!(
        "bandwidth-client: {} receipts, {total_bytes} bytes for work {work_id} → {out}",
        bundle.receipts.len()
    );
    Ok(())
}
