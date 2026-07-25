# H5 cycle 2 — credibility integrity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two deterrent gaps cycle 1 documented — receipts that can be minted by
re-fetching, and an evidence requirement that collapses to one byte — and move `RATE(W)`
from an aggregator config file onto the chain.

**Architecture:** Receipts stop identifying a *session position* and start identifying a
*content position* (`chunk_index`), so credit dedups on `(consumer, work_id, chunk_index)`
and is capped at a work's content size. The storage node credits a chunk all-or-nothing,
only once the whole chunk has been yielded to the transport, so bytes that never left the
server earn nothing. Settlement adds a per-user, per-epoch absolute evidence floor beneath
cycle 1's per-row ratio, and reads each work's bandwidth rate from `CWERegistry` instead of
a config table. `cwe-dapr` is not touched.

**Tech Stack:** Rust (workspace crates), `alloy` v2 (signing, recovery, contract calls),
`axum` 0.8 + `tokio` (node), `futures-util` + `bytes` (the delivery stream), `reqwest`
(client), `serde_jcs` (canonical JSON), Solidity/Foundry (`CWERegistry`), bash + Anvil.

**Governing spec:** `docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md`.
Read it before Task 1. Decisions E1–E6 and §2 (why the three changes compose) are binding.

## Global Constraints

- **No AI attribution anywhere** — not in code, comments, docs, commit messages, branch
  names, or anything pushed to GitHub. Hard project rule (`CLAUDE.md`).
- **Rust everywhere** except the Solidity under `chain/`.
- **Every function/method gets a `///` doc comment.** Non-trivial lines get an inline
  comment only where it adds understanding, never noise restating the code.
- **Deterministic integer math only** in credibility/payout paths. No floating point.
  ppm values live in `[0, 1_000_000]`.
- **`sims` (`cwe-dapr`) must not be modified.** The epoch factor is applied in settlement.
  If a task seems to need a DAPR change, stop and report — the design says it does not.
- **The full gate must be green at the END of the branch:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `(cd chain && forge test)`. Do **not** run `forge fmt`.
- **Expected red window — Tasks 1 through 6.** Task 1 changes the `Receipt` struct, which
  is a breaking change for three downstream targets, each repaired by a different task:
  the `cwe-storage` **library and node binary** (Task 2), the `bandwidth-client` **binary
  inside the same crate** (Task 3), and `cwe-settlement` (Task 6). This is inherent to a
  breaking type change and is **not** a defect. Implementers and reviewers for Tasks 1-5
  must verify with **target-scoped** commands and must **not** treat a build failure in a
  target another task owns as a finding:
  - Task 1: `cargo test -p cwe-receipt`, `cargo clippy -p cwe-receipt --all-targets -- -D warnings`
  - Task 2: `cargo test -p cwe-storage --lib`, `cargo build -p cwe-storage --lib --bin cwe-storage`,
    `cargo clippy -p cwe-storage --lib --bin cwe-storage -- -D warnings`
    (**not** `--all-targets`, and **not** a bare `-p cwe-storage` — both pull in
    `bandwidth_client.rs`, which is Task 3's file and still references the removed fields)
  - Task 3 onward: `-p cwe-storage` unscoped works again
  - Tasks 4-5: their own crates only
  `cargo fmt --all -- --check` works throughout and stays required. From Task 6 onward the
  full workspace gate applies again and must pass.
- **Never kill a process you did not start.** Capture exact PIDs (`cmd & PID=$!`) and kill
  only those. No `pkill`/`killall`/pattern kills — one previously killed a user's browser.
- Foundry lives at `$HOME/.foundry/bin`; prepend it to `PATH`.
- `cargo test --workspace` is slow (~4 min, ZK trusted-setup tests). Scope to `-p <crate>`
  while iterating.

## Protocol constants (used across tasks — keep these values identical everywhere)

```
CHUNK_SIZE          = 131_072          // bytes; libs/receipt
MIN_EPOCH_BYTES     = 131_072          // bytes; settlement default (one whole chunk)
MIN_BANDWIDTH_RATE  = 60_000           // CWERegistry clamp, ≈ 8 kbps
MAX_BANDWIDTH_RATE  = 2_000_000_000    // CWERegistry clamp, ≈ 266 Mbps
RATE_SCALE          = 1_000_000_000_000  // unchanged from cycle 1
```

The demo's proven per-row weight is exactly `45_000_000_000_000` (`minutes=60`,
`price_ppm=1e6`, `region_ppm=1e6`, `plays=2`, `d_ppm(2,1e6)=750_000`). At a bandwidth rate
of `60_000` the expectation is `45 × 60_000 = 2_700_000` bytes, so the demo's content file
must be at least that to let the honest act reach full credibility. Task 7 uses a 4 MiB
file (32 chunks).

## File Structure

| Path | Responsibility |
|---|---|
| `libs/receipt/src/lib.rs` | `Receipt` reshaped to carry `chunk_index`; `CHUNK_SIZE`; dedup key on content position |
| `services/storage/src/lib.rs` | Ledger keyed by content position; `DeliveryStream` (all-or-nothing credit); `fragment_for_chunk` |
| `services/storage/src/main.rs` | `/content` takes `chunk_index`; streams the chunk; `std::sync::Mutex` ledger |
| `services/storage/src/bin/bandwidth_client.rs` | Requests by chunk index; `--mode honest\|no-download\|refetch` |
| `chain/contracts/CWERegistry.sol` | `bandwidthRate` field, clamped registrant-only setter, getter |
| `chain/test/CWERegistry.t.sol` | Setter/clamp/getter coverage |
| `services/discovery-hub/src/manifest.rs`, `src/chain.rs` | Manifest mirrors `bandwidth_rate`; validated against chain |
| `services/settlement/src/receipts.rs` | Epoch factor; credibility takes the factor |
| `services/settlement/src/config.rs` | `RATES` removed; `MIN_EPOCH_BYTES` added |
| `services/settlement/src/chain.rs` | Rates read from `CWERegistry`; verified-bytes sidecar |
| `ops/demo/run_bandwidth_demo.sh`, `.github/workflows/ci.yml` | Six-act demo |
| `ROADMAP.md`, `docs/roadmap.md`, `project-map.js` | Status sync at merge |

---

### Task 1: `cwe-receipt` — receipts bind content position

**Files:**
- Modify: `libs/receipt/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in the same file

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const CHUNK_SIZE: u64 = 131_072;`
  - `pub struct Receipt { work_id: String, consumer: String, node: String, chunk_index: u64, bytes: u64, epoch: u64 }`
  - `Receipt::dedup_key(&self) -> (String, String, u64)` returning `(consumer, work_id, chunk_index)`, all lowercased
  - `Receipt::offset(&self) -> u64`
  - `canonical_bytes`, `recover_signer`, `SignedReceipt`, `ReceiptBundle`, `normalize_addr`, `ReceiptError` — all unchanged in signature

**Design notes:**
- `session_nonce` and `chunk_nonce` are **deleted**, not deprecated. Both were
  client-chosen, which is what let a fresh session mint fresh credit; leaving them would
  imply protection they no longer provide (spec §3.1).
- The dedup key moving to `(consumer, work_id, chunk_index)` also fixes a cycle-1 latent
  double-count: two different nodes serving the same chunk to the same consumer used to
  count twice. There is a required test for exactly this.
- This is a breaking format change and deliberately un-versioned (decision E6).

- [ ] **Step 1: Update the failing tests**

Replace the `sample` helper and add the new cases in `libs/receipt/src/lib.rs`'s test
module. Existing signature/tamper tests keep their names and intent; they just build the
new shape.

```rust
    /// Build a receipt for `consumer`/`node`, with everything else fixed.
    fn sample(consumer: &str, node: &str) -> Receipt {
        Receipt {
            work_id: format!("0x{}", "aa".repeat(32)),
            consumer: consumer.to_string(),
            node: node.to_string(),
            chunk_index: 3,
            bytes: 131_072,
            epoch: 7,
        }
    }

    /// The dedup key is (consumer, work_id, chunk_index) — content position,
    /// not session position. Two receipts for the same chunk of the same work
    /// to the same consumer collide even when a DIFFERENT node served them,
    /// because it is the same content either way. (Cycle 1 keyed on the node
    /// and double-counted this case.)
    #[test]
    fn dedup_key_is_content_position_not_node() {
        let a = sample("0xc0ffee", "0xnode1");
        let mut b = sample("0xc0ffee", "0xnode2");
        b.bytes = 1; // a replay may differ in payload; the key must still collide
        assert_eq!(a.dedup_key(), b.dedup_key());

        // A different chunk of the same work is genuinely distinct evidence.
        let mut c = a.clone();
        c.chunk_index += 1;
        assert_ne!(a.dedup_key(), c.dedup_key());

        // So is the same chunk delivered to a different consumer.
        let d = sample("0xdecaf", "0xnode1");
        assert_ne!(a.dedup_key(), d.dedup_key());
    }

    /// The dedup key normalises case, so a mixed-case receipt cannot evade it.
    #[test]
    fn dedup_key_normalises_case() {
        let a = sample("0xc0ffee", "0xnode1");
        let b = sample("0xC0FFEE", "0xNODE1");
        assert_eq!(a.dedup_key(), b.dedup_key());
    }

    /// A chunk index maps to its byte offset in the content.
    #[test]
    fn offset_follows_chunk_index() {
        let mut r = sample("0xc0ffee", "0xnode1");
        r.chunk_index = 0;
        assert_eq!(r.offset(), 0);
        r.chunk_index = 5;
        assert_eq!(r.offset(), 5 * CHUNK_SIZE);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-receipt`
Expected: FAIL — `Receipt` has no field `chunk_index`, no method `offset`.

- [ ] **Step 3: Reshape `Receipt`**

Replace the `Receipt` struct and its `dedup_key`, and add `CHUNK_SIZE` and `offset`:

```rust
/// The fixed content-block size a receipt attests, in bytes.
///
/// Content is addressed as a sequence of `CHUNK_SIZE` blocks (the final block of
/// a work is short). Node, client and aggregator all use this one constant — a
/// disagreement would make honest receipts undeduplicatable.
pub const CHUNK_SIZE: u64 = 131_072;

/// One co-signable statement that a specific BLOCK of `work_id`'s content was
/// delivered from `node` to `consumer` during `epoch`.
///
/// The receipt identifies content position (`chunk_index`), not session
/// position. That is what bounds the evidence a consumer can accumulate: each
/// block of a work counts once, so total credit for a (consumer, work) pair is
/// capped at the work's size however many requests are issued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// The work whose content moved, as a lowercase `0x` 32-byte hex string.
    pub work_id: String,
    /// The consumer that received the bytes — the same address that submits the
    /// matching usage on-chain, so bytes attribute to the right DAPR row.
    pub consumer: String,
    /// The serving storage node; must hold a valid storage-node credential for
    /// this receipt to count.
    pub node: String,
    /// Which `CHUNK_SIZE` block of the work's content this attests.
    pub chunk_index: u64,
    /// Bytes actually delivered for that block. Equals `CHUNK_SIZE` except for a
    /// work's final, short block.
    pub bytes: u64,
    /// The settlement epoch this receipt belongs to.
    pub epoch: u64,
}

impl Receipt {
    /// The byte offset into the work's content at which this chunk begins.
    pub fn offset(&self) -> u64 {
        self.chunk_index.saturating_mul(CHUNK_SIZE)
    }

    /// The credit-dedup key `(consumer, work_id, chunk_index)`.
    ///
    /// Each distinct block of a work earns credit once per consumer per epoch.
    /// The node is deliberately NOT part of the key: two nodes serving the same
    /// block to the same consumer moved the same content, and crediting both
    /// would double-count it. `bytes` is excluded so a replay claiming a larger
    /// count still collides.
    pub fn dedup_key(&self) -> (String, String, u64) {
        (
            normalize_addr(&self.consumer),
            self.work_id.to_ascii_lowercase(),
            self.chunk_index,
        )
    }
}
```

Keep `canonical_bytes` and `recover_signer` exactly as they are — they operate on the
struct generically and need no change.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cwe-receipt`
Expected: PASS — including the pre-existing signature, tamper and malformed-signature
tests, which must keep their original names and assertions.

- [ ] **Step 5: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-receipt --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add libs/receipt/src/lib.rs
git commit -m "receipt: bind receipts to content position and dedup on (consumer, work, chunk)"
```

---

### Task 2: `cwe-storage` node — credit only fully delivered chunks

**Files:**
- Modify: `services/storage/Cargo.toml` (add `futures-util`, `bytes`)
- Modify: `services/storage/src/lib.rs`
- Modify: `services/storage/src/main.rs`
- Test: inline `#[cfg(test)] mod tests` in `services/storage/src/lib.rs`

**Interfaces:**
- Consumes: `cwe_receipt::{CHUNK_SIZE, Receipt, normalize_addr}` (Task 1).
- Produces:
  - `pub struct ServedChunk { work_id: String, consumer: String, bytes: u64 }` (unchanged shape)
  - `Ledger::record(&mut self, consumer: &str, work_id: &str, chunk_index: u64, served: ServedChunk)`
  - `Ledger::get(&self, consumer: &str, work_id: &str, chunk_index: u64) -> Option<&ServedChunk>`
  - `pub fn issue_receipt(ledger: &Ledger, node_addr: &str, epoch: u64, consumer: &str, work_id: &str, chunk_index: u64) -> Result<Receipt, StorageError>`
  - `pub fn fragment_for_chunk(dir: &Path, work_id: &str, chunk_index: u64) -> Result<Vec<u8>, StorageError>`
  - `pub struct DeliveryStream` with `DeliveryStream::new(data: Vec<u8>, piece: usize, on_complete: impl FnOnce() + Send + 'static) -> Self`
  - HTTP: `GET /content/{work_id}?consumer={addr}&chunk_index={n}` → streamed bytes;
    `POST /receipt` body `{"work_id": "0x..", "chunk_index": n, "consumer": "0x.."}` →
    `{"receipt": {...}, "node_sig": "0x.."}`; `GET /health`

**Design notes:**
- **All-or-nothing credit (spec §3.2).** The ledger entry is written only when the whole
  chunk has been yielded. An abandoned transfer credits nothing; the client re-requests
  the same index and Task 1's dedup makes the retry idempotent.
- **The ledger moves from `tokio::sync::RwLock` to `std::sync::Mutex`.** The completion
  callback fires inside `Stream::poll_next`, which is synchronous — it cannot `.await`.
  Ledger operations are short map writes, so a blocking mutex is correct here and simpler
  than plumbing a channel. Hold the guard only for the insert.
- `offset`/`len` disappear from the query: a chunk index fully determines the window.

- [ ] **Step 1: Add the two dependencies**

In `services/storage/Cargo.toml` `[dependencies]`:

```toml
# The delivery stream credits a chunk only once fully yielded, so it needs the
# Stream trait and the Bytes buffer axum's streaming body is built from.
futures-util = { version = "0.3", default-features = false, features = ["std"] }
bytes = "1"
```

- [ ] **Step 2: Write the failing tests**

Replace `services/storage/src/lib.rs`'s test module with:

```rust
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
            ServedChunk { work_id: work(), consumer: "0xc0ffee".to_string(), bytes: 4096 },
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
            issue_receipt(&l, "0xdecaf", 7, "0xdecaf", &work(), 0),
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
            ServedChunk { work_id: work(), consumer: "0xc0ffee".to_string(), bytes: 4096 },
        );
        assert_eq!(l.get("0xc0ffee", &work(), 0).unwrap().bytes, 4096);
        assert_eq!(
            issue_receipt(&l, "0xn0de", 7, "0xc0ffee", &work(), 0).unwrap().bytes,
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

        assert_eq!(fragment_for_chunk(&dir, &w, 0).unwrap().len(), CHUNK_SIZE as usize);
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
        assert!(matches!(fragment_for_chunk(&dir, "0xzz", 0), Err(StorageError::BadWorkId)));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p cwe-storage --lib`
Expected: FAIL — `DeliveryStream` and `fragment_for_chunk` not found; `Ledger::record`
takes the wrong arguments.

- [ ] **Step 4: Rewrite the ledger keying and add the chunk window**

In `services/storage/src/lib.rs`, replace the `Ledger`, `issue_receipt` and `fragment`
items with:

```rust
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
    let start = std::cmp::min(
        chunk_index.saturating_mul(CHUNK_SIZE) as usize,
        data.len(),
    );
    let end = std::cmp::min(start.saturating_add(CHUNK_SIZE as usize), data.len());
    Ok(data[start..end].to_vec())
}
```

Update the imports at the top of the file to `use cwe_receipt::{normalize_addr, Receipt, CHUNK_SIZE};`.

- [ ] **Step 5: Add the delivery stream**

Append to `services/storage/src/lib.rs`:

```rust
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
```

- [ ] **Step 6: Run the library tests to verify they pass**

Run: `cargo test -p cwe-storage --lib`
Expected: PASS — 7 tests, including `abandoned_stream_credits_nothing`.

- [ ] **Step 7: Rewire the HTTP handlers**

In `services/storage/src/main.rs`:

Change the shared state's ledger to a blocking mutex, because the completion callback runs
inside a synchronous `poll_next` and cannot await:

```rust
    /// What has been fully delivered so far. A blocking mutex rather than an
    /// async one: the delivery stream's completion callback fires inside
    /// `poll_next`, which cannot `.await`, and every ledger operation is a short
    /// map write.
    ledger: std::sync::Mutex<Ledger>,
```

Replace `ContentQuery` and the `content` handler:

```rust
/// Query parameters of `GET /content/{work_id}`.
#[derive(Debug, Deserialize)]
struct ContentQuery {
    /// The address the bytes are being delivered to; bound into the receipt.
    consumer: String,
    /// Which `CHUNK_SIZE` block of the work's content to deliver.
    chunk_index: u64,
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
```

Replace `ReceiptRequest` and the receipt handler's lookup:

```rust
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
```

and inside the handler, replace the ledger read and `issue_receipt` call with:

```rust
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
```

Update the crate-level `//!` docs so the configuration list and the delivery description
match the new behaviour: the node credits a chunk only on full delivery, and addresses
content by chunk index.

- [ ] **Step 8: Build and re-run**

Run: `cargo build -p cwe-storage && cargo test -p cwe-storage`
Expected: builds clean; 6 lib tests pass.

- [ ] **Step 9: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-storage --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 10: Commit**

```bash
git add services/storage/Cargo.toml services/storage/src/lib.rs services/storage/src/main.rs Cargo.lock
git commit -m "storage: address content by chunk index and credit only fully delivered chunks"
```

---

### Task 3: `bandwidth-client` — chunk-indexed, with adversarial modes

**Files:**
- Modify: `services/storage/src/bin/bandwidth_client.rs`

**Interfaces:**
- Consumes: `cwe_receipt::{CHUNK_SIZE, Receipt, ReceiptBundle, SignedReceipt}`; the node's
  HTTP surface from Task 2.
- Produces: `bandwidth-client --mode honest|no-download|refetch`. Environment: `STORAGE_URL`
  (default `http://127.0.0.1:8546`), `WORK_ID`, `PRIVATE_KEY`, `EPOCH`, `CHUNKS`
  (default `4`), `OUT`, and `REPEATS` (default `100`, `refetch` mode only).

**Design notes:**
- The adversarial modes exist so the demo can prove the fixes actually bite. This follows
  the pattern `zk_submit --mode tamper-digest` already established in this repo.
- The five field checks added in cycle 1 stay, updated to the new shape.

**Why the "never downloaded" mode is `no-download` and not "abandon mid-transfer".**
The obvious adversarial mode — start a chunk, read one slice, drop the connection — cannot
be asserted on deterministically over loopback. The node writes 16 KiB pieces into a
socket whose buffers auto-tune to megabytes, so a whole 128 KiB chunk is usually absorbed
by the kernel before the client's abort is noticed; the stream then completes legitimately
and the chunk is credited. That is not a bug — the bytes really did leave the server, which
is exactly what the measure counts (spec §3.2: receive-and-discard is an ordinary download,
not a threat) — but it makes any byte-exact demo assertion a race.

Mid-transfer abandonment is therefore proven **deterministically at the unit level** by
Task 2's `abandoned_stream_credits_nothing`, which drives the stream directly with no
sockets involved. The demo instead exercises the same trust property over HTTP in a form
that has no race: a client that requests **receipts for chunks it never downloaded at
all**. The node has no ledger entry, refuses every request, and the client ends with zero
receipts — every time.

- [ ] **Step 1: Rewrite the download and mode dispatch**

Replace the body of `services/storage/src/bin/bandwidth_client.rs` below the imports.
Update the module docs to describe the three modes. Add to the imports:

```rust
use futures_util::StreamExt;
```

The core loop:

```rust
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
        return Err(format!("chunk {chunk_index}: node signature does not match its address").into());
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
    let consumer_sig = format!("0x{}", hex_string(&signer.sign_message_sync(&msg)?.as_bytes()));
    Ok(Some(SignedReceipt {
        receipt: resp.receipt,
        node_sig: resp.node_sig,
        consumer_sig,
    }))
}
```

And the `main` dispatch:

```rust
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
                let got =
                    fetch_chunk_fully(&http, &base, &work_id, &consumer, chunk_index).await?;
                if got == 0 {
                    break; // past the end of the content
                }
                if let Some(sr) = co_sign_chunk(
                    &http, &base, &signer, &work_id, &consumer, epoch, chunk_index, Some(got),
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
                    &http, &base, &signer, &work_id, &consumer, epoch, chunk_index, None,
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
                    &http, &base, &signer, &work_id, &consumer, epoch, 0, Some(got),
                )
                .await?
                {
                    signed.push(sr);
                }
            }
        }
        other => return Err(format!("unknown mode '{other}'").into()),
    }

    let bundle = ReceiptBundle { epoch, receipts: signed };
    std::fs::write(&out, bundle.to_json()?)?;
    println!(
        "bandwidth-client[{mode}]: {} receipts for work {work_id} → {out}",
        bundle.receipts.len()
    );
    Ok(())
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p cwe-storage --bin bandwidth-client`
Expected: builds clean.

- [ ] **Step 3: Smoke-test all three modes against a live node**

```bash
TMP=$(mktemp -d)
WORK=0x$(printf 'ab%.0s' {1..32})
# 4 chunks of 128 KiB.
head -c 524288 /dev/zero | tr '\0' 'x' > "$TMP/$WORK.bin"
NODE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
CONS_KEY=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d

CONTENT_DIR=$TMP PRIVATE_KEY=$NODE_KEY EPOCH=1 PORT=8546 \
  cargo run -q -p cwe-storage --bin cwe-storage &
NODE_PID=$!
until curl -sf http://127.0.0.1:8546/health >/dev/null; do :; done

for M in honest no-download refetch; do
  STORAGE_URL=http://127.0.0.1:8546 WORK_ID=$WORK PRIVATE_KEY=$CONS_KEY EPOCH=1 \
    CHUNKS=4 REPEATS=10 OUT=$TMP/$M.json \
    cargo run -q -p cwe-storage --bin bandwidth-client -- --mode $M
done

# Kill ONLY the node we started, by the exact PID we captured.
kill -TERM "$NODE_PID"
echo "honest receipts:  $(jq '.receipts | length' $TMP/honest.json)"
echo "no-download receipts: $(jq '.receipts | length' $TMP/no-download.json)"
echo "refetch receipts: $(jq '.receipts | length' $TMP/refetch.json)"
rm -rf "$TMP"
```

Expected: `honest receipts: 4`, **`no-download receipts: 0`** (the node refused every one),
`refetch receipts: 10` (all for chunk 0 — the aggregator, not the client, is what
collapses them).

If `no-download` produces ANY receipts, the node is signing for content it never
delivered — the core trust property is broken. Do not proceed; report it.

- [ ] **Step 4: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-storage --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add services/storage/src/bin/bandwidth_client.rs
git commit -m "storage: chunk-indexed client with no-download and refetch modes for the demo"
```

---

### Task 4: `CWERegistry.bandwidthRate`

**Files:**
- Modify: `chain/contracts/CWERegistry.sol`
- Test: `chain/test/CWERegistry.t.sol`

**Interfaces:**
- Produces: `setBandwidthRate(bytes32,uint256)`, `bandwidthRateOf(bytes32) → uint256`,
  constants `MIN_BANDWIDTH_RATE = 60_000` and `MAX_BANDWIDTH_RATE = 2_000_000_000`,
  event `BandwidthRateSet(bytes32 indexed workId, uint256 rate)`.

**Design notes:**
- **Additive only (decision E5).** `registerWork`'s signature must NOT change — every
  demo, script and test that registers a work would otherwise need migrating.
- Unset is `0`, which settlement already treats as fail-closed, so existing registrations
  stay safe by default.
- The contract already declares `error NotRegistrant()` and uses it for exactly this
  check. An **unregistered** work needs no separate error: its `registrant` is the zero
  address, so `msg.sender != w.registrant` reverts `NotRegistrant` naturally. Add only
  `BadBandwidthRate`.

- [ ] **Step 1: Write the failing tests**

Append to `chain/test/CWERegistry.t.sol`, adapting the fixture names to the ones the file
already uses (read its `setUp` first):

```solidity
    /// @notice The clamp bounds are exactly the agreed protocol constants. The
    ///         aggregator's expectations are calibrated against these, so a
    ///         silent change here must fail loudly.
    function test_bandwidthRate_boundsArePinned() public {
        assertEq(registry.MIN_BANDWIDTH_RATE(), 60_000);
        assertEq(registry.MAX_BANDWIDTH_RATE(), 2_000_000_000);
    }

    /// @notice An unset work reports a zero rate, which the aggregator treats as
    ///         fail-closed — so every pre-existing registration stays safe.
    function test_bandwidthRate_defaultsToZero() public {
        assertEq(registry.bandwidthRateOf(WORK), 0);
    }

    /// @notice The registrant can set a rate inside the clamp, and it reads back.
    function test_bandwidthRate_registrantCanSet() public {
        vm.prank(creator);
        registry.setBandwidthRate(WORK, 960_000);
        assertEq(registry.bandwidthRateOf(WORK), 960_000);
    }

    /// @notice Both bounds are inclusive.
    function test_bandwidthRate_boundsAreInclusive() public {
        vm.prank(creator);
        registry.setBandwidthRate(WORK, 60_000);
        assertEq(registry.bandwidthRateOf(WORK), 60_000);

        vm.prank(creator);
        registry.setBandwidthRate(WORK, 2_000_000_000);
        assertEq(registry.bandwidthRateOf(WORK), 2_000_000_000);
    }

    /// @notice A rate below the floor is refused — the floor is what stops a
    ///         creator making their own work trivially cheap to prove.
    function test_bandwidthRate_rejectsBelowMin() public {
        vm.prank(creator);
        vm.expectRevert();
        registry.setBandwidthRate(WORK, 59_999);
    }

    /// @notice A rate above the ceiling is refused, so a mistyped value cannot
    ///         demand unmeetable evidence and burn an honest creator's earnings.
    function test_bandwidthRate_rejectsAboveMax() public {
        vm.prank(creator);
        vm.expectRevert();
        registry.setBandwidthRate(WORK, 2_000_000_001);
    }

    /// @notice Only the registrant may set it.
    function test_bandwidthRate_onlyRegistrant() public {
        vm.prank(stranger);
        vm.expectRevert();
        registry.setBandwidthRate(WORK, 960_000);
    }
```

`WORK`, `creator` and `stranger` must be whatever the existing test file already uses for
a registered work, its registrant, and an unrelated address. If the file has no
`stranger`-equivalent, add one with `makeAddr("stranger")` next to the existing fixtures.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd chain && export PATH="$HOME/.foundry/bin:$PATH" && forge test --match-test bandwidthRate`
Expected: FAIL — member `setBandwidthRate` / `bandwidthRateOf` not found.

- [ ] **Step 3: Implement the field, constants and setter**

In `chain/contracts/CWERegistry.sol`, add to the `Work` struct:

```solidity
        uint256 bandwidthRate; // bytes expected per 1e12 units of proven DAPR weight (0 = unset)
```

Add the constants, event and error near the existing ones:

```solidity
    /// @notice Lower clamp on a work's bandwidth rate, ≈ 8 kbps — an order of
    ///         magnitude below any real medium, so no legitimate work is
    ///         excluded, while still refusing a rate low enough to make a work
    ///         free to prove.
    uint256 public constant MIN_BANDWIDTH_RATE = 60_000;
    /// @notice Upper clamp, ≈ 266 Mbps — comfortably above 4K, so a mistyped
    ///         rate cannot silently demand unmeetable evidence.
    uint256 public constant MAX_BANDWIDTH_RATE = 2_000_000_000;

    /// @notice Emitted when a work's bandwidth rate is set or changed.
    event BandwidthRateSet(bytes32 indexed workId, uint256 rate);

    /// @notice A bandwidth rate outside [MIN_BANDWIDTH_RATE, MAX_BANDWIDTH_RATE].
    error BadBandwidthRate();
```

And the setter/getter:

```solidity
    /// @notice Set the bandwidth rate the settlement aggregator uses to decide
    ///         how many bytes a usage claim on this work should be backed by.
    /// @dev Registrant-only and clamped: the rate decides how much evidence a
    ///      claim must carry, so leaving it unbounded would let a creator make
    ///      their own work trivially cheap to prove. Left unset it stays 0,
    ///      which the aggregator treats as fail-closed.
    function setBandwidthRate(bytes32 workId, uint256 rate) external {
        Work storage w = _works[workId];
        // An unregistered work has a zero registrant, so this same check also
        // rejects setting a rate on a work that does not exist.
        if (msg.sender != w.registrant) revert NotRegistrant();
        if (rate < MIN_BANDWIDTH_RATE || rate > MAX_BANDWIDTH_RATE) revert BadBandwidthRate();
        w.bandwidthRate = rate;
        emit BandwidthRateSet(workId, rate);
    }

    /// @notice The work's bandwidth rate, or 0 if never set.
    function bandwidthRateOf(bytes32 workId) external view returns (uint256) {
        return _works[workId].bandwidthRate;
    }
```

`NotRegistrant` is the contract's existing error, declared at the top of the file
alongside `NotVerifiedCreator` and used by `registerWork`'s update path — reuse it rather
than adding a variant.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd chain && forge test --match-test bandwidthRate`
Expected: PASS — 7 tests.

- [ ] **Step 5: Run the whole contract suite**

Run: `cd chain && forge test`
Expected: PASS, no regressions. Do **not** run `forge fmt`.

- [ ] **Step 6: Commit**

```bash
git add chain/contracts/CWERegistry.sol chain/test/CWERegistry.t.sol
git commit -m "chain: per-work bandwidth rate with a protocol clamp"
```

---

### Task 5: Discovery hub — mirror `bandwidth_rate`

**Files:**
- Modify: `services/discovery-hub/src/manifest.rs`
- Modify: `services/discovery-hub/src/chain.rs`
- Test: the existing inline test modules in both files

**Interfaces:**
- Consumes: `CWERegistry.bandwidthRateOf` (Task 4).
- Produces: `WorkManifest.bandwidth_rate: u64`, validated to equal the on-chain value.

**Design notes:**
- This mirrors exactly what `price_per_min` already does — read how that field is
  declared, fetched in `chain.rs`'s `sol!` block, carried on the on-chain struct, and
  compared in the validation function, then follow it precisely. Do not invent a new
  pattern.
- Adding a field changes the manifest's canonical JSON, so every fixture that builds a
  `WorkManifest` in tests needs the new field. That is a fixture change, not a
  behavioural one.

- [ ] **Step 1: Add the manifest field**

In `services/discovery-hub/src/manifest.rs`, beside `price_per_min`:

```rust
    /// Bytes expected per 1e12 units of proven DAPR weight; MUST equal the
    /// on-chain value. The settlement aggregator uses it to decide how much
    /// bandwidth evidence a usage claim on this work must carry.
    pub bandwidth_rate: u64,
```

- [ ] **Step 2: Fetch and compare it on chain**

In `services/discovery-hub/src/chain.rs`:
- add `function bandwidthRateOf(bytes32 workId) external view returns (uint256);` to the
  `sol!` interface, beside `pricePerMinOf`;
- add `pub bandwidth_rate: u64,` to the on-chain work struct;
- populate it in the fetch function the same way `price_per_min` is populated, with the
  same overflow guard;
- add the equality check next to the existing price check:

```rust
    // The rate decides how much evidence a claim on this work must carry, so a
    // manifest that disagrees with the chain is rejected outright rather than
    // trusted — same rule as the price it sits beside.
    if m.bandwidth_rate != on_chain.bandwidth_rate {
        return Err(IngestError::BandwidthRateMismatch);
    }
```

Place it immediately after the existing `PriceMismatch` check so the two content-property
validations read together.

- [ ] **Step 3: Update the test fixtures and add a mismatch test**

Add `bandwidth_rate: 960_000,` to every `WorkManifest` literal in the test modules of both
files (and anywhere else the compiler flags), and to every `OnChainWork` literal in
`chain.rs`'s tests. Add an `IngestError::BandwidthRateMismatch` variant beside
`PriceMismatch`, and return it from the new check.

Then add this test, modelled directly on the existing `price_mismatch_is_rejected` (same
module, same `FakeRegistry`/`manifest` helpers):

```rust
    /// A manifest whose bandwidth rate disagrees with the chain is rejected.
    /// The rate decides how much evidence a claim on this work must carry, so
    /// a mismatch is refused rather than resolved in either direction.
    #[tokio::test]
    async fn bandwidth_rate_mismatch_is_rejected() {
        let signer = PrivateKeySigner::random();
        let m = manifest(signer.address());
        let sig = signer
            .sign_message_sync(&m.canonical_bytes().unwrap())
            .unwrap();
        let reg = FakeRegistry(Some(OnChainWork {
            registrant: signer.address(),
            price_per_min: 1_000_000,
            bandwidth_rate: 2_000_000, // differs from the manifest
            region: Bytes32([7; 32]),
        }));
        assert!(matches!(
            validate_ingest(&m, &sig.as_bytes(), &reg).await,
            Err(IngestError::BandwidthRateMismatch)
        ));
    }
```

The `region` and `price_per_min` values above must match whatever the file's `manifest()`
helper produces, so only the bandwidth rate differs — otherwise the test could pass on the
wrong mismatch.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cwe-discovery-hub`
Expected: PASS, including the new mismatch test.

- [ ] **Step 5: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-discovery-hub --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add services/discovery-hub/src
git commit -m "hub: mirror the on-chain bandwidth rate in signed manifests"
```

---

### Task 6: Settlement — epoch floor, chunk dedup, on-chain rates

**Files:**
- Modify: `services/settlement/src/receipts.rs`
- Modify: `services/settlement/src/config.rs`
- Modify: `services/settlement/src/chain.rs`
- Test: inline `#[cfg(test)] mod tests` in `receipts.rs`

**Interfaces:**
- Consumes: `cwe_receipt` (Task 1); `CWERegistry.bandwidthRateOf` (Task 4).
- Produces:
  - `pub fn epoch_factor_ppm(verified: &BTreeMap<(String, String), u128>, min_epoch_bytes: u64) -> BTreeMap<String, u64>`
  - `row_credibility_ppm(rows, verified, rates, epoch_factor)` — one new parameter
  - `Config.min_epoch_bytes: u64` (env `MIN_EPOCH_BYTES`, default `131_072`); `Config.rates` **removed**
  - a verified-bytes sidecar written next to `OUT`

**Design notes:**
- `accept_receipts` needs **no signature change**: it already dedups on
  `receipt.dedup_key()`, which Task 1 redefined. Its behaviour changes for free. Add a
  test proving the new semantics rather than editing the function.
- **`cwe-dapr` must not be touched.** The epoch factor multiplies into the per-row
  credibility settlement already computes.
- The sidecar is what lets the demo assert on *mechanisms* rather than only payouts. Cycle
  1 asserted only on payouts, which is how a wrong-reason failure could pass as a
  right-reason one.

- [ ] **Step 1: Write the failing tests**

Add to `services/settlement/src/receipts.rs`'s test module:

```rust
    /// Two receipts for the SAME chunk of the same work to the same consumer,
    /// served by DIFFERENT nodes, count once — it is the same content. Cycle 1
    /// keyed on the node and double-counted this.
    #[test]
    fn same_chunk_from_two_nodes_counts_once() {
        let node_a = PrivateKeySigner::random();
        let node_b = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node_a, &consumer, 1000, 5, 0),
                signed(&node_b, &consumer, 1000, 5, 0),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// Re-fetching the same chunk cannot amplify credit, however many receipts
    /// are produced.
    #[test]
    fn refetching_one_chunk_credits_it_once() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let receipts = (0..50).map(|_| signed(&node, &consumer, 1000, 5, 0)).collect();
        let got = accept_receipts(&ReceiptBundle { epoch: 5, receipts }, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// Distinct chunks are distinct evidence and accumulate.
    #[test]
    fn distinct_chunks_accumulate() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 1000, 5, 1),
                signed(&node, &consumer, 500, 5, 2),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(2500));
    }

    /// A user meeting the epoch floor is undiscounted; one below it is scaled
    /// proportionally; one with nothing is zero.
    #[test]
    fn epoch_factor_scales_below_the_floor() {
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xfull".to_string(), work()), 131_072);
        verified.insert(("0xhalf".to_string(), work()), 65_536);
        verified.insert(("0xdust".to_string(), work()), 1);

        let f = epoch_factor_ppm(&verified, 131_072);
        assert_eq!(f.get("0xfull").copied(), Some(1_000_000));
        assert_eq!(f.get("0xhalf").copied(), Some(500_000));
        assert_eq!(f.get("0xdust").copied(), Some(7)); // 1e6/131072, floored
    }

    /// Evidence sums ACROSS works: a user who consumed several works reaches the
    /// floor on their combined bytes, not per work.
    #[test]
    fn epoch_factor_sums_across_works() {
        let other = format!("0x{}", "bb".repeat(32));
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 65_536);
        verified.insert(("0xu".to_string(), other), 65_536);
        assert_eq!(
            epoch_factor_ppm(&verified, 131_072).get("0xu").copied(),
            Some(1_000_000)
        );
    }

    /// Over-delivering does not earn above neutral.
    #[test]
    fn epoch_factor_clamps_at_neutral() {
        let mut verified: BTreeMap<(String, String), u128> = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 10_000_000);
        assert_eq!(
            epoch_factor_ppm(&verified, 131_072).get("0xu").copied(),
            Some(1_000_000)
        );
    }

    /// The epoch factor multiplies the per-row ratio: a row that satisfies its
    /// own expectation is still discounted if the user's epoch evidence is thin.
    /// This is what closes the dust-weight gap.
    #[test]
    fn epoch_factor_discounts_a_satisfied_row() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);

        // Row ratio alone would be full credit...
        let mut full = BTreeMap::new();
        full.insert("0xu".to_string(), 1_000_000u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates, &full), vec![1_000_000]);

        // ...but with only 8192 of the 131072-byte floor, it is scaled to 6.25%.
        let factor = epoch_factor_ppm(&verified, 131_072);
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates, &factor), vec![62_500]);
    }

    /// A user absent from the factor map earns nothing — fail closed.
    #[test]
    fn missing_epoch_factor_fails_closed() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(
            row_credibility_ppm(&rows, &verified, &rates, &BTreeMap::new()),
            vec![0]
        );
    }
```

Update the existing `signed(...)` helper so its last parameter is `chunk_index` and it
builds the Task 1 receipt shape; update every existing call accordingly. Existing tests
that pass an all-neutral factor must pass `&full` (a map with `1_000_000` for the user)
so they keep asserting what they asserted before.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-settlement --lib receipts`
Expected: FAIL — `epoch_factor_ppm` not found; `row_credibility_ppm` takes 3 arguments.

- [ ] **Step 3: Implement the epoch factor**

Add to `services/settlement/src/receipts.rs`:

```rust
/// Each user's per-epoch evidence factor in ppm, from their DEDUPED verified
/// bytes summed across every work.
///
/// ```text
/// epoch_factor(U) = clamp(total_verified_bytes(U) · 1e6 / MIN_EPOCH_BYTES, 0, 1e6)
/// ```
///
/// This is the absolute floor beneath the per-row ratio, and it is what closes
/// the dust-weight gap: the ratio alone can be satisfied by a single byte when
/// the claimed weight is small, because expectation scales with weight. An
/// absolute floor does not scale, so a claimant routing a whole tier fee must
/// have received a real quantity of content this epoch whatever they claimed.
///
/// A genuinely light user is unaffected — one 30-second play moves far more than
/// the floor — which is the point: a light subscriber is legitimate, and the
/// deterrent must fall on claims backed by nothing, not on modest consumption.
pub fn epoch_factor_ppm(
    verified: &BTreeMap<(String, String), u128>,
    min_epoch_bytes: u64,
) -> BTreeMap<String, u64> {
    // Sum each user's evidence across all their works.
    let mut totals: BTreeMap<String, u128> = BTreeMap::new();
    for ((user, _work), bytes) in verified {
        let entry = totals.entry(normalize_addr(user)).or_insert(0);
        *entry = entry.saturating_add(*bytes);
    }

    // A zero floor would divide by zero; treat it as "no floor configured" and
    // leave every user neutral rather than crediting or burning arbitrarily.
    if min_epoch_bytes == 0 {
        return totals.into_keys().map(|u| (u, 1_000_000)).collect();
    }

    totals
        .into_iter()
        .map(|(user, bytes)| {
            let ppm = mul_div_floor(bytes, 1_000_000, min_epoch_bytes as u128);
            (user, std::cmp::min(ppm, 1_000_000) as u64)
        })
        .collect()
}
```

- [ ] **Step 4: Apply the factor in `row_credibility_ppm`**

Add the parameter and fold it in. Replace the function's signature and its final
computation:

```rust
pub fn row_credibility_ppm(
    rows: &[RawRow],
    verified: &BTreeMap<(String, String), u128>,
    rates: &BTreeMap<String, u64>,
    epoch_factor: &BTreeMap<String, u64>,
) -> Vec<u64> {
```

and, at the end of the per-row closure, replace `std::cmp::min(ppm, 1_000_000) as u64`
with:

```rust
            let row_ppm = std::cmp::min(ppm, 1_000_000) as u64;
            // Fold in the user's absolute per-epoch evidence factor. A user with
            // no entry has supplied no verified bytes at all, so they fail
            // closed at zero rather than defaulting to neutral.
            let factor = epoch_factor
                .get(&normalize_addr(&row.user))
                .copied()
                .unwrap_or(0);
            mul_div_floor(row_ppm as u128, factor as u128, 1_000_000) as u64
```

Also apply the factor on the two early `return` paths so they stay consistent: the
missing/zero-rate path already returns `0` (still correct), and the `row.raw == 0` neutral
path must become `factor` rather than a flat `1_000_000` — a zero-weight row still belongs
to a user whose epoch evidence may be thin. Extend that branch's doc comment to say so.

Update the function's doc comment to describe both stages: the per-row ratio, then the
per-user absolute floor.

- [ ] **Step 5: Swap `RATES` for `MIN_EPOCH_BYTES` in the config**

In `services/settlement/src/config.rs`: delete the `rates` field, its `RATES` parsing
block and the `ConfigError::Rates` variant; add:

```rust
    /// The absolute per-epoch evidence floor in bytes (from `MIN_EPOCH_BYTES`,
    /// default one whole `CHUNK_SIZE`). A user whose deduped verified bytes fall
    /// below this has every row scaled down proportionally.
    ///
    /// Aggregator-side rather than on-chain because, unlike a work's bandwidth
    /// rate, it is not settable by anyone who benefits from it.
    pub min_epoch_bytes: u64,
```

and in `from_env`:

```rust
        // The absolute evidence floor. One whole chunk by default: the smallest
        // statement that means anything is "at least one complete block of a
        // real work reached this user this epoch".
        let min_epoch_bytes: u64 = match std::env::var("MIN_EPOCH_BYTES") {
            Ok(v) => v
                .parse()
                .map_err(|_| ConfigError::Invalid("MIN_EPOCH_BYTES".into()))?,
            Err(_) => 131_072,
        };
```

Update the `from_env` doc comment's list of recognised variables.

- [ ] **Step 6: Read rates from the chain and write the sidecar**

In `services/settlement/src/chain.rs`, add `bandwidthRateOf` to the registry `sol!`
binding (or add a `Registry` binding if none exists in this file), then inside the
receipts branch, after computing `verified`:

```rust
            // Each work's expected-bytes rate now comes from the chain, not a
            // config file: it decides payouts, so it must live somewhere the
            // aggregator operator does not hand-maintain per work.
            let registry_addr = Address::from_str(&cfg.deployments.registry)?;
            let registry = Registry::new(registry_addr, provider);
            let mut rates: BTreeMap<String, u64> = BTreeMap::new();
            for row in &rows {
                let work = row.work.to_ascii_lowercase();
                if rates.contains_key(&work) {
                    continue;
                }
                let id = B256::from_str(&work)?;
                let rate = registry.bandwidthRateOf(id).call().await?;
                rates.insert(work, u64::try_from(rate).unwrap_or(0));
            }

            let factor = epoch_factor_ppm(&verified, cfg.min_epoch_bytes);
            let ppm = row_credibility_ppm(&rows, &verified, &rates, &factor);

            // Write the per-(user, work) verified bytes beside the payout output
            // so a demo or operator can assert on the MECHANISM — how much
            // evidence each claim carried — and not only on the final payout,
            // which can be zero for several different reasons.
            let sidecar: Vec<serde_json::Value> = verified
                .iter()
                .map(|((user, work), bytes)| {
                    serde_json::json!({
                        "user": user,
                        "work_id": work,
                        "verified_bytes": bytes.to_string(),
                        "epoch_factor_ppm": factor.get(user).copied().unwrap_or(0),
                    })
                })
                .collect();
            let sidecar_path = cfg.out_path.with_extension("bandwidth.json");
            std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sidecar)?)?;

            eprintln!(
                "bandwidth: {} receipts submitted, {} (user, work) pairs credited, detail → {}",
                bundle.receipts.len(),
                verified.len(),
                sidecar_path.display()
            );
            Some(ppm)
```

Add the imports these need (`B256`, `epoch_factor_ppm`). Update `run_events`'s doc comment
to describe the epoch floor and the on-chain rate lookup.

- [ ] **Step 7: Run the settlement suite**

Run: `cargo test -p cwe-settlement`
Expected: PASS. Pre-existing tests may need the new `row_credibility_ppm` argument and the
new receipt shape — those are signature/fixture updates. If a pre-existing *expected
value* changes, stop and report BLOCKED: neutral inputs must still reproduce cycle 1's
numbers.

- [ ] **Step 8: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 9: Commit**

```bash
git add services/settlement/src
git commit -m "settlement: per-epoch evidence floor, chunk dedup, on-chain bandwidth rates"
```

---

### Task 7: Six-act demo + CI

**Files:**
- Modify: `ops/demo/run_bandwidth_demo.sh`
- Modify: `.github/workflows/ci.yml` (only if the job needs new packages)

**Interfaces:**
- Consumes: everything from Tasks 1-6, plus the existing `zk_submit --mode honest` and
  the `cwe-settlement` binary (`RECEIPTS`, `MIN_EPOCH_BYTES`; `RATES` is gone).

**Design notes:**
- **Read the existing `ops/demo/run_bandwidth_demo.sh` first** and extend it. Keep its
  Anvil startup, PID discipline, deploy sequence and assertion style.
- Six users, one act each — a user may submit only once per epoch. Settle once.
- **Every work gets a bandwidth rate except Act 5's**, whose absence is the thing Act 5
  proves. Set them with `cast send $REG "setBandwidthRate(bytes32,uint256)" $WORK 60000`.
- Content: a 4 MiB file (32 chunks) per work. At rate `60_000` the expectation is
  `45 × 60_000 = 2_700_000` bytes, so a full 32-chunk download clears it.
- **Assert mechanisms as well as payouts**, reading the sidecar with `jq`. Act 2 must
  assert *both* that its verified bytes are 0 *and* that its fee burned — otherwise a
  zero payout caused by the epoch floor would masquerade as proof that delivery-gating
  works.
- **The node's ledger persists for the life of the process, keyed by
  `(consumer, work_id, chunk_index)`.** Task 3's implementer hit this while smoke-testing:
  running `honest` and then `no-download` with the *same* consumer key against the *same*
  node makes the second run find the first run's genuine ledger entries and receive real
  receipts — making a "proves nothing" result look like a broken trust property. The six
  acts are safe by construction because each uses a **distinct user** and a **distinct
  work**, and Act 6 runs against a separate node process. Preserve both properties: if you
  ever reuse a user or a work across two acts, the later act inherits the earlier one's
  evidence and its assertion becomes meaningless.

- [ ] **Step 1: Extend the demo script**

Rework `ops/demo/run_bandwidth_demo.sh` to six acts. Key differences from the current
version:

```bash
# --- content: 4 MiB per work (32 chunks of 128 KiB) --------------------------
for W in "$WORK_W" "$WORK_G" "$WORK_R" "$WORK_N"; do
  head -c 4194304 /dev/zero | tr '\0' 'x' > "$CONTENT/${W,,}.bin"
done
# Act 4's work is deliberately SMALLER than one chunk, so a claimant whose whole
# epoch is this work cannot reach the absolute evidence floor however honestly
# they download it. That is the dust-weight deterrent working — and, per spec
# §3.3, the accepted cost it imposes on very short works.
head -c 1024 /dev/zero | tr '\0' 'x' > "$CONTENT/${WORK_D,,}.bin"

# --- bandwidth rates: every work EXCEPT the unset-rate act's ----------------
# 60000 = MIN_BANDWIDTH_RATE. Expectation for the demo's proven weight
# (45e12) is 45 * 60000 = 2_700_000 bytes, comfortably under the 4 MiB served.
for W in "$WORK_W" "$WORK_G" "$WORK_R" "$WORK_D"; do
  send $DEPLOYER $REG "setBandwidthRate(bytes32,uint256)" "$W" 60000
done
# WORK_N deliberately left unset — Act 5 proves an unrated work fails closed.
```

and the settlement invocation drops `RATES` and gains the floor:

```bash
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DEPLOYMENTS=$DEP \
  RECEIPTS=$BUNDLE MIN_EPOCH_BYTES=131072 OUT=$OUT "$SETTLE" \
  || fail "settlement (event mode + receipts) failed"
SIDE="${OUT%.json}.bandwidth.json"
```

The six acts, each with its own user and work:

Five new works must be registered alongside the existing ones, each with its own payee and
consent signature — reuse the script's existing `register_work` helper:
`$WORK_W` (honest), `$WORK_G` (no-download), `$WORK_R` (refetch), `$WORK_D` (dust),
`$WORK_N` (no rate). Act 6 reuses `$WORK_W`.

| Act | User | Work | Content | Client invocation |
|---|---|---|---|---|
| 1 honest | `$U1` | `$WORK_W` | 4 MiB | `--mode honest CHUNKS=32` |
| 2 no-download | `$U2` | `$WORK_G` | 4 MiB | `--mode no-download CHUNKS=32` |
| 3 refetch | `$U3` | `$WORK_R` | 4 MiB | `--mode refetch REPEATS=100` |
| 4 dust weight | `$U4` | `$WORK_D` | **1 KiB** | `--mode honest CHUNKS=1` |
| 5 unset rate | `$U5` | `$WORK_N` | 4 MiB | `--mode honest CHUNKS=32` |
| 6 rogue node | `$U6` | `$WORK_W` | 4 MiB | `--mode honest CHUNKS=32`, port 8547 |

**Act 4 needs a deliberately tiny content file (1 KiB, not 4 MiB).** With a full-size
file, an honest single-chunk download delivers exactly 131_072 bytes — precisely the epoch
floor — so the floor would *not* bite and the act would prove nothing about the mechanism
it exists to demonstrate. A 1 KiB work makes the claimant's entire epoch evidence 1024
bytes, giving an epoch factor of `1024 · 1e6 / 131_072 ≈ 7812` ppm (0.78%), so the fee is
almost entirely burned even though the row's own ratio is satisfied for what little was
claimed.

Note honestly in the script's comments that Act 4 is simultaneously a demonstration of the
accepted consequence recorded in spec §3.3 — a work smaller than one chunk cannot alone
satisfy an absolute floor. The act proves the deterrent works; it also shows exactly whom
it costs.

- [ ] **Step 2: Add the assertions**

```bash
step "Assertions"
credit_of() { jq -r --arg w "$1" '[.entries[] | select(.work_id == $w) | .amount][0] // "0"' "$OUT"; }
bytes_of()  { jq -r --arg u "$1" --arg w "$2" \
  '[.[] | select(.user == $u and .work_id == $w) | .verified_bytes][0] // "0"' "$SIDE"; }

# Act 1 — honest: full fee, and real evidence behind it.
[ "$(credit_of "${WORK_W,,}")" = "$FEE" ] || fail "honest work was not paid in full"
[ "$(bytes_of "$(cast wallet address $U1)" "${WORK_W,,}")" -ge 4000000 ] \
  || fail "honest act did not register its downloaded bytes"

# Act 2 — never downloaded: ZERO evidence (the mechanism), and a burned fee.
B2=$(bytes_of "$(cast wallet address $U2)" "${WORK_G,,}")
[ "$B2" = "0" ] || fail "undownloaded chunks were credited $B2 bytes, expected 0"
[ "$(credit_of "${WORK_G,,}")" = "0" ] || fail "work with no downloads earned credit"

# Act 3 — refetch: 100 receipts for one chunk collapse to a single chunk's bytes.
B3=$(bytes_of "$(cast wallet address $U3)" "${WORK_R,,}")
[ "$B3" -le 131072 ] || fail "re-fetching amplified evidence to $B3 bytes"
[ "$(credit_of "${WORK_R,,}")" = "0" ] || fail "re-fetch work earned credit"

# Act 4 — dust: real but tiny evidence, so the absolute floor guts the payout.
B4=$(bytes_of "$(cast wallet address $U4)" "${WORK_D,,}")
[ "$B4" = "1024" ] || fail "dust act moved $B4 bytes, expected its whole 1 KiB work"
C4=$(credit_of "${WORK_D,,}")
[ "$C4" -lt $((FEE / 1000)) ] \
  || fail "dust claim earned $C4, expected the epoch floor to burn nearly all of $FEE"

# Act 5 — unset rate: fails closed regardless of genuine evidence.
B5=$(bytes_of "$(cast wallet address $U5)" "${WORK_N,,}")
[ "$B5" -ge 4000000 ] || fail "unset-rate act should still have moved real bytes"
[ "$(credit_of "${WORK_N,,}")" = "0" ] || fail "work with no bandwidth rate earned credit"

# Act 6 — uncredentialed node: receipts rejected outright, so no evidence at all.
[ "$(bytes_of "$(cast wallet address $U6)" "${WORK_W,,}")" = "0" ] \
  || fail "rogue node's receipts were counted"
```

Act 3's assertion `B3 -le 131072` is the load-bearing one: without chunk dedup it would be
100 × 131072.

- [ ] **Step 3: Run the demo**

Run: `make -C ops bandwidth-demo`
Expected: `✅ BANDWIDTH DEMO PASSED`.

If an act fails, read the `bandwidth:` line and the sidecar before changing anything.
**Do not weaken an assertion to make the demo pass** — the assertions encode the cycle's
claim. If one cannot be met, report BLOCKED with the evidence.

- [ ] **Step 4: Commit**

```bash
git add ops/demo/run_bandwidth_demo.sh .github/workflows/ci.yml
git commit -m "ops: six-act bandwidth demo covering delivery-gating, dedup and the epoch floor"
```

---

### Task 8: Documentation and status sync

**Files:**
- Modify: `docs/roadmap.md`, `ROADMAP.md`, `project-map.js`
- Modify: `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md` (cycle 1's
  known-limitations entries are now closed)

**Design notes:** `CLAUDE.md` requires the three status documents to flip together, and
demands the map track reality rather than aspiration. Today's date is **2026-07-26**.

- [ ] **Step 1: Update the three status documents**

- Mark **H5 cycle 2** done in all three, describing what shipped: content-position
  receipts, delivery-gated crediting, the per-user epoch evidence floor, and the on-chain
  bandwidth rate.
- **Remove** the dust-weight and fetch-and-discard entries from the known-limitations
  lists — they are closed. Do not leave them reading as open.
- Add the honest residual in their place: crediting establishes that bytes left the server
  and were accepted by the client's transport, not that the client application consumed
  them — permanently out of scope, not deferred (spec §3.2).
- Add the accepted consequence from spec §3.3: a user whose entire epoch is a single work
  smaller than one chunk (~8 seconds of audio) is partially discounted.
- Update `project.updated` to `2026-07-26`; add a `h5c2` roadmap entry as done and update
  the `cwe-receipt`, `cwe-storage`, settlement and registry nodes.
- Do **not** touch `project-map.html`.

- [ ] **Step 2: Annotate cycle 1's spec**

In `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md` §1.2, mark the
dust-weight and unmetered-fragment entries as **closed in cycle 2**, with a pointer to
`docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md`. Leave the
text itself intact — it is the record of what was known when.

- [ ] **Step 3: Verify the map still renders**

Run: `node --check project-map.js`
Expected: valid. Then open `file:///home/roland/git/clean-web-economy/project-map.html`
and confirm the new entries appear.

- [ ] **Step 4: Run the full gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd chain && export PATH="$HOME/.foundry/bin:$PATH" && forge test)
```

Expected: all green. Do not proceed to merge on a red result.

- [ ] **Step 5: Commit**

```bash
git add ROADMAP.md docs/roadmap.md project-map.js docs/superpowers/specs
git commit -m "Roadmap + project map: H5 cycle 2 (credibility integrity) complete"
```

---

## Verification checklist (before merging)

- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green
- [ ] `(cd chain && forge test)` green
- [ ] All nine demos pass, including the six-act `bandwidth-demo`
- [ ] `sims/` (`cwe-dapr`) is untouched by the whole branch — verify with
      `git diff main --stat -- sims/` returning nothing
- [ ] No pre-existing test's expected value was edited
- [ ] **Mutation check (required, per spec §7):** the final review must confirm the demo
      *fails* when each of the three fixes is individually reverted — delivery-gating,
      chunk dedup, and the epoch floor. A demo that passes regardless proves nothing.
- [ ] No mention of any AI agent, assistant, or vendor anywhere in the diff
