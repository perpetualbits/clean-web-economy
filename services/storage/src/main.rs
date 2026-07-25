//! The `cwe-storage` node: serves content chunks over HTTP and co-signs a
//! bandwidth receipt for each chunk it actually delivered in full.
//!
//! Cycle 2 is still deliberately a single plain-HTTP node, not a swarm: the
//! point is to make real bytes move and to produce evidence the aggregator can
//! verify, not to build content distribution. Peer discovery, redundancy,
//! proof-of-storage and the rest live in the deferred storage-swarm cycle.
//!
//! One boundary worth naming precisely: the ledger this node signs from records
//! bytes *fully handed to the transport* — every slice of the chunk has left
//! this process for the client's socket — not bytes confirmed to have been
//! read by the client application. A plain HTTP server has no hook to observe
//! the latter, so "the node attests delivery to the transport" is the
//! strongest claim it can make honestly, and it is what the
//! [`cwe_storage::DeliveryStream`] completion callback below records.
//!
//! That is deliberately NOT the same claim cycle 1 made. Cycle 1's ledger entry
//! was written the moment bytes were read off disk, before anything was sent —
//! a client could request a chunk, never read the response body, and still
//! receive a signed receipt for the full count. This cycle closes that: the
//! entry is written by the stream's completion callback, which fires only when
//! the whole chunk has been yielded, so an abandoned transfer credits nothing
//! and the node has no receipt to sign for it (see `content` below).
//!
//! The receipt still binds no byte RANGE, only a chunk index and a total count,
//! but the anti-replay key is now content position — `(consumer, work_id,
//! chunk_index)` — rather than a client-chosen session/chunk nonce. That is
//! what stops a consumer minting unlimited fresh evidence for the same bytes:
//! re-requesting a chunk overwrites the same ledger entry and the aggregator
//! dedups on the identical key, so total credit for a work is capped at the
//! work's real size no matter how many requests are issued.
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
use cwe_storage::{fragment_for_chunk, issue_receipt, DeliveryStream, Ledger, ServedChunk};
use serde::{Deserialize, Serialize};

/// Everything the handlers share: the node's identity, its content directory,
/// the epoch it is serving, and the ledger of what it has fully delivered.
struct NodeState {
    /// The node's signing key; also the source of its address.
    signer: PrivateKeySigner,
    /// Where `<work_id>.bin` files live.
    content_dir: PathBuf,
    /// The settlement epoch every issued receipt is bound to.
    epoch: u64,
    /// What has been fully delivered so far. A blocking mutex rather than an
    /// async one: the delivery stream's completion callback fires inside
    /// `poll_next`, which cannot `.await`, and every ledger operation is a short
    /// map write.
    ledger: std::sync::Mutex<Ledger>,
}

/// Query parameters of `GET /content/{work_id}`.
#[derive(Debug, Deserialize)]
struct ContentQuery {
    /// The address the bytes are being delivered to; bound into the receipt.
    consumer: String,
    /// Which `CHUNK_SIZE` block of the work's content to deliver.
    chunk_index: u64,
}

/// Body of `POST /receipt`: which delivered chunk to attest.
#[derive(Debug, Deserialize)]
struct ReceiptRequest {
    /// The work the chunk belongs to.
    work_id: String,
    /// Which block of that work.
    chunk_index: u64,
    /// The consumer the chunk was delivered to.
    consumer: String,
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

/// Deliver one content chunk, crediting it only once it has been delivered
/// in full.
///
/// The ledger entry is written by the delivery stream's completion callback,
/// not here — so a client that abandons the transfer part-way leaves no record
/// and can obtain no receipt for this chunk. Retrying the same index is safe:
/// the ledger overwrites, and the aggregator dedups on content position.
///
/// The credited count is what left the server for the client's transport. It is
/// not, and cannot over HTTP be, a claim about what the client application did
/// with the bytes — see the crate docs for why that distinction does not matter
/// for a bandwidth measure.
async fn content(
    State(state): State<Arc<NodeState>>,
    AxumPath(work_id): AxumPath<String>,
    Query(q): Query<ContentQuery>,
) -> impl IntoResponse {
    let bytes = match fragment_for_chunk(&state.content_dir, &work_id, q.chunk_index) {
        Ok(b) => b,
        // A bad id or unreadable content is a client-visible 404; the node
        // simply has nothing to serve under that name.
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    // Capture what a completed delivery should record, then hand ownership to
    // the stream's completion callback.
    let served = ServedChunk {
        work_id: work_id.to_ascii_lowercase(),
        consumer: normalize_addr(&q.consumer),
        bytes: bytes.len() as u64,
    };
    let state_for_done = state.clone();
    let consumer = normalize_addr(&q.consumer);
    let work_for_done = work_id.to_ascii_lowercase();
    let chunk_index = q.chunk_index;

    // 16 KiB slices: small enough that an abandoned transfer stops early, large
    // enough to avoid a poll per byte.
    let stream = DeliveryStream::new(bytes, 16 * 1024, move || {
        if let Ok(mut ledger) = state_for_done.ledger.lock() {
            ledger.record(&consumer, &work_for_done, chunk_index, served);
        }
    });

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

/// Issue and sign a receipt for a previously delivered chunk.
///
/// Returns 404 when the node has no record of fully delivering that chunk — it
/// will not sign for bytes it did not finish moving.
async fn receipt(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<ReceiptRequest>,
) -> impl IntoResponse {
    let node_addr = format!("{:#x}", state.signer.address());
    let receipt = {
        // Hold the lock only for the lookup; signing happens outside it.
        let ledger = match state.ledger.lock() {
            Ok(l) => l,
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "ledger poisoned").into_response()
            }
        };
        match issue_receipt(
            &ledger,
            &node_addr,
            state.epoch,
            &req.consumer,
            &req.work_id,
            req.chunk_index,
        ) {
            Ok(r) => r,
            Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
        }
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
        ledger: std::sync::Mutex::new(Ledger::default()),
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
