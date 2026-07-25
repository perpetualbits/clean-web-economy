//! The storage node's serving core (H5 cycle 2).
//!
//! Two responsibilities, both deliberately free of network and chain code so
//! they can be tested directly: reading a `CHUNK_SIZE` block of a work's
//! content out of a content directory by chunk index, and remembering exactly
//! which content positions have been FULLY DELIVERED to whom so a receipt can
//! be issued for them later.
//!
//! The node only ever signs byte counts from its own ledger, and the ledger
//! only ever holds a chunk once it has been handed to the transport in full —
//! see [`DeliveryStream`]. A consumer cannot talk it into attesting bytes that
//! never left the server, and an abandoned transfer earns nothing rather than
//! a proportional share, which is precisely the guarantee the aggregator
//! relies on when it turns receipts into bandwidth credibility.

use std::collections::BTreeMap;
use std::path::Path;

use cwe_receipt::{normalize_addr, Receipt, CHUNK_SIZE};

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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

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
}
