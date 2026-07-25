//! The `cwe-storage` node: serves content fragments over HTTP and co-signs a
//! bandwidth receipt for each fragment it actually delivered.
//!
//! Cycle 1 is deliberately a single plain-HTTP node, not a swarm: the point is to
//! make real bytes move and to produce evidence the aggregator can verify, not to
//! build content distribution. Peer discovery, redundancy, proof-of-storage and
//! the rest live in the deferred storage-swarm cycle.
//!
//! One boundary worth naming: the ledger this node signs from records bytes
//! *read from disk and handed to the response writer*, not bytes confirmed to
//! have reached the consumer's socket — a plain HTTP handler has no hook to
//! observe the latter. The node's signature alone therefore is not proof of
//! delivery. What makes the receipt trustworthy is the OTHER signature: the
//! consumer counts what it actually received and refuses to counter-sign a
//! mismatch, and the aggregator only pays out receipts both parties signed. So
//! an over-attested node count simply produces a receipt nobody will
//! counter-sign — worthless on its own, and unable to become spendable.
//!
//! Configuration (environment):
//! * `CONTENT_DIR` — directory holding `<work_id>.bin` files (required)
//! * `PRIVATE_KEY` — this node's signing key; its address is the one that must
//!   hold a storage-node credential for receipts to count (required)
//! * `EPOCH`       — the settlement epoch receipts are bound to (required)
//! * `PORT`        — listen port (default 8546)

use std::path::PathBuf;
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use cwe_receipt::{normalize_addr, Receipt};
use cwe_storage::{fragment, issue_receipt, Ledger, ServedChunk};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Everything the handlers share: the node's identity, its content directory,
/// the epoch it is serving, and the ledger of what it has served.
struct NodeState {
    /// The node's signing key; also the source of its address.
    signer: PrivateKeySigner,
    /// Where `<work_id>.bin` files live.
    content_dir: PathBuf,
    /// The settlement epoch every issued receipt is bound to.
    epoch: u64,
    /// What has been served so far, guarded for concurrent requests.
    ledger: RwLock<Ledger>,
}

/// Query parameters of `GET /content/{work_id}`.
#[derive(Debug, Deserialize)]
struct ContentQuery {
    /// The address the bytes are being served to; bound into the receipt.
    consumer: String,
    /// The consumer-chosen per-session nonce.
    session: String,
    /// The chunk counter within the session.
    chunk: u64,
    /// Byte offset into the work's content.
    offset: u64,
    /// How many bytes to serve from `offset`.
    len: u64,
}

/// Body of `POST /receipt`: which served chunk to attest.
#[derive(Debug, Deserialize)]
struct ReceiptRequest {
    /// The session the chunk was served under.
    session_nonce: String,
    /// The chunk counter within that session.
    chunk_nonce: u64,
}

/// Response of `POST /receipt`: the node's statement and its signature over it.
#[derive(Debug, Serialize)]
struct ReceiptResponse {
    /// The receipt the node is willing to stand behind.
    receipt: Receipt,
    /// The node's EIP-191 signature over the receipt's canonical bytes.
    node_sig: String,
}

/// Liveness probe, so the demo can wait for the node to accept connections.
async fn health() -> &'static str {
    "ok"
}

/// Serve a fragment of a work's content and record what was handed to the
/// response writer for it.
///
/// The recorded byte count is the length of the fragment actually read off
/// disk — not what the caller asked for — so a clamped or short read attests
/// only the bytes that were really available, never an inflated request size.
///
/// What it is NOT: proof of delivery. This count is taken the moment the bytes
/// are handed to axum's response writer, before they are flushed to the
/// consumer's socket; if the connection drops mid-transfer the ledger still
/// holds the full count. A plain HTTP handler has no hook to observe the
/// client actually receiving the bytes, so establishing that the transfer
/// completed is deliberately not this function's job — it is the consumer's.
/// The consumer counts what it actually received and declines to counter-sign
/// a receipt whose count doesn't match, and the aggregator only pays out
/// receipts both parties signed. So a receipt only becomes spendable once
/// node and consumer agree on the number; the node's signature by itself
/// proves nothing about delivery.
async fn content(
    State(state): State<Arc<NodeState>>,
    AxumPath(work_id): AxumPath<String>,
    Query(q): Query<ContentQuery>,
) -> impl IntoResponse {
    let bytes = match fragment(&state.content_dir, &work_id, q.offset, q.len) {
        Ok(b) => b,
        // A bad id or unreadable content is a client-visible 404; the node
        // simply has nothing to serve under that name.
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // Record what we are about to hand over, keyed exactly as a later receipt
    // request will ask for it. This is bytes read, not bytes confirmed
    // delivered; the consumer's counter-signature is what turns it into proof.
    state.ledger.write().await.record(
        &q.session,
        q.chunk,
        ServedChunk {
            work_id: work_id.to_ascii_lowercase(),
            consumer: normalize_addr(&q.consumer),
            bytes: bytes.len() as u64,
        },
    );

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    )
        .into_response()
}

/// Issue and sign a receipt for a previously served chunk.
///
/// Returns 404 when the node has no record of serving that chunk — it will not
/// sign for bytes it did not move.
async fn receipt(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<ReceiptRequest>,
) -> impl IntoResponse {
    let ledger = state.ledger.read().await;
    let node_addr = format!("{:#x}", state.signer.address());
    let receipt = match issue_receipt(
        &ledger,
        &node_addr,
        state.epoch,
        &req.session_nonce,
        req.chunk_nonce,
    ) {
        Ok(r) => r,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // Sign the canonical bytes; the consumer will counter-sign the very same
    // encoding, which is why both sides call `canonical_bytes`.
    let msg = match receipt.canonical_bytes() {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let sig = match state.signer.sign_message_sync(&msg) {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(ReceiptResponse {
        receipt,
        node_sig: format!("0x{}", hex_string(&sig.as_bytes())),
    })
    .into_response()
}

/// Lowercase hex of a byte slice, the form receipts carry signatures in.
fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read a required environment variable or fail with a clear message.
fn req_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::var(name).map_err(|_| format!("missing required environment variable: {name}").into())
}

/// Start the node: build the shared state from the environment and serve until
/// killed.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let content_dir = PathBuf::from(req_env("CONTENT_DIR")?);
    let signer: PrivateKeySigner = req_env("PRIVATE_KEY")?.parse()?;
    let epoch: u64 = req_env("EPOCH")?.parse()?;
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8546".to_string())
        .parse()?;

    let addr = format!("{:#x}", signer.address());
    let state = Arc::new(NodeState {
        signer,
        content_dir,
        epoch,
        ledger: RwLock::new(Ledger::default()),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/content/{work_id}", get(content))
        .route("/receipt", post(receipt))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("cwe-storage: node {addr} serving epoch {epoch} on port {port}");
    axum::serve(listener, app).await?;
    Ok(())
}
