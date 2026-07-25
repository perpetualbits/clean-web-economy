//! The storage node's serving core (H5 cycle 1).
//!
//! Two responsibilities, both deliberately free of network and chain code so
//! they can be tested directly: reading a fragment of a work's content out of a
//! content directory, and remembering exactly what was served to whom so a
//! receipt can be issued for it later.
//!
//! The node only ever signs byte counts from its own ledger. A consumer cannot
//! talk it into attesting bytes that never left the disk, which is precisely the
//! guarantee the aggregator relies on when it turns receipts into bandwidth
//! credibility.

use std::collections::BTreeMap;
use std::path::Path;

use cwe_receipt::{normalize_addr, Receipt};

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

/// The node's in-memory record of everything it has served this run, keyed by
/// `(session_nonce, chunk_nonce)` — the same coordinates a receipt request
/// arrives with.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Served chunks by `(session_nonce, chunk_nonce)`. A repeat of the same key
    /// overwrites rather than accumulating, so a retried download cannot inflate
    /// the count the node is willing to attest.
    entries: BTreeMap<(String, u64), ServedChunk>,
}

impl Ledger {
    /// Record that `served` was delivered for `session`/`chunk`, replacing any
    /// previous record for that exact key.
    pub fn record(&mut self, session: &str, chunk: u64, served: ServedChunk) {
        self.entries
            .insert((session.to_ascii_lowercase(), chunk), served);
    }

    /// Look up what was served for `session`/`chunk`, if anything.
    pub fn get(&self, session: &str, chunk: u64) -> Option<&ServedChunk> {
        self.entries.get(&(session.to_ascii_lowercase(), chunk))
    }
}

/// Build the receipt this node is willing to sign for `session`/`chunk`.
///
/// The byte count comes from the ledger, never from the caller. If the node has
/// no record of serving that chunk it returns [`StorageError::NotServed`] and
/// signs nothing — a fabricated receipt request gets no signature, so a
/// colluding client is left holding a half-signed receipt the aggregator drops.
pub fn issue_receipt(
    ledger: &Ledger,
    node_addr: &str,
    epoch: u64,
    session: &str,
    chunk: u64,
) -> Result<Receipt, StorageError> {
    let served = ledger.get(session, chunk).ok_or(StorageError::NotServed)?;
    Ok(Receipt {
        work_id: served.work_id.to_ascii_lowercase(),
        consumer: normalize_addr(&served.consumer),
        node: normalize_addr(node_addr),
        bytes: served.bytes,
        epoch,
        session_nonce: session.to_ascii_lowercase(),
        chunk_nonce: chunk,
    })
}

/// Read `len` bytes of `work_id`'s content starting at `offset`.
///
/// Content lives at `{dir}/{work_id}.bin`. The read is clamped to the end of the
/// file, so asking for more than remains yields what remains (and an offset past
/// the end yields an empty slice) rather than an error — a short read is a fact
/// about the content, not a failure.
///
/// `work_id` is validated to be exactly `0x` followed by 64 hex characters
/// BEFORE it is joined onto `dir`; that check is what keeps a crafted id from
/// escaping the content directory.
pub fn fragment(dir: &Path, work_id: &str, offset: u64, len: u64) -> Result<Vec<u8>, StorageError> {
    if !is_work_id(work_id) {
        return Err(StorageError::BadWorkId);
    }
    let path = dir.join(format!("{}.bin", work_id.to_ascii_lowercase()));
    let data = std::fs::read(&path).map_err(|e| StorageError::Content(e.to_string()))?;

    // Clamp the window to the file: `start` past EOF gives an empty slice.
    let start = std::cmp::min(offset as usize, data.len());
    let end = std::cmp::min(start.saturating_add(len as usize), data.len());
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
    /// A receipt was requested for a (session, chunk) the node never served.
    #[error("no bytes were served for that session and chunk")]
    NotServed,
    /// The work id was not a canonical `0x` + 64-hex-character string.
    #[error("malformed work id")]
    BadWorkId,
    /// The content file could not be read.
    #[error("reading content: {0}")]
    Content(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ledger holding one served chunk for `sess`/`chunk 0`.
    fn ledger_with_one_chunk() -> Ledger {
        let mut l = Ledger::default();
        l.record(
            "0xfeed",
            0,
            ServedChunk {
                work_id: format!("0x{}", "aa".repeat(32)),
                consumer: "0xc0ffee".to_string(),
                bytes: 4096,
            },
        );
        l
    }

    /// The node issues a receipt carrying ITS OWN recorded byte count, bound to
    /// the requested session and chunk.
    #[test]
    fn issues_a_receipt_for_a_served_chunk() {
        let l = ledger_with_one_chunk();
        let r = issue_receipt(&l, "0xn0de", 7, "0xfeed", 0).unwrap();
        assert_eq!(r.bytes, 4096);
        assert_eq!(r.epoch, 7);
        assert_eq!(r.chunk_nonce, 0);
        assert_eq!(r.session_nonce, "0xfeed");
        assert_eq!(r.node, "0xn0de");
        assert_eq!(r.consumer, "0xc0ffee");
    }

    /// The node REFUSES to sign a receipt for a chunk it never served — the
    /// property that stops a client fabricating byte counts.
    #[test]
    fn refuses_to_sign_for_bytes_it_did_not_serve() {
        let l = ledger_with_one_chunk();
        assert!(matches!(
            issue_receipt(&l, "0xn0de", 7, "0xfeed", 99),
            Err(StorageError::NotServed)
        ));
        assert!(matches!(
            issue_receipt(&l, "0xn0de", 7, "0xdead", 0),
            Err(StorageError::NotServed)
        ));
    }

    /// Recording the same (session, chunk) twice does not double-count: the
    /// ledger holds one entry per key.
    #[test]
    fn recording_the_same_chunk_twice_keeps_one_entry() {
        let mut l = ledger_with_one_chunk();
        l.record(
            "0xfeed",
            0,
            ServedChunk {
                work_id: format!("0x{}", "aa".repeat(32)),
                consumer: "0xc0ffee".to_string(),
                bytes: 999_999,
            },
        );
        assert_eq!(l.get("0xfeed", 0).unwrap().bytes, 999_999);
        assert_eq!(
            issue_receipt(&l, "0xn0de", 7, "0xfeed", 0).unwrap().bytes,
            999_999
        );
    }

    /// `fragment` returns exactly the requested window and clamps at EOF.
    #[test]
    fn fragment_reads_a_window_and_clamps_at_eof() {
        let dir = std::env::temp_dir().join("cwe-storage-test-frag");
        std::fs::create_dir_all(&dir).unwrap();
        let work = format!("0x{}", "ab".repeat(32));
        std::fs::write(dir.join(format!("{work}.bin")), vec![7u8; 1000]).unwrap();

        assert_eq!(fragment(&dir, &work, 0, 256).unwrap().len(), 256);
        // Asking past the end yields only what remains, not an error.
        assert_eq!(fragment(&dir, &work, 900, 500).unwrap().len(), 100);
        // Starting past the end yields nothing.
        assert_eq!(fragment(&dir, &work, 5000, 10).unwrap().len(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A work id that is not exactly 0x + 64 hex chars is rejected before any
    /// filesystem access, so a crafted id cannot escape the content directory.
    #[test]
    fn rejects_a_malformed_work_id() {
        let dir = std::env::temp_dir();
        assert!(matches!(
            fragment(&dir, "../../etc/passwd", 0, 16),
            Err(StorageError::BadWorkId)
        ));
        assert!(matches!(
            fragment(&dir, "0xzz", 0, 16),
            Err(StorageError::BadWorkId)
        ));
    }
}
