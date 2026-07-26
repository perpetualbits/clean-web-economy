//! The storage node's serving core (H5 cycle 2).
//!
//! Three responsibilities live here so the node's actual behaviour — not just
//! its building blocks — can be exercised without a socket: reading a
//! `CHUNK_SIZE` block of a work's content out of a content directory by chunk
//! index, remembering exactly which content positions have been FULLY
//! DELIVERED to whom so a receipt can be issued for them later, and the HTTP
//! [`router`] that wires those pieces to `GET /content/{work_id}` and
//! `POST /receipt`. `main.rs` is deliberately thin: it only builds a
//! [`NodeState`] from the environment and serves this router.
//!
//! The node only ever signs byte counts from its own ledger, and the ledger
//! only ever holds a chunk once it has been handed to the transport in full —
//! see [`DeliveryStream`]. A consumer cannot talk it into attesting bytes that
//! never left the server, and an abandoned transfer earns nothing rather than
//! a proportional share, which is precisely the guarantee the aggregator
//! relies on when it turns receipts into bandwidth credibility. The
//! `oneshot`-driven tests at the bottom of this file pin that guarantee at the
//! HTTP layer, not just against the bare [`DeliveryStream`] type.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use cwe_receipt::{normalize_addr, Receipt, CHUNK_SIZE};
use serde::{Deserialize, Serialize};

/// A record of one fragment actually served: which work, to whom, how many bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedChunk {
    /// The work whose content was served, lowercase `0x` 32-byte hex.
    pub work_id: String,
    /// The consumer the bytes went to, lowercase `0x` address hex.
    pub consumer: String,
    /// How many bytes were actually written to the response body.
    pub bytes: u64,
}

/// The node's in-memory record of every chunk it has fully delivered this run,
/// keyed by CONTENT position — `(consumer, work_id, chunk_index)` — exactly the
/// coordinates a receipt request arrives with and the aggregator dedups on.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Delivered chunks. A repeat of the same key overwrites rather than
    /// accumulating, so a retried transfer cannot inflate what the node attests.
    entries: BTreeMap<(String, String, u64), ServedChunk>,
}

impl Ledger {
    /// Record that `served` was fully delivered for this content position.
    pub fn record(&mut self, consumer: &str, work_id: &str, chunk_index: u64, served: ServedChunk) {
        self.entries.insert(
            (
                normalize_addr(consumer),
                work_id.to_ascii_lowercase(),
                chunk_index,
            ),
            served,
        );
    }

    /// Look up what was delivered for this content position, if anything.
    pub fn get(&self, consumer: &str, work_id: &str, chunk_index: u64) -> Option<&ServedChunk> {
        self.entries.get(&(
            normalize_addr(consumer),
            work_id.to_ascii_lowercase(),
            chunk_index,
        ))
    }
}

/// Build the receipt this node is willing to sign for a content position.
///
/// The byte count comes from the ledger, never from the caller, and the ledger
/// only holds chunks that were delivered IN FULL. A transfer the client
/// abandoned part-way leaves no entry, so [`StorageError::NotServed`] is
/// returned and nothing is signed.
pub fn issue_receipt(
    ledger: &Ledger,
    node_addr: &str,
    epoch: u64,
    consumer: &str,
    work_id: &str,
    chunk_index: u64,
) -> Result<Receipt, StorageError> {
    let served = ledger
        .get(consumer, work_id, chunk_index)
        .ok_or(StorageError::NotServed)?;
    Ok(Receipt {
        work_id: served.work_id.to_ascii_lowercase(),
        consumer: normalize_addr(&served.consumer),
        node: normalize_addr(node_addr),
        chunk_index,
        bytes: served.bytes,
        epoch,
    })
}

/// Read the `chunk_index`-th `CHUNK_SIZE` block of `work_id`'s content.
///
/// The final block of a work is short and is returned at its true length; an
/// index past the end yields an empty slice rather than an error, since that is
/// a fact about the content, not a failure. `work_id` is validated as exactly
/// `0x` + 64 hex characters BEFORE it is joined onto `dir` — that check is what
/// keeps a crafted id from escaping the content directory.
pub fn fragment_for_chunk(
    dir: &Path,
    work_id: &str,
    chunk_index: u64,
) -> Result<Vec<u8>, StorageError> {
    if !is_work_id(work_id) {
        return Err(StorageError::BadWorkId);
    }
    let path = dir.join(format!("{}.bin", work_id.to_ascii_lowercase()));
    let data = std::fs::read(&path).map_err(|e| StorageError::Content(e.to_string()))?;

    // Clamp the window to the file; `start` past EOF gives an empty slice.
    let start = std::cmp::min(chunk_index.saturating_mul(CHUNK_SIZE) as usize, data.len());
    let end = std::cmp::min(start.saturating_add(CHUNK_SIZE as usize), data.len());
    Ok(data[start..end].to_vec())
}

/// Whether `s` is exactly `0x` followed by 64 hex characters — the canonical
/// 32-byte work-id form, and the only shape allowed to reach the filesystem.
fn is_work_id(s: &str) -> bool {
    match s.strip_prefix("0x") {
        Some(rest) => rest.len() == 64 && rest.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}

/// Errors the serving core can produce.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// A receipt was requested for a content position the node never fully
    /// delivered — either it was never requested, or the transfer that started
    /// it was abandoned before completion.
    #[error("no chunk was fully delivered for that consumer, work and chunk index")]
    NotServed,
    /// The work id was not a canonical `0x` + 64-hex-character string.
    #[error("malformed work id")]
    BadWorkId,
    /// The content file could not be read.
    #[error("reading content: {0}")]
    Content(String),
}

/// A response body that reports completion, so a chunk is credited only when it
/// has been delivered in full.
///
/// The stream hands out `piece`-sized slices of `data`. When the last slice has
/// been taken and the consumer polls once more, the stream fires `on_complete`
/// and ends. If it is dropped before that — the client abandoned the transfer,
/// the connection broke — `on_complete` never runs and nothing is credited.
///
/// This is what makes crediting all-or-nothing: partial delivery earns zero
/// rather than a proportional share, which removes partial-chunk accounting
/// entirely and makes "was this chunk delivered?" a boolean.
pub struct DeliveryStream {
    /// The chunk's bytes.
    data: Vec<u8>,
    /// How far through `data` the consumer has taken.
    pos: usize,
    /// Slice size handed out per poll.
    piece: usize,
    /// Fired once, when the whole chunk has been yielded. `None` after firing,
    /// which also makes the completion idempotent.
    on_complete: Option<Box<dyn FnOnce() + Send>>,
}

impl DeliveryStream {
    /// Wrap `data`, yielding it in `piece`-sized slices and calling
    /// `on_complete` only if the stream is driven all the way to the end.
    pub fn new(data: Vec<u8>, piece: usize, on_complete: impl FnOnce() + Send + 'static) -> Self {
        DeliveryStream {
            data,
            pos: 0,
            // A zero piece size would spin forever; treat it as "one slice".
            piece: piece.max(1),
            on_complete: Some(Box::new(on_complete)),
        }
    }
}

impl futures_util::Stream for DeliveryStream {
    type Item = Result<bytes::Bytes, std::io::Error>;

    /// Hand out the next slice of `data`, or — once everything has been taken —
    /// fire the completion callback exactly once and end the stream.
    ///
    /// This runs synchronously inside axum's body-streaming machinery, which is
    /// exactly why the ledger it feeds must be a blocking mutex rather than an
    /// async one: there is no executor context here to `.await` on.
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // Everything has been handed out: fire completion exactly once, then end.
        if this.pos >= this.data.len() {
            if let Some(done) = this.on_complete.take() {
                done();
            }
            return std::task::Poll::Ready(None);
        }

        let end = std::cmp::min(this.pos + this.piece, this.data.len());
        let slice = bytes::Bytes::copy_from_slice(&this.data[this.pos..end]);
        this.pos = end;
        std::task::Poll::Ready(Some(Ok(slice)))
    }
}

/// Everything the HTTP handlers share: the node's identity, its content
/// directory, the epoch it is serving, and the ledger of what it has fully
/// delivered.
pub struct NodeState {
    /// The node's signing key; also the source of its address.
    pub signer: PrivateKeySigner,
    /// Where `<work_id>.bin` files live.
    pub content_dir: PathBuf,
    /// The settlement epoch every issued receipt is bound to.
    pub epoch: u64,
    /// What has been fully delivered so far. A blocking mutex rather than an
    /// async one: the delivery stream's completion callback fires inside
    /// `poll_next`, which cannot `.await`, and every ledger operation is a short
    /// map write.
    pub ledger: std::sync::Mutex<Ledger>,
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

/// Liveness probe, so the demo (or a test) can wait for the node to accept
/// connections.
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
/// with the bytes — see the module docs for why that distinction does not
/// matter for a bandwidth measure.
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

/// Build the node's HTTP router over shared `state` — `GET /health`,
/// `GET /content/{work_id}` and `POST /receipt`.
///
/// Kept separate from `main` so both the real binary and tests can drive the
/// exact same routing and handler code; a test that only drove `DeliveryStream`
/// or `Ledger` directly would pin those types without pinning that the
/// `content` handler actually wires the ledger write to stream completion
/// rather than, say, the moment bytes are read off disk.
pub fn router(state: Arc<NodeState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/content/{work_id}", get(content))
        .route("/receipt", post(receipt))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use futures_util::StreamExt;
    use tower::ServiceExt; // for `oneshot`

    /// A canonical work id for the tests.
    fn work() -> String {
        format!("0x{}", "aa".repeat(32))
    }

    /// A ledger holding one delivered chunk (index 0) for `0xc0ffee`.
    fn ledger_with_one_chunk() -> Ledger {
        let mut l = Ledger::default();
        l.record(
            "0xc0ffee",
            &work(),
            0,
            ServedChunk {
                work_id: work(),
                consumer: "0xc0ffee".to_string(),
                bytes: 4096,
            },
        );
        l
    }

    /// The node issues a receipt carrying ITS OWN recorded byte count, bound to
    /// the requested content position.
    #[test]
    fn issues_a_receipt_for_a_delivered_chunk() {
        let l = ledger_with_one_chunk();
        let r = issue_receipt(&l, "0xn0de", 7, "0xc0ffee", &work(), 0).unwrap();
        assert_eq!(r.bytes, 4096);
        assert_eq!(r.epoch, 7);
        assert_eq!(r.chunk_index, 0);
        assert_eq!(r.work_id, work());
        assert_eq!(r.consumer, "0xc0ffee");
        assert_eq!(r.node, "0xn0de");
    }

    /// The node REFUSES to sign for a chunk it never delivered — including one
    /// it started but did not finish, since a partial transfer records nothing.
    #[test]
    fn refuses_to_sign_for_an_undelivered_chunk() {
        let l = ledger_with_one_chunk();
        assert!(matches!(
            issue_receipt(&l, "0xn0de", 7, "0xc0ffee", &work(), 9),
            Err(StorageError::NotServed)
        ));
        assert!(matches!(
            issue_receipt(&l, "0xn0de", 7, "0xdecaf", &work(), 0),
            Err(StorageError::NotServed)
        ));
    }

    /// A stream driven to completion credits the chunk exactly once.
    #[tokio::test]
    async fn fully_consumed_stream_credits_the_chunk() {
        let credited = std::sync::Arc::new(std::sync::Mutex::new(0u64));
        let sink = credited.clone();
        let mut s = DeliveryStream::new(vec![7u8; 1000], 256, move || {
            *sink.lock().unwrap() += 1000;
        });

        let mut total = 0usize;
        while let Some(piece) = s.next().await {
            total += piece.unwrap().len();
        }
        assert_eq!(total, 1000);
        assert_eq!(*credited.lock().unwrap(), 1000);
    }

    /// A stream polled part-way and DROPPED credits NOTHING. This is the whole
    /// point of all-or-nothing crediting: bytes that never finished leaving the
    /// server must not earn evidence.
    #[tokio::test]
    async fn abandoned_stream_credits_nothing() {
        let credited = std::sync::Arc::new(std::sync::Mutex::new(0u64));
        let sink = credited.clone();
        let mut s = DeliveryStream::new(vec![7u8; 1000], 256, move || {
            *sink.lock().unwrap() += 1000;
        });

        // Take one piece, then drop the stream mid-transfer.
        let first = s.next().await.unwrap().unwrap();
        assert_eq!(first.len(), 256);
        drop(s);

        assert_eq!(*credited.lock().unwrap(), 0);
    }

    /// Re-delivering the same chunk overwrites rather than accumulating, so a
    /// client that retries after a failed transfer is attested once — the node
    /// side of the same idempotency the aggregator enforces by dedup key.
    #[test]
    fn redelivering_a_chunk_keeps_one_entry() {
        let mut l = ledger_with_one_chunk();
        l.record(
            "0xc0ffee",
            &work(),
            0,
            ServedChunk {
                work_id: work(),
                consumer: "0xc0ffee".to_string(),
                bytes: 4096,
            },
        );
        assert_eq!(l.get("0xc0ffee", &work(), 0).unwrap().bytes, 4096);
        assert_eq!(
            issue_receipt(&l, "0xn0de", 7, "0xc0ffee", &work(), 0)
                .unwrap()
                .bytes,
            4096
        );
    }

    /// `fragment_for_chunk` returns the chunk's window, and the final chunk is
    /// short rather than padded.
    #[test]
    fn fragment_for_chunk_windows_the_content() {
        let dir = std::env::temp_dir().join("cwe-storage-test-chunks");
        std::fs::create_dir_all(&dir).unwrap();
        let w = format!("0x{}", "ab".repeat(32));
        // One full chunk plus 100 bytes.
        let size = CHUNK_SIZE as usize + 100;
        std::fs::write(dir.join(format!("{w}.bin")), vec![7u8; size]).unwrap();

        assert_eq!(
            fragment_for_chunk(&dir, &w, 0).unwrap().len(),
            CHUNK_SIZE as usize
        );
        assert_eq!(fragment_for_chunk(&dir, &w, 1).unwrap().len(), 100);
        // A chunk index past the end yields nothing rather than erroring.
        assert_eq!(fragment_for_chunk(&dir, &w, 99).unwrap().len(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A malformed work id is rejected before any filesystem access, so a
    /// crafted id cannot escape the content directory.
    #[test]
    fn rejects_a_malformed_work_id() {
        let dir = std::env::temp_dir();
        assert!(matches!(
            fragment_for_chunk(&dir, "../../etc/passwd", 0),
            Err(StorageError::BadWorkId)
        ));
        assert!(matches!(
            fragment_for_chunk(&dir, "0xzz", 0),
            Err(StorageError::BadWorkId)
        ));
    }

    /// A canonical work id distinct from `work()`, for the router-level tests
    /// below — kept separate so a mistake in one test's fixture cannot be
    /// masked by another test's ledger entry for the same coordinates.
    fn router_work() -> String {
        format!("0x{}", "bb".repeat(32))
    }

    /// Build a `NodeState` that serves `content` for `work_id` out of a fresh,
    /// uniquely-named temp directory (`dir_name`), and return it wrapped for
    /// [`router`] alongside the directory path for the test to clean up.
    fn state_serving(dir_name: &str, work_id: &str, content: &[u8]) -> (Arc<NodeState>, PathBuf) {
        let dir = std::env::temp_dir().join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{work_id}.bin")), content).unwrap();
        let state = Arc::new(NodeState {
            signer: PrivateKeySigner::random(),
            content_dir: dir.clone(),
            epoch: 1,
            ledger: std::sync::Mutex::new(Ledger::default()),
        });
        (state, dir)
    }

    /// This is the wiring-level regression test for cycle 2's headline fix: it
    /// drives the actual `content` handler through `router`, not the bare
    /// `DeliveryStream` type, so a future refactor that moves the ledger write
    /// back into the handler (cycle 1's bug) is caught here even though
    /// `abandoned_stream_credits_nothing` above would not notice it.
    ///
    /// Polling exactly one frame and dropping the body without draining the
    /// rest is deterministic and needs no socket: axum streams the response
    /// body lazily, so a client that stops polling never reaches the stream's
    /// completion callback, and the ledger it would have written to stays
    /// empty.
    #[tokio::test]
    async fn abandoned_http_delivery_yields_no_receipt() {
        let work_id = router_work();
        let (state, dir) = state_serving("cwe-storage-test-abandoned", &work_id, &[7u8; 4096]);
        let app = router(state);
        let consumer = "0xc0ffee";

        let resp = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/content/{work_id}?consumer={consumer}&chunk_index=0"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Poll exactly one frame, then drop — never reaching the end of the
        // stream, so `on_complete` never fires and nothing is credited.
        let mut data = resp.into_body().into_data_stream();
        data.next().await.unwrap().unwrap();
        drop(data);

        let receipt_req = serde_json::json!({
            "work_id": work_id,
            "chunk_index": 0,
            "consumer": consumer,
        })
        .to_string();
        let resp = app
            .oneshot(
                Request::post("/receipt")
                    .header("content-type", "application/json")
                    .body(Body::from(receipt_req))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The companion positive case: a fully drained response body DOES reach
    /// the stream's completion callback, so the same content position is
    /// creditable and a receipt is issued.
    #[tokio::test]
    async fn fully_drained_http_delivery_yields_a_receipt() {
        let work_id = router_work();
        let (state, dir) = state_serving("cwe-storage-test-full-drain", &work_id, &[7u8; 4096]);
        let app = router(state);
        let consumer = "0xc0ffee";

        let resp = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/content/{work_id}?consumer={consumer}&chunk_index=0"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // Drain the body fully so the stream reaches completion and credits
        // the ledger, mirroring what a real HTTP client does when it reads
        // the whole response.
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.len(), 4096);

        let receipt_req = serde_json::json!({
            "work_id": work_id,
            "chunk_index": 0,
            "consumer": consumer,
        })
        .to_string();
        let resp = app
            .oneshot(
                Request::post("/receipt")
                    .header("content-type", "application/json")
                    .body(Body::from(receipt_req))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        std::fs::remove_dir_all(&dir).ok();
    }
}
