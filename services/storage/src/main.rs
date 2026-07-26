//! The `cwe-storage` node binary: serves content chunks over HTTP and
//! co-signs a bandwidth receipt for each chunk it actually delivered in full.
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
//! [`cwe_storage::DeliveryStream`] completion callback records.
//!
//! That is deliberately NOT the same claim cycle 1 made. Cycle 1's ledger entry
//! was written the moment bytes were read off disk, before anything was sent —
//! a client could request a chunk, never read the response body, and still
//! receive a signed receipt for the full count. This cycle closes that: the
//! entry is written by the stream's completion callback, which fires only when
//! the whole chunk has been yielded, so an abandoned transfer credits nothing
//! and the node has no receipt to sign for it.
//!
//! The receipt still binds no byte RANGE, only a chunk index and a total count,
//! but the anti-replay key is now content position — `(consumer, work_id,
//! chunk_index)` — rather than a client-chosen session/chunk nonce. That is
//! what stops a consumer minting unlimited fresh evidence for the same bytes:
//! re-requesting a chunk overwrites the same ledger entry and the aggregator
//! dedups on the identical key, so total credit for a work is capped at the
//! work's real size no matter how many requests are issued.
//!
//! This binary is deliberately thin: all routing and handler logic lives in
//! `cwe_storage::router`, which is what the crate's tests exercise directly
//! (see `services/storage/src/lib.rs`) so the wiring between the HTTP layer
//! and the delivery-gated ledger is pinned without a socket. This file only
//! reads the environment, builds the shared state, and serves.
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
use cwe_storage::{router, Ledger, NodeState};

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

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("cwe-storage: node {addr} serving epoch {epoch} on port {port}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
