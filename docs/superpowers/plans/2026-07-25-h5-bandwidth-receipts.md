# H5 — Storage layer + real bandwidth receipts (cycle 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make H3's bandwidth-credibility knob live — a real HTTP storage node serves
bytes, consumer and node co-sign receipts, and the aggregator verifies them (both
signatures + a CWEIdentity storage-node credential + anti-replay) to compute a real
per-(user, work) credibility that feeds DAPR, so "claimed but no bytes moved" becomes a
live strict loss.

**Architecture:** A new portable `cwe-receipt` crate defines the receipt tuple, its
EIP-191 signing/verification and its anti-replay dedup key. A new `cwe-storage` HTTP node
serves fragments from a content directory, records what it actually served, and will only
sign receipts for bytes it has a record of serving. A companion consumer binary downloads
fragments, co-signs the node's receipts, and writes a per-epoch receipt bundle. `cwe-dapr`
grows a per-row-credibility core (the existing per-work API becomes a thin wrapper), and
settlement's event mode verifies the bundle on-chain against `CWEIdentity` and turns
verified bytes into per-row credibility. Neutral credibility reproduces today's payouts
bit-for-bit, so every existing demo is untouched.

**Tech Stack:** Rust (workspace crates), `alloy` v2 (signing, recovery, contract calls),
`axum` 0.8 + `tokio` (the storage node's HTTP server), `reqwest` (the consumer client),
`serde_jcs` (RFC 8785 canonical JSON as the signed bytes), Solidity/Foundry
(`CredentialTypes`), bash + Anvil (the demo).

**Governing spec:** `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md`.
Read it before Task 1; the decision table (D1–D9) and §4.1 are binding.

## Global Constraints

- **No AI attribution anywhere** — not in code, comments, docs, commit messages, branch
  names, or anything pushed to GitHub. Write everything as ordinary human-authored work.
  This is a hard project rule (`CLAUDE.md`).
- **Rust everywhere** except the Solidity under `chain/`. New crates join the root Cargo
  workspace's `members` list.
- **Every function/method gets a `///` doc comment** describing in detail what it does.
  Non-trivial lines get an inline comment — only where it adds understanding, never noise
  that restates the code.
- **Deterministic integer math only** in payout/credibility paths. No floating point.
  `u128` intermediates, explicit saturating/checked arithmetic, ppm in `[0, 1_000_000]`.
- **The full gate must stay green:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and
  `(cd chain && forge test)`. Do **not** run `forge fmt`.
- **Neutral credibility (`1_000_000`) must reproduce current payouts bit-for-bit.** Every
  pre-existing `cwe-dapr` and settlement test must pass unmodified. If you find yourself
  editing an existing expected value, you have broken this constraint — stop and fix the
  code instead.
- **Never kill a process you did not start.** Demo scripts capture the exact PID of what
  they launch (`cmd & PID=$!`) and kill only that PID. No `pkill`/`killall`/pattern kills.
- Foundry lives at `$HOME/.foundry/bin`; demo scripts prepend it to `PATH`.
- `cargo test --workspace` is slow (~3–4 min) because the zk-circuits setup tests run a
  real trusted setup. Use `--release` when iterating, and prefer `-p <crate>` while
  working a single task.

## Key constants and units (read before Task 2)

The proven per-row weight settlement receives is
`weight = minutes · price_ppm · region_ppm · D(plays) / 1e6` (`cwe_zk_circuits::dr::weight_of`).
For the demo's sample row (`minutes = 60`, `price_ppm = 1e6`, `region_ppm = 1e6`,
`plays = 2`, `d_ppm(2, 1e6) = 750_000`) this is exactly
`60 · 1e6 · 1e6 · 750_000 / 1e6 = 45_000_000_000_000` (4.5 × 10¹³).

Because weights are this large, an integer "bytes per unit weight" would always floor to
zero. `RATE(W)` is therefore expressed as **bytes per 10¹² units of weight**:

```text
RATE_SCALE   = 1_000_000_000_000                       // 10^12
expected_bytes(U,W) = weight(U,W) · rate(W) / RATE_SCALE
```

10¹² of weight is exactly one minute of a full-price (`price_ppm = 1e6`),
neutral-region, first-play work — so `rate(W)` reads naturally as **bytes per minute of
full-price content** (128 kbps audio ≈ 960_000).

## File Structure

| Path | Responsibility |
|---|---|
| `libs/receipt/Cargo.toml`, `libs/receipt/src/lib.rs` (new crate `cwe-receipt`) | The `Receipt` tuple, its canonical bytes, EIP-191 sign/recover, `SignedReceipt::verify`, `ReceiptBundle`, and the anti-replay dedup key. Portable: no chain calls, no I/O. Shared by node, consumer and settlement. |
| `sims/src/lib.rs` | `allocate_from_raw_with_row_credibility` (the new per-row core) + `allocate_from_raw` reduced to a wrapper. |
| `chain/contracts/CredentialTypes.sol` | Adds the `STORAGE_NODE` credential-type tag. |
| `chain/test/CWEIdentity.t.sol` | Covers attest/isValid/revoke for the new tag. |
| `services/storage/Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/bin/bandwidth_client.rs` (new crate `cwe-storage`) | `lib.rs`: served-chunk ledger + receipt issuance logic (unit-testable, no network). `main.rs`: the axum HTTP node. `bandwidth_client.rs`: the consumer that downloads, co-signs, and writes a bundle. |
| `services/settlement/src/config.rs` | `Deployments.identity`; `Config.receipts_path` (`RECEIPTS`) and `Config.rates` (`RATES`). |
| `services/settlement/src/receipts.rs` (new) | Bundle verification against `CWEIdentity` + anti-replay, and the per-row credibility computation (§4/§4.1 of the spec). |
| `services/settlement/src/chain.rs` | Event mode wires verified bytes → per-row credibility → the extended DAPR call. |
| `ops/demo/run_bandwidth_demo.sh`, `ops/Makefile` | `make bandwidth-demo`. |
| `.github/workflows/ci.yml` | The `bandwidth-e2e` job. |
| `ROADMAP.md`, `docs/roadmap.md`, `project-map.js` | Status sync at merge. |

**One deliberate choice against the spec's component table:** spec §2.1 lists the consumer
receipt path as `clients/player-plugin` *(or a demo client)*. This plan takes the demo
client — `bandwidth-client`, a second binary in `cwe-storage`. The player still settles on
the legacy **disclosure** path, so wiring receipts into it would mean dragging it onto the
event-mode path first; that migration is already listed as deferred H2 cycle-2 work.
Keeping the client separate leaves `make player-demo` untouched.

---

### Task 1: `cwe-receipt` — the receipt type, signing, and anti-replay

**Files:**
- Create: `libs/receipt/Cargo.toml`
- Create: `libs/receipt/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members`)
- Test: inline `#[cfg(test)] mod tests` in `libs/receipt/src/lib.rs`

**Interfaces:**
- Consumes: nothing (this is the base crate).
- Produces — every later task depends on these exact names:
  - `pub struct Receipt { work_id: String, consumer: String, node: String, bytes: u64, epoch: u64, session_nonce: String, chunk_nonce: u64 }`
  - `Receipt::canonical_bytes(&self) -> Result<Vec<u8>, ReceiptError>`
  - `Receipt::recover_signer(&self, sig_hex: &str) -> Result<Address, ReceiptError>`
  - `Receipt::dedup_key(&self) -> (String, String, u64)`
  - `pub struct SignedReceipt { receipt: Receipt, node_sig: String, consumer_sig: String }`
  - `SignedReceipt::verify(&self) -> Result<(), ReceiptError>`
  - `pub struct ReceiptBundle { epoch: u64, receipts: Vec<SignedReceipt> }`
  - `ReceiptBundle::from_json(&str) -> Result<ReceiptBundle, ReceiptError>`, `to_json(&self) -> Result<String, ReceiptError>`
  - `pub fn normalize_addr(s: &str) -> String`
  - `pub enum ReceiptError` with variants `Canonical(String)`, `BadSignature`, `Recover(String)`, `SignerMismatch { field: &'static str }`, `Json(String)`

**Design notes for the implementer:**
- Addresses and hex ids are stored as **lowercase `0x`-prefixed strings**, matching
  `cwe_dapr::RawRow`'s `user`/`work` keys (settlement builds them with `format!("{:#x}", addr)`
  and `Bytes32(..).to_string()`). This is what lets receipts join to DAPR rows without
  any conversion. `normalize_addr` lowercases and is applied on construction and in `verify`.
- The signed bytes are RFC 8785 canonical JSON of the `Receipt` (`serde_jcs::to_vec`),
  exactly the pattern `WorkManifest::canonical_bytes` already uses. Both parties sign the
  *same* bytes; `recover_address_from_msg` applies the EIP-191 personal-sign prefix.
- Signatures are `0x`-prefixed hex strings of the 65-byte `r||s||v` form, so a bundle is
  plain JSON.

- [ ] **Step 1: Create the crate manifest and register it in the workspace**

`libs/receipt/Cargo.toml`:

```toml
# H5 — the portable bandwidth-receipt type shared by the storage node, the
# consumer client, and the settlement aggregator.
[package]
name = "cwe-receipt"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Co-signed bandwidth receipts for the Clean Web Economy"

[dependencies]
alloy = { version = "2", features = ["full"] }
serde.workspace = true
serde_json.workspace = true
serde_jcs.workspace = true
thiserror.workspace = true

[dev-dependencies]
alloy = { version = "2", features = ["full"] }
```

In the root `Cargo.toml`, add to `members` immediately after `"libs/wallet-zk",`:

```toml
    "libs/receipt",       # H5 — co-signed bandwidth receipts
```

- [ ] **Step 2: Write the failing tests**

Create `libs/receipt/src/lib.rs` containing ONLY the test module below plus
`use` lines; it will not compile until Step 4, which is the point.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    /// Build a receipt for `consumer`/`node`, with everything else fixed.
    fn sample(consumer: &str, node: &str) -> Receipt {
        Receipt {
            work_id: format!("0x{}", "aa".repeat(32)),
            consumer: consumer.to_string(),
            node: node.to_string(),
            bytes: 524_288,
            epoch: 7,
            session_nonce: format!("0x{}", "5c".repeat(32)),
            chunk_nonce: 3,
        }
    }

    /// Co-sign `receipt` with both keys, producing a well-formed `SignedReceipt`.
    fn co_sign(receipt: Receipt, node: &PrivateKeySigner, consumer: &PrivateKeySigner) -> SignedReceipt {
        let msg = receipt.canonical_bytes().unwrap();
        SignedReceipt {
            node_sig: format!("0x{}", hex_of(&node.sign_message_sync(&msg).unwrap().as_bytes())),
            consumer_sig: format!("0x{}", hex_of(&consumer.sign_message_sync(&msg).unwrap().as_bytes())),
            receipt,
        }
    }

    /// Lowercase hex of a byte slice (test-local; the crate stores hex strings).
    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Canonical bytes are stable across calls (RFC 8785 sorts keys).
    #[test]
    fn canonical_bytes_are_stable() {
        let r = sample("0x00", "0x11");
        assert_eq!(r.canonical_bytes().unwrap(), r.canonical_bytes().unwrap());
    }

    /// A correctly co-signed receipt verifies.
    #[test]
    fn co_signed_receipt_verifies() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        co_sign(r, &node, &consumer).verify().unwrap();
    }

    /// Tampering with `bytes` after signing invalidates both signatures.
    #[test]
    fn tampered_bytes_fail_verification() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        signed.receipt.bytes += 1; // inflate after the fact
        assert!(signed.verify().is_err());
    }

    /// A receipt signed by the wrong node key does not verify against the
    /// `node` address it names.
    #[test]
    fn wrong_node_signature_fails() {
        let node = PrivateKeySigner::random();
        let impostor = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()), // names `node`...
        );
        let signed = co_sign(r, &impostor, &consumer); // ...but `impostor` signed
        assert!(signed.verify().is_err());
    }

    /// A missing (empty) consumer signature is rejected, not treated as absent.
    #[test]
    fn missing_consumer_signature_fails() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        signed.consumer_sig = String::new();
        assert!(signed.verify().is_err());
    }

    /// The dedup key is (node, session_nonce, chunk_nonce): two receipts from
    /// the same node/session/chunk collide, a different chunk does not.
    #[test]
    fn dedup_key_identifies_a_replay() {
        let a = sample("0xaa", "0xbb");
        let mut b = a.clone();
        b.bytes = 1; // a replay may differ in payload; the key must still collide
        assert_eq!(a.dedup_key(), b.dedup_key());

        let mut c = a.clone();
        c.chunk_nonce += 1;
        assert_ne!(a.dedup_key(), c.dedup_key());
    }

    /// Addresses are normalised to lowercase so receipts join to DAPR row keys.
    #[test]
    fn addresses_normalise_to_lowercase() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()).to_uppercase(),
            &format!("{:#x}", node.address()).to_uppercase(),
        );
        let signed = co_sign(r, &node, &consumer);
        // Verification is case-insensitive on the declared addresses...
        signed.verify().unwrap();
        // ...and `normalize_addr` is what makes it so.
        assert_eq!(normalize_addr("0xAbCd"), "0xabcd");
    }

    /// A bundle round-trips through JSON unchanged.
    #[test]
    fn bundle_round_trips_through_json() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let bundle = ReceiptBundle { epoch: 7, receipts: vec![co_sign(r, &node, &consumer)] };
        let json = bundle.to_json().unwrap();
        let back = ReceiptBundle::from_json(&json).unwrap();
        assert_eq!(back.epoch, 7);
        assert_eq!(back.receipts.len(), 1);
        back.receipts[0].verify().unwrap();
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p cwe-receipt`
Expected: FAIL — compile errors, `cannot find type Receipt in this scope` and friends.

- [ ] **Step 4: Implement the crate**

Prepend to `libs/receipt/src/lib.rs` (above the test module):

```rust
//! Co-signed bandwidth receipts (H5 cycle 1).
//!
//! A receipt is the smallest unit of evidence that content bytes actually moved:
//! a storage node states how many bytes of a work it served to a consumer in a
//! given epoch, and BOTH parties sign that statement. Neither side can fabricate
//! one alone — the node will not sign bytes it did not serve, and the aggregator
//! will not count a receipt the consumer did not counter-sign.
//!
//! This crate is deliberately portable: it defines the tuple, its canonical
//! signable bytes, signature recovery, and the anti-replay dedup key, but does no
//! I/O and makes no chain calls. Credential checks and replay rejection are the
//! aggregator's job (see `services/settlement/src/receipts.rs`).

use alloy::primitives::Address;
use alloy::primitives::Signature;
use serde::{Deserialize, Serialize};

/// One co-signable statement that `bytes` of `work_id` moved from `node` to
/// `consumer` during `epoch`.
///
/// Addresses and 32-byte ids are lowercase `0x`-prefixed hex strings, matching
/// the key form `cwe_dapr::RawRow` uses for `user` and `work` — that is what lets
/// verified receipts join directly to the DAPR rows they credit.
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
    /// Bytes served, as agreed by both parties.
    pub bytes: u64,
    /// The settlement epoch this receipt belongs to.
    pub epoch: u64,
    /// Per consumer-node session nonce, as a lowercase `0x` 32-byte hex string.
    pub session_nonce: String,
    /// Per-chunk counter within a session; with `session_nonce` and `node` it
    /// forms the anti-replay key.
    pub chunk_nonce: u64,
}

impl Receipt {
    /// The RFC 8785 canonical JSON bytes of this receipt — the exact bytes both
    /// parties sign. Both signers and the verifier call this one function, so the
    /// encodings cannot drift.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReceiptError> {
        serde_jcs::to_vec(self).map_err(|e| ReceiptError::Canonical(e.to_string()))
    }

    /// Recover the address that produced `sig_hex` (a `0x`-prefixed 65-byte
    /// `r||s||v` EIP-191 signature) over this receipt's canonical bytes.
    pub fn recover_signer(&self, sig_hex: &str) -> Result<Address, ReceiptError> {
        // Strip the optional 0x prefix and decode; anything malformed is a bad
        // signature rather than a panic.
        let raw = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);
        let bytes = decode_hex(raw).ok_or(ReceiptError::BadSignature)?;
        let sig = Signature::try_from(bytes.as_slice()).map_err(|_| ReceiptError::BadSignature)?;
        let msg = self.canonical_bytes()?;
        // `recover_address_from_msg` applies the EIP-191 personal-sign prefix, so
        // it matches a signer that used `sign_message`.
        sig.recover_address_from_msg(&msg)
            .map_err(|e| ReceiptError::Recover(e.to_string()))
    }

    /// The anti-replay key `(node, session_nonce, chunk_nonce)`.
    ///
    /// The aggregator drops any receipt whose key it has already counted this
    /// epoch, which kills both duplicate submission and replay of an old
    /// receipt. The key deliberately excludes `bytes` so that re-submitting the
    /// same chunk with an inflated byte count still collides.
    pub fn dedup_key(&self) -> (String, String, u64) {
        (
            normalize_addr(&self.node),
            self.session_nonce.to_ascii_lowercase(),
            self.chunk_nonce,
        )
    }
}

/// A receipt plus both parties' EIP-191 signatures over its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedReceipt {
    /// The statement being attested.
    pub receipt: Receipt,
    /// The storage node's signature, `0x`-prefixed hex.
    pub node_sig: String,
    /// The consumer's signature, `0x`-prefixed hex.
    pub consumer_sig: String,
}

impl SignedReceipt {
    /// Check that both signatures recover to the addresses the receipt names.
    ///
    /// This is the portable half of verification: it proves the receipt was
    /// co-signed by exactly the two parties it claims and has not been altered
    /// since. It says nothing about whether the node is *credentialed*, whether
    /// the epoch is right, or whether this receipt was already counted — those
    /// are the aggregator's checks.
    pub fn verify(&self) -> Result<(), ReceiptError> {
        let node = self.receipt.recover_signer(&self.node_sig)?;
        if format!("{node:#x}") != normalize_addr(&self.receipt.node) {
            return Err(ReceiptError::SignerMismatch { field: "node" });
        }
        let consumer = self.receipt.recover_signer(&self.consumer_sig)?;
        if format!("{consumer:#x}") != normalize_addr(&self.receipt.consumer) {
            return Err(ReceiptError::SignerMismatch { field: "consumer" });
        }
        Ok(())
    }
}

/// A consumer's whole per-epoch set of co-signed receipts, as submitted to the
/// aggregator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptBundle {
    /// The epoch every receipt in this bundle must be bound to.
    pub epoch: u64,
    /// The co-signed receipts.
    pub receipts: Vec<SignedReceipt>,
}

impl ReceiptBundle {
    /// Parse a bundle from its JSON representation.
    pub fn from_json(s: &str) -> Result<ReceiptBundle, ReceiptError> {
        serde_json::from_str(s).map_err(|e| ReceiptError::Json(e.to_string()))
    }

    /// Serialise this bundle to pretty JSON (bundles are written to disk and
    /// read by a human as often as by the aggregator).
    pub fn to_json(&self) -> Result<String, ReceiptError> {
        serde_json::to_string_pretty(self).map_err(|e| ReceiptError::Json(e.to_string()))
    }
}

/// Lowercase an address/hex string so string comparisons against the canonical
/// `{:#x}` form (and against DAPR's row keys) are case-insensitive.
pub fn normalize_addr(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Decode a lowercase-or-uppercase hex string into bytes, returning `None` on any
/// malformed input (odd length or a non-hex digit).
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Errors from building or verifying receipts.
#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    /// The receipt could not be canonicalised to RFC 8785 JSON.
    #[error("canonicalising receipt: {0}")]
    Canonical(String),
    /// A signature was missing, malformed, or the wrong length.
    #[error("malformed signature")]
    BadSignature,
    /// Public-key recovery failed for a syntactically valid signature.
    #[error("recovering signer: {0}")]
    Recover(String),
    /// A signature recovered to an address other than the one the receipt names.
    #[error("{field} signature does not match the address the receipt names")]
    SignerMismatch {
        /// Which party mismatched: `"node"` or `"consumer"`.
        field: &'static str,
    },
    /// The bundle JSON could not be parsed or serialised.
    #[error("receipt bundle JSON: {0}")]
    Json(String),
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p cwe-receipt`
Expected: PASS — 7 tests.

- [ ] **Step 6: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-receipt --all-targets -- -D warnings`
Expected: both clean. Fix anything reported before committing.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock libs/receipt
git commit -m "receipt: co-signed bandwidth receipt type, EIP-191 signing, anti-replay key"
```

---

### Task 2: `cwe-dapr` — per-row bandwidth credibility

**Files:**
- Modify: `sims/src/lib.rs` (`allocate_from_raw` at ~line 301; `DaprError`)
- Test: inline `#[cfg(test)] mod tests` in `sims/src/lib.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `pub fn allocate_from_raw_with_row_credibility(tier_fees: &BTreeMap<UserId, u128>, rows: &[RawRow], credibility_ppm: &[u64]) -> Result<Payouts, DaprError>`
  - `allocate_from_raw` keeps its existing signature and becomes a wrapper.
  - New error variant `DaprError::RowCredibilityLen`.

**Design notes for the implementer:**
- The strict-loss property is the whole point: the denominator stays the
  **bandwidth-free** `rw_u = Σ raw`, while the numerator uses `cred`. The shortfall
  `fee - target` goes to `unallocated` (burned), never redistributed. Do not "fix" this.
- Clamp each credibility to `1_000_000` on the way in, exactly as `bw_ppm` does, so a
  stray above-neutral value degrades to neutral instead of breaking conservation.
- The existing function body already does everything; you are threading a per-row value
  through instead of a per-work lookup. Keep row grouping and ordering identical —
  largest-remainder ties are broken by position, so any reordering changes payouts.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `sims/src/lib.rs`:

```rust
    /// Three rows across two users, used by the per-row credibility tests.
    fn cred_rows() -> (BTreeMap<UserId, u128>, Vec<RawRow>) {
        let mut fees: BTreeMap<UserId, u128> = BTreeMap::new();
        fees.insert("u1".to_string(), 1_000_000);
        fees.insert("u2".to_string(), 1_000_000);
        let rows = vec![
            RawRow { user: "u1".into(), work: "wA".into(), raw: 300 },
            RawRow { user: "u1".into(), work: "wB".into(), raw: 100 },
            RawRow { user: "u2".into(), work: "wA".into(), raw: 400 },
        ];
        (fees, rows)
    }

    /// All-neutral per-row credibility reproduces `allocate_from_raw` with an
    /// empty (neutral) bandwidth map, bit for bit. This is the compatibility
    /// guarantee every existing caller and demo relies on.
    #[test]
    fn neutral_row_credibility_matches_allocate_from_raw() {
        let (fees, rows) = cred_rows();
        let baseline = allocate_from_raw(&fees, &rows, &BTreeMap::new()).unwrap();
        let neutral = vec![1_000_000u64; rows.len()];
        let got = allocate_from_raw_with_row_credibility(&fees, &rows, &neutral).unwrap();
        assert_eq!(got, baseline);
    }

    /// A zero-credibility row is a STRICT LOSS: its share is burned into
    /// `unallocated`, not handed to the user's other rows. u1's wA row is
    /// discredited, so u1 pays only the 100/400 its wB row still earns.
    #[test]
    fn zero_credibility_row_burns_its_share() {
        let (fees, rows) = cred_rows();
        let creds = vec![0, 1_000_000, 1_000_000];
        let got = allocate_from_raw_with_row_credibility(&fees, &rows, &creds).unwrap();

        // u1: rw = 400, sum_cred = 100 → target = 1_000_000 · 100/400 = 250_000,
        // all of it to wB; the other 750_000 is burned.
        // u2: fully credible → its whole 1_000_000 fee goes to wA.
        assert_eq!(got.per_work.get("wB").copied(), Some(250_000));
        assert_eq!(got.per_work.get("wA").copied(), Some(1_000_000));
        assert_eq!(got.unallocated, 750_000);

        // Conservation: everything paid plus everything burned equals the fees in.
        let paid: u128 = got.per_work.values().sum();
        assert_eq!(paid + got.unallocated, 2_000_000);
    }

    /// Per-row credibility is per-(user, work): discrediting u1's wA row does
    /// not touch u2's row on the SAME work. This is the property that makes
    /// per-user padding punishable without collateral damage.
    #[test]
    fn row_credibility_does_not_leak_across_users() {
        let (fees, rows) = cred_rows();
        let creds = vec![0, 1_000_000, 1_000_000];
        let got = allocate_from_raw_with_row_credibility(&fees, &rows, &creds).unwrap();
        // u2's contribution to wA is untouched by u1's discredited row.
        assert_eq!(got.per_work.get("wA").copied(), Some(1_000_000));
    }

    /// A fractional credibility discounts proportionally: at 50% on u1's wA row,
    /// u1's target is (150+100)/400 of its fee.
    #[test]
    fn fractional_credibility_discounts_proportionally() {
        let (fees, rows) = cred_rows();
        let creds = vec![500_000, 1_000_000, 1_000_000];
        let got = allocate_from_raw_with_row_credibility(&fees, &rows, &creds).unwrap();
        // u1: cred = 150 + 100 = 250, rw = 400 → target = 625_000, burned 375_000.
        assert_eq!(got.unallocated, 375_000);
        let paid: u128 = got.per_work.values().sum();
        assert_eq!(paid + got.unallocated, 2_000_000);
    }

    /// An above-neutral credibility degrades to neutral rather than paying out
    /// more than the fee (conservation must hold on bogus input).
    #[test]
    fn above_neutral_row_credibility_degrades_to_neutral() {
        let (fees, rows) = cred_rows();
        let baseline = allocate_from_raw(&fees, &rows, &BTreeMap::new()).unwrap();
        let bogus = vec![5_000_000u64; rows.len()];
        let got = allocate_from_raw_with_row_credibility(&fees, &rows, &bogus).unwrap();
        assert_eq!(got, baseline);
    }

    /// A credibility slice whose length does not match `rows` is a caller bug
    /// and must be refused, not silently padded.
    #[test]
    fn mismatched_credibility_length_is_an_error() {
        let (fees, rows) = cred_rows();
        let short = vec![1_000_000u64; rows.len() - 1];
        assert!(matches!(
            allocate_from_raw_with_row_credibility(&fees, &rows, &short),
            Err(DaprError::RowCredibilityLen)
        ));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-dapr --lib`
Expected: FAIL — `cannot find function allocate_from_raw_with_row_credibility`.

- [ ] **Step 3: Implement the per-row core and reduce the old function to a wrapper**

In `sims/src/lib.rs`, replace the whole `pub fn allocate_from_raw(...) { ... }` body
(lines ~301–391) with the wrapper plus the new core below. Keep the existing doc comment
on `allocate_from_raw` and append the sentence noted below.

```rust
/// Compute per-work payouts (and the reputation signal) from pre-computed
/// per-row `raw` weights and a PER-WORK bandwidth-credibility map.
///
/// [... keep the existing doc comment verbatim, then append: ...]
///
/// This is now a thin wrapper over
/// [`allocate_from_raw_with_row_credibility`]: it expands the per-work map into
/// one credibility per row. Callers with a real per-(user, work) bandwidth
/// signal — settlement, once receipts are verified — should call the per-row
/// form directly, since a per-work value cannot distinguish an honest user from
/// a padder on the same work.
pub fn allocate_from_raw(
    tier_fees: &BTreeMap<UserId, u128>,
    rows: &[RawRow],
    bandwidth_ppm: &BTreeMap<WorkId, u64>,
) -> Result<Payouts, DaprError> {
    // Expand the per-work signal to one value per row; `bw_ppm` supplies the
    // neutral default and the upper clamp.
    let per_row: Vec<u64> = rows.iter().map(|r| bw_ppm(bandwidth_ppm, &r.work)).collect();
    allocate_from_raw_with_row_credibility(tier_fees, rows, &per_row)
}

/// Compute per-work payouts (and the reputation signal) from pre-computed
/// per-row `raw` weights and a PER-ROW bandwidth-credibility signal.
///
/// This is the payout core. `credibility_ppm[i]` is row `i`'s bandwidth
/// credibility in ppm — `1_000_000` is neutral (full credit), `0` means the
/// bandwidth layer saw no evidence that this user actually received this work's
/// content. Values above neutral are clamped down to neutral, so bandwidth can
/// only ever discount a payout; that keeps `cred ≤ raw` and with it fee
/// conservation, even on malformed input.
///
/// For each row `cred_i = raw_i · credibility_ppm[i] / 1e6`. Per user,
/// `rw_u = Σ raw` (the **bandwidth-free** denominator) and `sum_cred = Σ cred`
/// drive `target = fee · sum_cred / rw_u`, the largest-remainder [`apportion`],
/// and the `unallocated` accounting. Because the denominator excludes the
/// discount, the shortfall `fee - target` is **burned into `unallocated`, not
/// redistributed** — that is the anti-fraud strict-loss property: claiming usage
/// whose bytes never moved destroys the claimant's own money instead of moving
/// it somewhere else.
///
/// Row order within a user must match the caller's intended apportionment
/// tie-breaks, since largest-remainder ties are broken by position.
///
/// Returns [`DaprError::RowCredibilityLen`] if `credibility_ppm.len() != rows.len()`.
pub fn allocate_from_raw_with_row_credibility(
    tier_fees: &BTreeMap<UserId, u128>,
    rows: &[RawRow],
    credibility_ppm: &[u64],
) -> Result<Payouts, DaprError> {
    // The two slices are positionally paired, so a length mismatch is a caller
    // bug that would silently mis-attribute credibility. Refuse it.
    if credibility_ppm.len() != rows.len() {
        return Err(DaprError::RowCredibilityLen);
    }

    // Group rows by user, carrying each row's credibility with it and preserving
    // each row's relative order within its user so largest-remainder tie-breaks
    // are reproducible. `BTreeMap` keeps user iteration order stable.
    let mut rows_by_user: BTreeMap<&UserId, Vec<(&RawRow, u64)>> = BTreeMap::new();
    for (row, &cred_ppm) in rows.iter().zip(credibility_ppm) {
        // Clamp on the way in: an above-neutral value degrades to neutral.
        rows_by_user
            .entry(&row.user)
            .or_default()
            .push((row, cred_ppm.min(1_000_000)));
    }

    let mut per_work: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut unallocated: u128 = 0;
    // Reputation accumulators: total bandwidth-adjusted usage, and the set of
    // distinct users, per work. Populated alongside the payout pass so the two
    // signals are always computed from the same row data.
    let mut rep_usage: BTreeMap<WorkId, u128> = BTreeMap::new();
    let mut rep_users: BTreeMap<WorkId, std::collections::BTreeSet<&UserId>> = BTreeMap::new();

    // Apportion every paying user's fee. A user with no usage rows (or only
    // zero-raw rows) has nowhere to send their fee, so it becomes `unallocated`.
    for (user, fee) in tier_fees {
        let user_rows = rows_by_user.get(user).cloned().unwrap_or_default();

        // Compute each row's cred (bandwidth-discounted raw) and this user's
        // totals of raw and cred, in one pass.
        let mut creds = Vec::with_capacity(user_rows.len());
        let mut rw_u: u128 = 0;
        let mut sum_cred: u128 = 0;
        for (row, cred_ppm) in &user_rows {
            let cred = mul_div(row.raw, *cred_ppm as u128, 1_000_000)?;
            rw_u = rw_u.checked_add(row.raw).ok_or(DaprError::Overflow)?;
            sum_cred = sum_cred.checked_add(cred).ok_or(DaprError::Overflow)?;
            creds.push(cred);

            // Reputation uses `cred` (bandwidth-adjusted usage), accumulated
            // into a local before inserting to avoid re-borrowing the map
            // while it is already borrowed by `entry`.
            let updated = rep_usage.get(&row.work).copied().unwrap_or(0);
            let updated = updated.checked_add(cred).ok_or(DaprError::Overflow)?;
            rep_usage.insert(row.work.clone(), updated);
            rep_users.entry(row.work.clone()).or_default().insert(user);
        }

        // No attributable raw value at all → the whole fee is unallocated.
        if rw_u == 0 || sum_cred == 0 {
            unallocated = unallocated.checked_add(*fee).ok_or(DaprError::Overflow)?;
            continue;
        }

        // Amount actually paid to works this user's fee funds: `fee` scaled by
        // the bandwidth-discounted share of raw value. `≤ fee`; the shortfall is
        // the bandwidth discount, which goes to `unallocated` below.
        let target = mul_div(*fee, sum_cred, rw_u)?;
        unallocated = unallocated
            .checked_add(fee.checked_sub(target).ok_or(DaprError::Overflow)?)
            .ok_or(DaprError::Overflow)?;

        // Split `target` across rows by cred, exactly (largest remainder), and
        // fold the results into the per-work totals.
        let shares = apportion(target, &creds, sum_cred)?;
        for ((row, _), share) in user_rows.iter().zip(shares) {
            let entry = per_work.entry(row.work.clone()).or_insert(0);
            *entry = entry.checked_add(share).ok_or(DaprError::Overflow)?;
        }
    }

    // Fold the accumulators into the public `Reputation` shape.
    let reputation = rep_usage
        .into_iter()
        .map(|(w, weighted_usage)| {
            let distinct_users = rep_users.get(&w).map(|s| s.len() as u64).unwrap_or(0);
            (
                w,
                Reputation {
                    distinct_users,
                    weighted_usage,
                },
            )
        })
        .collect();

    Ok(Payouts {
        per_work,
        unallocated,
        reputation,
    })
}
```

- [ ] **Step 4: Add the new error variant**

In the `DaprError` enum in `sims/src/lib.rs`, add:

```rust
    /// The per-row credibility slice's length did not match the row slice's.
    #[error("per-row credibility length does not match the number of rows")]
    RowCredibilityLen,
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p cwe-dapr`
Expected: PASS — the six new tests AND every pre-existing `cwe-dapr` test, including the
fixture oracle tests in `sims/tests/fixtures.rs`, unchanged. If any pre-existing
expected value now differs, the wrapper is wrong — fix the code, never the fixture.

- [ ] **Step 6: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-dapr --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 7: Commit**

```bash
git add sims/src/lib.rs
git commit -m "dapr: per-row bandwidth credibility core; per-work API becomes a wrapper"
```

---

### Task 3: `STORAGE_NODE` credential type

**Files:**
- Modify: `chain/contracts/CredentialTypes.sol`
- Test: `chain/test/CWEIdentity.t.sol`

**Interfaces:**
- Consumes: nothing.
- Produces: `CredentialTypes.STORAGE_NODE == keccak256("cwe.credential.storage-node")`.
  Task 6 recomputes this same value in Rust; Task 7's demo computes it with
  `cast keccak "cwe.credential.storage-node"`. All three must agree.

- [ ] **Step 1: Write the failing test**

The existing file declares `CWEIdentity internal id;` and `address internal issuer`, and
imports only `CWEIdentity` — so add the `CredentialTypes` import alongside it:

```solidity
import {CredentialTypes} from "../contracts/CredentialTypes.sol";
```

Then append this test inside `contract CWEIdentityTest`, reusing the existing `id`/
`issuer` fixtures (do not introduce new ones):

```solidity
    /// @notice The storage-node credential tag is the agreed keccak of its label,
    ///         and it behaves like any other credential: attestable, valid,
    ///         revocable. The settlement aggregator recomputes this same constant
    ///         off-chain, so a drift here would silently stop every bandwidth
    ///         receipt from counting.
    function test_storageNode_lifecycle() public {
        assertEq(CredentialTypes.STORAGE_NODE, keccak256("cwe.credential.storage-node"));

        address node = makeAddr("storage-node");
        vm.prank(issuer);
        id.attest(node, CredentialTypes.STORAGE_NODE, type(uint64).max);
        assertTrue(id.isValid(node, CredentialTypes.STORAGE_NODE));

        vm.prank(issuer);
        id.revoke(node, CredentialTypes.STORAGE_NODE);
        assertFalse(id.isValid(node, CredentialTypes.STORAGE_NODE));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd chain && forge test --match-test test_storageNode_lifecycle`
Expected: FAIL — `Identifier not found or not unique: STORAGE_NODE`.

- [ ] **Step 3: Add the constant**

In `chain/contracts/CredentialTypes.sol`, inside the library after `JUROR`:

```solidity
    /// @notice A credentialed storage node, whose co-signed bandwidth receipts the
    ///         settlement aggregator will count toward bandwidth credibility.
    bytes32 internal constant STORAGE_NODE = keccak256("cwe.credential.storage-node");
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd chain && forge test --match-test test_storageNode_lifecycle`
Expected: PASS.

- [ ] **Step 5: Run the whole contract suite**

Run: `cd chain && forge test`
Expected: PASS, no regressions. Do **not** run `forge fmt`.

- [ ] **Step 6: Commit**

```bash
git add chain/contracts/CredentialTypes.sol chain/test/CWEIdentity.t.sol
git commit -m "chain: add the storage-node credential type"
```

---

### Task 4: `cwe-storage` — the serving node

**Files:**
- Create: `services/storage/Cargo.toml`
- Create: `services/storage/src/lib.rs` (the ledger + receipt-issuance core)
- Create: `services/storage/src/main.rs` (the axum HTTP node)
- Modify: `Cargo.toml` (workspace `members`)
- Test: inline `#[cfg(test)] mod tests` in `services/storage/src/lib.rs`

**Interfaces:**
- Consumes: `cwe_receipt::{Receipt, SignedReceipt}` (Task 1).
- Produces (used by Task 5's client and Task 7's demo):
  - `pub struct ServedChunk { work_id: String, consumer: String, bytes: u64 }`
  - `pub struct Ledger` with `record(&mut self, session: &str, chunk: u64, served: ServedChunk)`,
    `get(&self, session: &str, chunk: u64) -> Option<&ServedChunk>`
  - `pub fn issue_receipt(ledger: &Ledger, node_addr: &str, epoch: u64, session: &str, chunk: u64) -> Result<Receipt, StorageError>`
  - `pub fn fragment(dir: &Path, work_id: &str, offset: u64, len: u64) -> Result<Vec<u8>, StorageError>`
  - HTTP surface of the `cwe-storage` binary:
    - `GET /content/{work_id}?consumer={addr}&session={hex}&chunk={n}&offset={n}&len={n}`
      → `200` with the raw fragment bytes (`application/octet-stream`), recording what it served
    - `POST /receipt` with body `{"session_nonce":"0x..","chunk_nonce":n}`
      → `200` `{"receipt": {...}, "node_sig": "0x.."}`, or `404` if nothing was served for that key
    - `GET /health` → `200 "ok"`
  - Environment: `CONTENT_DIR` (required), `PRIVATE_KEY` (required, node key), `EPOCH`
    (required), `PORT` (default `8546`).

**Design notes for the implementer:**
- Content lives as `"{CONTENT_DIR}/{work_id}.bin"`, `work_id` being the lowercase `0x` hex
  form. Reject any `work_id` that is not exactly `0x` + 64 hex characters *before*
  touching the filesystem — that is what stops a path-traversal `work_id` like `../../etc`.
- The node signs **its own** byte count from the ledger, never a count the caller supplies.
  `issue_receipt` returning `StorageError::NotServed` for an unknown key is the
  "refuses to sign for bytes it didn't serve" property the spec calls for.
- The ledger is in-memory behind a `tokio::sync::RwLock` in `main.rs`; a restart forgetting
  sessions is acceptable for cycle 1 (a node with no memory of serving simply won't sign).
- Serve at most `len` bytes and clamp to the file's end; record the **actual** length.

- [ ] **Step 1: Create the crate manifest and register it in the workspace**

`services/storage/Cargo.toml`:

```toml
# H5 — a minimal storage node: serves real content bytes and co-signs the
# bandwidth receipts that prove they moved.
[package]
name = "cwe-storage"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Minimal content-serving storage node for the Clean Web Economy"

[dependencies]
cwe-receipt = { path = "../../libs/receipt" }
alloy = { version = "2", features = ["full"] }
axum = "0.8"
# `sync` is declared explicitly because the node holds its served-chunk ledger
# behind a tokio::sync::RwLock.
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "sync"] }
# The consumer client speaks HTTP to the node; rustls avoids a system OpenSSL.
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[[bin]]
name = "cwe-storage"
path = "src/main.rs"

[[bin]]
name = "bandwidth-client"
path = "src/bin/bandwidth_client.rs"
```

In the root `Cargo.toml` `members`, after `"services/settlement",`:

```toml
    "services/storage",    # H5 — content-serving storage node + receipt client
```

Note: `[[bin]] bandwidth-client` refers to a file Task 5 creates. To keep this task's
build green on its own, create `services/storage/src/bin/bandwidth_client.rs` now with a
placeholder `fn main() {}` carrying a `//! Implemented in Task 5.` module comment, and
replace it wholesale in Task 5.

- [ ] **Step 2: Write the failing tests**

Create `services/storage/src/lib.rs` with ONLY this test module for now:

```rust
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
        assert_eq!(issue_receipt(&l, "0xn0de", 7, "0xfeed", 0).unwrap().bytes, 999_999);
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
        assert!(matches!(fragment(&dir, "0xzz", 0, 16), Err(StorageError::BadWorkId)));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p cwe-storage --lib`
Expected: FAIL — `cannot find type Ledger in this scope` and friends.

- [ ] **Step 4: Implement the library core**

Prepend to `services/storage/src/lib.rs`:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p cwe-storage --lib`
Expected: PASS — 5 tests.

- [ ] **Step 6: Implement the HTTP node**

Create `services/storage/src/main.rs`:

```rust
//! The `cwe-storage` node: serves content fragments over HTTP and co-signs a
//! bandwidth receipt for each fragment it actually delivered.
//!
//! Cycle 1 is deliberately a single plain-HTTP node, not a swarm: the point is to
//! make real bytes move and to produce evidence the aggregator can verify, not to
//! build content distribution. Peer discovery, redundancy, proof-of-storage and
//! the rest live in the deferred storage-swarm cycle.
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

/// Serve a fragment of a work's content and record exactly what was delivered.
///
/// The recorded byte count is the length of the body actually returned — not
/// what the caller asked for — so a clamped or short read attests only the bytes
/// that really moved.
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
    // request will ask for it.
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
```

Note on axum 0.8: path parameters use `{work_id}` braces, not the `:work_id` colon form
of 0.7. If the router panics at startup with a path-syntax error, that is the cause.

- [ ] **Step 7: Verify the node builds and its tests pass**

Run: `cargo build -p cwe-storage && cargo test -p cwe-storage`
Expected: builds clean; the 5 lib tests pass.

- [ ] **Step 8: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-storage --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock services/storage
git commit -m "storage: minimal content-serving node that co-signs bandwidth receipts"
```

---

### Task 5: `bandwidth-client` — the consumer side

**Files:**
- Create (replacing the Task 4 placeholder): `services/storage/src/bin/bandwidth_client.rs`
- Test: exercised end-to-end by Task 7's demo; the pure logic it relies on is already
  covered by Tasks 1 and 4.

**Interfaces:**
- Consumes: `cwe_receipt::{Receipt, ReceiptBundle, SignedReceipt}`; the node's HTTP
  surface from Task 4.
- Produces: a `bandwidth-client` binary writing a `ReceiptBundle` JSON file. Environment:
  `STORAGE_URL` (default `http://127.0.0.1:8546`), `WORK_ID`, `PRIVATE_KEY` (the
  consumer's wallet key — the same key that submits usage on-chain), `EPOCH`,
  `CHUNKS` (default `4`), `CHUNK_LEN` (default `131072`), `OUT` (bundle path, required).

**Design notes for the implementer:**
- The consumer verifies the node's signature **before** counter-signing. Counter-signing a
  receipt whose node signature is bad would just waste the consumer's own credibility.
- The session nonce is derived deterministically from the consumer address, work id and
  epoch (no `rand` dependency needed, and it makes the demo reproducible):
  `keccak256(consumer || work_id || epoch)`.
- Bytes are downloaded and their length checked, but the payload itself is discarded —
  cycle 1 proves bytes *moved*, not that they were retained.

- [ ] **Step 1: Write the client**

Replace `services/storage/src/bin/bandwidth_client.rs` entirely:

```rust
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
        let consumer_sig = format!("0x{}", hex_string(&signer.sign_message_sync(&msg)?.as_bytes()));

        signed.push(SignedReceipt {
            receipt: resp.receipt,
            node_sig: resp.node_sig,
            consumer_sig,
        });
    }

    let bundle = ReceiptBundle { epoch, receipts: signed };
    std::fs::write(&out, bundle.to_json()?)?;
    println!(
        "bandwidth-client: {} receipts, {total_bytes} bytes for work {work_id} → {out}",
        bundle.receipts.len()
    );
    Ok(())
}
```

- [ ] **Step 2: Build it**

Run: `cargo build -p cwe-storage --bin bandwidth-client`
Expected: builds clean.

- [ ] **Step 3: Smoke-test node and client together by hand**

```bash
# Serve 512 KiB of a fixture work from a scratch content dir.
TMP=$(mktemp -d)
WORK=0x$(printf 'ab%.0s' {1..32})
head -c 524288 /dev/zero | tr '\0' 'x' > "$TMP/$WORK.bin"

# Anvil's first two deterministic dev keys: node and consumer.
NODE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
CONS_KEY=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d

CONTENT_DIR=$TMP PRIVATE_KEY=$NODE_KEY EPOCH=1 PORT=8546 \
  cargo run -q -p cwe-storage --bin cwe-storage &
NODE_PID=$!
until curl -sf http://127.0.0.1:8546/health >/dev/null; do :; done

STORAGE_URL=http://127.0.0.1:8546 WORK_ID=$WORK PRIVATE_KEY=$CONS_KEY EPOCH=1 \
  CHUNKS=4 CHUNK_LEN=131072 OUT=$TMP/receipts.json \
  cargo run -q -p cwe-storage --bin bandwidth-client

# Kill ONLY the node we started, by the exact PID we captured.
kill -TERM "$NODE_PID"
jq '.receipts | length, (map(.receipt.bytes) | add)' "$TMP/receipts.json"
rm -rf "$TMP"
```

Expected: the client prints `4 receipts, 524288 bytes`, and `jq` prints `4` then `524288`.

- [ ] **Step 4: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy -p cwe-storage --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add services/storage/src/bin/bandwidth_client.rs
git commit -m "storage: consumer client that downloads fragments and co-signs receipts"
```

---

### Task 6: Settlement — verify receipts, compute per-row credibility

**Files:**
- Modify: `services/settlement/Cargo.toml` (add `cwe-receipt`)
- Modify: `services/settlement/src/config.rs` (`Deployments.identity`, `Config.receipts_path`, `Config.rates`)
- Create: `services/settlement/src/receipts.rs`
- Modify: `services/settlement/src/lib.rs` (declare `pub mod receipts;`)
- Modify: `services/settlement/src/chain.rs` (`run_events`)
- Modify: `services/settlement/src/settle.rs` (add `settle_raw_with_row_credibility`)
- Test: inline `#[cfg(test)] mod tests` in `services/settlement/src/receipts.rs`

**Interfaces:**
- Consumes: `cwe_receipt::{ReceiptBundle, normalize_addr}` (Task 1);
  `cwe_dapr::allocate_from_raw_with_row_credibility` (Task 2);
  `CredentialTypes.STORAGE_NODE` (Task 3, recomputed in Rust).
- Produces:
  - `pub const RATE_SCALE: u128 = 1_000_000_000_000;`
  - `pub fn accept_receipts(bundle: &ReceiptBundle, epoch: u64, credentialed: &dyn Fn(&str) -> bool) -> BTreeMap<(String, String), u128>`
  - `pub fn row_credibility_ppm(rows: &[RawRow], verified: &BTreeMap<(String, String), u128>, rates: &BTreeMap<String, u64>) -> Vec<u64>`
  - `pub fn settle_raw_with_row_credibility(epoch, tier_fees, rows, credibility_ppm: &[u64], escrow_works) -> Result<Settlement, SettleError>`
  - `Config.receipts_path: Option<PathBuf>` (`RECEIPTS`), `Config.rates: BTreeMap<String, u64>` (`RATES`),
    `Deployments.identity: String`.

**Design notes for the implementer:**
- `accept_receipts` takes a **credential predicate** rather than a provider, so the whole
  accept/reject policy is synchronously unit-testable. `chain.rs` does the async
  `CWEIdentity.isValid` calls, caches them per node address, and passes a closure.
- Rejection reasons each get an `eprintln!` warning naming the receipt, matching how
  `run_events` already reports rejected submissions.
- Fail-closed rate handling is spec §4.1 and is the point of the whole task. Read it again
  before writing `row_credibility_ppm`.
- `expected = raw · rate / RATE_SCALE` can overflow a naive `u128` multiply (`raw` is
  bounded by `2^120`), so split the multiplication as shown.

- [ ] **Step 1: Write the failing tests**

Create `services/settlement/src/receipts.rs` with ONLY this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;

    /// Lowercase hex of a byte slice.
    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A canonical work id for the tests.
    fn work() -> String {
        format!("0x{}", "aa".repeat(32))
    }

    /// Co-sign a receipt for `bytes` at `chunk` in `epoch`.
    fn signed(
        node: &PrivateKeySigner,
        consumer: &PrivateKeySigner,
        bytes: u64,
        epoch: u64,
        chunk: u64,
    ) -> SignedReceipt {
        let receipt = Receipt {
            work_id: work(),
            consumer: format!("{:#x}", consumer.address()),
            node: format!("{:#x}", node.address()),
            bytes,
            epoch,
            session_nonce: format!("0x{}", "5c".repeat(32)),
            chunk_nonce: chunk,
        };
        let msg = receipt.canonical_bytes().unwrap();
        SignedReceipt {
            node_sig: format!("0x{}", hex_of(&node.sign_message_sync(&msg).unwrap().as_bytes())),
            consumer_sig: format!(
                "0x{}",
                hex_of(&consumer.sign_message_sync(&msg).unwrap().as_bytes())
            ),
            receipt,
        }
    }

    /// Accept every node (the "all credentialed" predicate).
    fn all_ok(_node: &str) -> bool {
        true
    }

    /// Valid receipts from a credentialed node sum per (user, work).
    #[test]
    fn sums_verified_bytes_per_user_and_work() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 2000, 5, 1),
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(3000));
    }

    /// A receipt from an UNCREDENTIALED node contributes nothing — the check
    /// that stops a fraudster spinning up their own node to co-sign fakes.
    #[test]
    fn drops_receipts_from_an_uncredentialed_node() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 1000, 5, 0)],
        };
        let got = accept_receipts(&bundle, 5, &|_n: &str| false);
        assert!(got.is_empty());
    }

    /// A replayed chunk (same node, session and chunk nonce) is counted once,
    /// even if the replay claims more bytes.
    #[test]
    fn drops_a_replayed_chunk() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![
                signed(&node, &consumer, 1000, 5, 0),
                signed(&node, &consumer, 9_000_000, 5, 0), // same dedup key
            ],
        };
        let got = accept_receipts(&bundle, 5, &all_ok);
        let key = (format!("{:#x}", consumer.address()), work());
        assert_eq!(got.get(&key).copied(), Some(1000));
    }

    /// A receipt bound to a different epoch is dropped.
    #[test]
    fn drops_a_receipt_from_another_epoch() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 1000, 4, 0)],
        };
        assert!(accept_receipts(&bundle, 5, &all_ok).is_empty());
    }

    /// A receipt whose signature no longer matches its contents is dropped.
    #[test]
    fn drops_a_tampered_receipt() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let mut r = signed(&node, &consumer, 1000, 5, 0);
        r.receipt.bytes = 5_000_000; // inflate after signing
        let bundle = ReceiptBundle { epoch: 5, receipts: vec![r] };
        assert!(accept_receipts(&bundle, 5, &all_ok).is_empty());
    }

    /// Bytes meeting or exceeding expectation give full credibility; the clamp
    /// means over-serving buys no extra credit.
    #[test]
    fn full_credibility_when_bytes_meet_expectation() {
        // raw = 1e12 → expected = rate bytes exactly.
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates), vec![1_000_000]);

        // Ten times the bytes still clamps to neutral.
        let mut over = BTreeMap::new();
        over.insert(("0xu".to_string(), work()), 81_920u128);
        assert_eq!(row_credibility_ppm(&rows, &over, &rates), vec![1_000_000]);
    }

    /// Half the expected bytes gives half credibility.
    #[test]
    fn partial_bytes_give_proportional_credibility() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 4096u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates), vec![500_000]);
    }

    /// No receipts at all → zero credibility → a strict loss downstream.
    #[test]
    fn no_bytes_gives_zero_credibility() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(row_credibility_ppm(&rows, &BTreeMap::new(), &rates), vec![0]);
    }

    /// A MISSING or ZERO rate FAILS CLOSED (credibility 0), never neutral —
    /// otherwise whoever controls the rate could switch the discount off (§4.1).
    #[test]
    fn missing_or_zero_rate_fails_closed() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1_000_000_000_000 }];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu".to_string(), work()), 99_999u128);

        // Missing entirely.
        assert_eq!(row_credibility_ppm(&rows, &verified, &BTreeMap::new()), vec![0]);

        // Explicitly zero.
        let mut zero = BTreeMap::new();
        zero.insert(work(), 0u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &zero), vec![0]);
    }

    /// A zero-WEIGHT row has no expectation to fall short of, so it stays
    /// neutral (there is nothing to discount).
    #[test]
    fn zero_weight_row_is_neutral() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 0 }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(row_credibility_ppm(&rows, &BTreeMap::new(), &rates), vec![1_000_000]);
    }

    /// Credibility is per-(user, work): one user's bytes do not credit another's
    /// row on the same work.
    #[test]
    fn credibility_is_per_user_and_work() {
        let rows = vec![
            RawRow { user: "0xu1".into(), work: work(), raw: 1_000_000_000_000 },
            RawRow { user: "0xu2".into(), work: work(), raw: 1_000_000_000_000 },
        ];
        let mut verified = BTreeMap::new();
        verified.insert(("0xu1".to_string(), work()), 8192u128);
        let mut rates = BTreeMap::new();
        rates.insert(work(), 8192u64);
        assert_eq!(row_credibility_ppm(&rows, &verified, &rates), vec![1_000_000, 0]);
    }

    /// A very large weight does not overflow the expected-bytes computation.
    #[test]
    fn large_weight_does_not_overflow() {
        let rows = vec![RawRow { user: "0xu".into(), work: work(), raw: 1u128 << 118 }];
        let mut rates = BTreeMap::new();
        rates.insert(work(), u64::MAX);
        // Expected is astronomically larger than any real byte count, so this is
        // a strict loss — but it must COMPUTE, not panic or wrap.
        assert_eq!(row_credibility_ppm(&rows, &BTreeMap::new(), &rates), vec![0]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-settlement --lib receipts`
Expected: FAIL — `cannot find function accept_receipts`.

- [ ] **Step 3: Implement the receipts module**

Prepend to `services/settlement/src/receipts.rs`:

```rust
//! Turning co-signed bandwidth receipts into per-row bandwidth credibility.
//!
//! This is the aggregator's half of the H5 receipt protocol. `cwe-receipt` proves
//! a receipt was co-signed and unaltered; everything that makes a receipt
//! *count* lives here: it must be bound to the epoch being settled, its node must
//! hold a valid storage-node credential, and its `(node, session, chunk)` key
//! must not have been seen before. Surviving receipts are summed per
//! (user, work) and compared against what that row's proven weight implies the
//! user should have had to download.

use std::collections::{BTreeMap, BTreeSet};

use cwe_dapr::RawRow;
use cwe_receipt::{normalize_addr, Receipt, ReceiptBundle, SignedReceipt};

/// The denominator of the `RATE(W)` rate constant: a rate is expressed as bytes
/// per `RATE_SCALE` units of proven weight.
///
/// Proven weights are `minutes · price_ppm · region_ppm · D(plays) / 1e6`, so
/// they run to ~10^13 for an hour of content — an integer "bytes per unit
/// weight" would always floor to zero. 10^12 of weight is exactly one minute of
/// a full-price (`price_ppm = 1e6`), neutral-region, first-play work, which makes
/// a rate read naturally as **bytes per minute of full-price content** (128 kbps
/// audio ≈ 960_000).
pub const RATE_SCALE: u128 = 1_000_000_000_000;

/// Verify a receipt bundle and sum the accepted bytes per (consumer, work).
///
/// A receipt is counted only if ALL of the following hold:
/// 1. both signatures recover to the addresses it names (`SignedReceipt::verify`);
/// 2. it is bound to `epoch` — a receipt from another epoch is a replay attempt;
/// 3. its node satisfies `credentialed`, i.e. holds a valid, non-revoked
///    storage-node credential. This is what stops a fraudster standing up their
///    own "node" and co-signing fabricated receipts with a colluding client;
/// 4. its `(node, session_nonce, chunk_nonce)` key has not already been counted
///    in this bundle.
///
/// Rejections are reported on stderr and simply not counted; one bad receipt
/// never invalidates a bundle, it just fails to earn credibility. Keys are the
/// lowercase `0x` (consumer, work) pair, matching [`RawRow`]'s `user`/`work`.
pub fn accept_receipts(
    bundle: &ReceiptBundle,
    epoch: u64,
    credentialed: &dyn Fn(&str) -> bool,
) -> BTreeMap<(String, String), u128> {
    let mut totals: BTreeMap<(String, String), u128> = BTreeMap::new();
    // Every dedup key counted so far this bundle; a repeat is dropped outright.
    let mut seen: BTreeSet<(String, String, u64)> = BTreeSet::new();

    for signed in &bundle.receipts {
        let r: &Receipt = &signed.receipt;

        // (1) The receipt must be intact and co-signed by exactly the two
        // parties it names.
        if let Err(e) = SignedReceipt::verify(signed) {
            eprintln!("warning: dropping receipt (bad signature): {e}");
            continue;
        }

        // (2) Epoch binding: a receipt earned in another epoch cannot be
        // re-spent in this one.
        if r.epoch != epoch {
            eprintln!(
                "warning: dropping receipt from node {} (epoch {} != settlement epoch {epoch})",
                r.node, r.epoch
            );
            continue;
        }

        // (3) Only credentialed storage nodes' attestations count.
        if !credentialed(&normalize_addr(&r.node)) {
            eprintln!(
                "warning: dropping receipt from node {} (no valid storage-node credential)",
                r.node
            );
            continue;
        }

        // (4) Anti-replay within the bundle.
        let key = r.dedup_key();
        if !seen.insert(key) {
            eprintln!(
                "warning: dropping replayed receipt (node {}, session {}, chunk {})",
                r.node, r.session_nonce, r.chunk_nonce
            );
            continue;
        }

        // Accepted: attribute the bytes to this (consumer, work) pair.
        let entry = totals
            .entry((normalize_addr(&r.consumer), r.work_id.to_ascii_lowercase()))
            .or_insert(0);
        *entry = entry.saturating_add(r.bytes as u128);
    }

    totals
}

/// Compute each row's bandwidth credibility in ppm from the verified byte totals.
///
/// ```text
/// expected_bytes = raw · rate(work) / RATE_SCALE
/// credibility    = clamp(verified_bytes · 1e6 / expected_bytes, 0, 1e6)
/// ```
///
/// Two edge cases matter, and they resolve in OPPOSITE directions on purpose
/// (spec §4.1):
///
/// * A work with **no configured rate, or a rate of zero**, FAILS CLOSED to
///   credibility `0`. Treating it as neutral would mean whoever controls the
///   rate could switch the bandwidth discount off entirely — a puppet-work
///   fraudster would simply publish `rate = 0` and collect in full. A
///   misconfiguration must cost the claimant, not the system, and it is logged
///   so it is loud rather than silent.
/// * A **zero-weight row** (or one whose expectation floors to zero) is neutral:
///   there is no claim to discount, so there is nothing to punish.
///
/// The clamp at neutral means over-serving buys no extra credit — bandwidth can
/// only ever discount a payout.
pub fn row_credibility_ppm(
    rows: &[RawRow],
    verified: &BTreeMap<(String, String), u128>,
    rates: &BTreeMap<String, u64>,
) -> Vec<u64> {
    rows.iter()
        .map(|row| {
            let work = row.work.to_ascii_lowercase();

            // Fail closed on a missing or zero rate.
            let rate = match rates.get(&work).copied() {
                Some(r) if r > 0 => r as u128,
                _ => {
                    eprintln!(
                        "warning: work {work} has no configured bandwidth rate; \
                         crediting row for user {} as zero-credibility",
                        row.user
                    );
                    return 0;
                }
            };

            let expected = mul_div_floor(row.raw, rate, RATE_SCALE);
            // Nothing expected → nothing to discount.
            if expected == 0 {
                return 1_000_000;
            }

            let bytes = verified
                .get(&(normalize_addr(&row.user), work))
                .copied()
                .unwrap_or(0);
            // Ratio in ppm, clamped at neutral.
            let ppm = mul_div_floor(bytes, 1_000_000, expected);
            std::cmp::min(ppm, 1_000_000) as u64
        })
        .collect()
}

/// `floor(a · b / d)` without overflowing on large `a`.
///
/// Proven weights reach ~2^120 and rates ~2^64, so a plain `a * b` would wrap.
/// Splitting `a` into quotient and remainder against `d` keeps every
/// intermediate product small enough: `(a/d)·b + ((a%d)·b)/d`. Saturating
/// arithmetic on the outer add means an absurd input degrades to `u128::MAX`
/// (an unmeetable expectation → zero credibility) rather than wrapping to a
/// small number that would hand out free credit.
fn mul_div_floor(a: u128, b: u128, d: u128) -> u128 {
    let q = a / d;
    let r = a % d;
    q.saturating_mul(b).saturating_add(r.saturating_mul(b) / d)
}
```

Add `pub mod receipts;` to `services/settlement/src/lib.rs` alongside the other module
declarations, and add the dependency to `services/settlement/Cargo.toml`:

```toml
cwe-receipt = { path = "../../libs/receipt" }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cwe-settlement --lib receipts`
Expected: PASS — 12 tests.

- [ ] **Step 5: Extend the config**

In `services/settlement/src/config.rs`:

Add to `Deployments`:

```rust
    /// The `CWEIdentity` address (for checking storage-node credentials when
    /// verifying bandwidth receipts).
    pub identity: String,
```

Add to `Config`:

```rust
    /// Path to a receipt bundle to verify (from `RECEIPTS`). `None` means no
    /// bandwidth signal was supplied, so credibility stays neutral and payouts
    /// are exactly what they were before H5 — which is what keeps every legacy
    /// demo working unchanged.
    pub receipts_path: Option<PathBuf>,
    /// Per-work bandwidth rates in bytes per `RATE_SCALE` units of proven weight
    /// (from `RATES`), keyed by lowercase `0x` work id. Empty when unset.
    ///
    /// This map is aggregator-side ON PURPOSE: whoever sets a rate can switch
    /// that work's bandwidth discount off, so it must not come from the payout
    /// beneficiary or from the (consumer-written) receipt bundle.
    pub rates: BTreeMap<String, u64>,
```

In `from_env`, after `disclosure_path`:

```rust
        // A receipt bundle is optional; without one, bandwidth stays neutral.
        let receipts_path = std::env::var("RECEIPTS").ok().map(PathBuf::from);

        // The per-work rate map, if supplied. Keys are lowercased so lookups
        // match the row keys settlement builds from chain data.
        let rates: BTreeMap<String, u64> = match std::env::var("RATES") {
            Ok(path) => {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| ConfigError::Rates(path.clone(), e.to_string()))?;
                let parsed: BTreeMap<String, u64> = serde_json::from_str(&raw)
                    .map_err(|e| ConfigError::Rates(path, e.to_string()))?;
                parsed
                    .into_iter()
                    .map(|(k, v)| (k.to_ascii_lowercase(), v))
                    .collect()
            }
            Err(_) => BTreeMap::new(),
        };
```

Add both fields to the returned `Config { .. }`, add `use std::collections::BTreeMap;`
at the top, extend the `from_env` doc comment to mention `RECEIPTS` and `RATES`, and add
the error variant:

```rust
    /// The rates file could not be read or parsed.
    #[error("loading rates file {0}: {1}")]
    Rates(String, String),
```

- [ ] **Step 6: Add the row-credibility settle entry point**

In `services/settlement/src/settle.rs`, beside `settle_raw`:

```rust
/// As [`settle_raw`], but driven by a PER-ROW bandwidth-credibility signal
/// instead of a per-work map.
///
/// This is the path a real bandwidth layer takes: verified receipts yield one
/// credibility per (user, work) row, which distinguishes an honest user from a
/// padder claiming the same work — something a per-work value cannot do. The
/// routing/Merkle/escrow tail is shared with [`settle_raw`].
pub fn settle_raw_with_row_credibility(
    epoch: u64,
    tier_fees: &BTreeMap<String, u128>,
    rows: &[RawRow],
    credibility_ppm: &[u64],
    escrow_works: &BTreeSet<String>,
) -> Result<Settlement, SettleError> {
    let payouts = allocate_from_raw_with_row_credibility(tier_fees, rows, credibility_ppm)?;
    finalize(epoch, payouts, escrow_works)
}
```

Add `allocate_from_raw_with_row_credibility` to the `cwe_dapr` import list in that file.

- [ ] **Step 7: Wire event mode**

In `services/settlement/src/chain.rs`, replace the final `Ok(settle_raw(...))` of
`run_events` (currently lines ~370–376) with:

```rust
    // Bandwidth credibility: with a receipt bundle present, verify it and turn
    // the verified bytes into a per-row discount; without one, stay neutral and
    // pay exactly as before H5.
    let credibility = match &cfg.receipts_path {
        Some(path) => {
            let raw = std::fs::read_to_string(path)?;
            let bundle = ReceiptBundle::from_json(&raw)?;

            // Resolve each distinct node's storage-node credential ONCE, then
            // hand `accept_receipts` a synchronous predicate over the results —
            // it keeps the accept/reject policy free of async and of the chain.
            let identity_addr = Address::from_str(&cfg.deployments.identity)?;
            let identity = Identity::new(identity_addr, provider);
            let cred_type = keccak256(b"cwe.credential.storage-node");
            let mut valid: BTreeMap<String, bool> = BTreeMap::new();
            for signed in &bundle.receipts {
                let node = normalize_addr(&signed.receipt.node);
                if valid.contains_key(&node) {
                    continue;
                }
                // A node address that will not even parse cannot be credentialed.
                let ok = match Address::from_str(&node) {
                    Ok(addr) => identity.isValid(addr, cred_type).call().await?,
                    Err(_) => false,
                };
                valid.insert(node, ok);
            }

            let verified = accept_receipts(&bundle, cfg.epoch, &|node: &str| {
                valid.get(node).copied().unwrap_or(false)
            });
            let ppm = row_credibility_ppm(&rows, &verified, &cfg.rates);
            eprintln!(
                "bandwidth: {} receipts submitted, {} (user, work) pairs credited",
                bundle.receipts.len(),
                verified.len()
            );
            Some(ppm)
        }
        None => None,
    };

    // Pay from the proven weights. Escrow is empty for cycle-1 — every proven
    // work pays directly (see the fn doc).
    Ok(match credibility {
        Some(ppm) => settle_raw_with_row_credibility(
            cfg.epoch,
            &tier_fees,
            &rows,
            &ppm,
            &BTreeSet::new(),
        )?,
        None => settle_raw(cfg.epoch, &tier_fees, &rows, &BTreeMap::new(), &BTreeSet::new())?,
    })
```

Add the needed imports at the top of `chain.rs`:

```rust
use alloy::primitives::keccak256;
use cwe_receipt::{normalize_addr, ReceiptBundle};

use crate::receipts::{accept_receipts, row_credibility_ppm};
use crate::settle::settle_raw_with_row_credibility;
```

and declare the `CWEIdentity` binding beside the existing `sol!` contract bindings in
that file (match the surrounding style exactly — check how `Tiers`/`Beacon` are declared):

```rust
sol! {
    /// Minimal `CWEIdentity` view used to check storage-node credentials.
    #[sol(rpc)]
    interface Identity {
        function isValid(address subject, bytes32 credType) external view returns (bool);
    }
}
```

Also extend `run_events`'s doc comment with a bullet noting that a `RECEIPTS` bundle
drives per-row bandwidth credibility and that its absence means neutral.

- [ ] **Step 8: Run the full settlement suite**

Run: `cargo test -p cwe-settlement`
Expected: PASS, including every pre-existing test unchanged. Any settlement test that
constructs a `Deployments` literal will need the new `identity` field — add it there;
that is a test-fixture change, not an expected-value change.

- [ ] **Step 9: Check formatting and lints**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 10: Commit**

```bash
git add services/settlement Cargo.lock
git commit -m "settlement: verify bandwidth receipts and drive per-row credibility in event mode"
```

---

### Task 7: `make bandwidth-demo` + CI

**Files:**
- Create: `ops/demo/run_bandwidth_demo.sh`
- Modify: `ops/Makefile`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: everything from Tasks 1–6, plus the existing `zk_submit` binary
  (`--mode honest`, env `RPC_URL`/`PRIVATE_KEY`/`DEPLOYMENTS`/`TIER`/`WORK_ID`) and the
  `cwe-settlement` binary (env `RPC_URL`/`PRIVATE_KEY`/`EPOCH`/`DEPLOYMENTS`/`OUT`, plus
  the new `RECEIPTS`/`RATES`).
- Produces: `make -C ops bandwidth-demo`; CI job `bandwidth-e2e`.

**Design notes for the implementer:**
- Model the script closely on `ops/zk_demo.sh` — same Anvil startup, same PID discipline,
  same deploy/beacon/tier/register sequence. Read it first.
- **Three users submit in the same epoch, then ONE settlement run** proves all three
  outcomes at once (a user may submit only once per epoch, so each act needs its own
  user).
- Expected bytes for the demo: the sample row's proven weight is exactly
  `45_000_000_000_000`, so with `rate = 8192`, `expected = 45 · 8192 = 368_640` bytes.
  Serving 512 KiB (524_288) puts the honest consumer comfortably over expectation, so it
  clamps to full credit and the demo does not hinge on the weight to the byte.
- All three works get a rate entry. That is deliberate: it proves Acts 2 and 3 fail
  because bytes were missing or the node was uncredentialed, not because a rate was absent.
- The rogue node must run on a **different port** with a **different key** and get no
  credential.

- [ ] **Step 1: Write the demo script**

Create `ops/demo/run_bandwidth_demo.sh` (make it executable: `chmod +x`):

```bash
#!/usr/bin/env bash
#
# Bandwidth-receipt end-to-end demo — the H5 capstone.
#
# Proves that the bandwidth-credibility knob is LIVE: real bytes move over HTTP,
# consumer and storage node co-sign receipts for them, and the aggregator turns
# verified receipts into a per-(user, work) credibility that decides who gets
# paid. Everything runs against a fresh local Anvil node in EVENT mode, so the
# usage half is backed by real Groth16 proofs, not a disclosure file.
#
#   Act 1 (honest):        downloads 512 KiB of work W from a CREDENTIALED node,
#                          co-signs receipts, and is paid in full.
#   Act 2 (puppet work):   claims heavy usage of work F but downloads nothing.
#                          Credibility 0 → its fee is BURNED, F earns nothing.
#   Act 3 (rogue node):    downloads real bytes of work G, but from a node with
#                          no storage-node credential. Receipts are rejected →
#                          same strict loss.
#
# Requirements: foundry (anvil/forge/cast), cargo, jq, curl. No Docker — the
# script starts and stops its own Anvil and storage nodes, killing only the exact
# PIDs it started.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC="http://127.0.0.1:8545"
WORKDIR="$(mktemp -d)"
CONTENT="$WORKDIR/content"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$CONTENT"

step() { echo; echo "=== $* ==="; }
fail() { echo "❌ $*"; exit 1; }

# --- build (release: the prover must not run in debug) ----------------------
step "Building settlement, zk_submit and the storage node (release)"
cargo build --release --quiet -p cwe-settlement -p cwe-storage --manifest-path "$ROOT/Cargo.toml"
SETTLE="$ROOT/target/release/cwe-settlement"
ZK_SUBMIT="$ROOT/target/release/zk_submit"
STORAGE="$ROOT/target/release/cwe-storage"
CLIENT="$ROOT/target/release/bandwidth-client"

# --- regenerate the devnet proving key if missing (fresh checkout) ----------
if [ ! -f "$ROOT/chain/zk/proving_key.bin" ]; then
  step "Regenerating devnet proving key (missing)"
  echo "regenerating devnet proving key (missing)... (~85s, deterministic)"
  ( cd "$ROOT" && cargo run --release --quiet -p cwe-zk-circuits --bin export_keys )
fi

# --- start Anvil (stop only the processes we start) -------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
GOOD_NODE_PID=""
ROGUE_NODE_PID=""
cleanup() {
  # Kill ONLY the exact PIDs this script started.
  [ -n "$GOOD_NODE_PID" ]  && kill -TERM "$GOOD_NODE_PID"  2>/dev/null || true
  [ -n "$ROGUE_NODE_PID" ] && kill -TERM "$ROGUE_NODE_PID" 2>/dev/null || true
  kill -TERM "$ANVIL_PID" 2>/dev/null || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && break; done

# Anvil's deterministic dev keys.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                          # owner + issuer + aggregator
U1=${KEYS[1]}                                # honest consumer      (Act 1)
U2=${KEYS[2]}                                # puppet-work claimant (Act 2)
U3=${KEYS[3]}                                # rogue-node consumer  (Act 3)
GOOD_NODE_KEY=${KEYS[4]}                     # credentialed storage node
ROGUE_NODE_KEY=${KEYS[5]}                    # uncredentialed storage node
PAYEE_W=$(cast wallet address ${KEYS[6]})    # creator of the honest work
PAYEE_F=$(cast wallet address ${KEYS[7]})    # creator of the puppet work
PAYEE_G=$(cast wallet address ${KEYS[8]})    # creator of the rogue-served work

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }

# --- step 1: deploy with the real Groth16 verifier ---------------------------
step "1. Deploying contracts (VERIFIER=groth16)"
( cd "$ROOT/chain" && VERIFIER=groth16 PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
CONS=$(jq -r .consumption "$DEP"); BEACON=$(jq -r .beacon "$DEP")
TIERS=$(jq -r .tiers "$DEP"); REG=$(jq -r .registry "$DEP")
IDENTITY=$(jq -r .identity "$DEP")

# --- step 2: epoch beacon key ------------------------------------------------
step "2. Publishing the epoch beacon key"
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
KEY=0x$(printf '5c%.0s' {1..32})
send $DEPLOYER $BEACON "setKey(uint256,bytes32)" $EPOCH $KEY
echo "epoch=$EPOCH"

# --- step 3: credentials, works, tier fee, subscriptions ---------------------
step "3. Registering works, credentialing the storage node, funding subscriptions"
LIGHT=$(cast keccak "light"); FEE=1000000000000000000     # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
FAR=18446744073709551615                                  # type(uint64).max
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE

# The deployer is a trusted issuer and attests itself a verified-creator
# credential so it may register works.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR

# The GOOD node gets a storage-node credential; the ROGUE node deliberately does not.
SN=$(cast keccak "cwe.credential.storage-node")
GOOD_NODE_ADDR=$(cast wallet address $GOOD_NODE_KEY)
ROGUE_NODE_ADDR=$(cast wallet address $ROGUE_NODE_KEY)
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $GOOD_NODE_ADDR $SN $FAR
echo "credentialed node=$GOOD_NODE_ADDR   rogue node=$ROGUE_NODE_ADDR (no credential)"

# Register one work per act, each with its own payee's consent signature.
register_work() {                      # $1 label  $2 payee  $3 payee key
  local work content digest sig
  work=$(cast format-bytes32-string "$1")
  content=$(cast keccak "content-$1")
  digest=$(cast call --rpc-url $RPC $REG \
    "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" "$work" "$content" "$2" "$PPM")
  sig=$(cast wallet sign --private-key "$3" "$digest")
  send $DEPLOYER $REG \
    "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
    "$work" "$content" "[$2]" "[1000000]" "[$sig]" $PPM $EU
  echo "$work"
}
WORK_W=$(register_work bwwork  "$PAYEE_W" "${KEYS[6]}")
WORK_F=$(register_work bwpuppet "$PAYEE_F" "${KEYS[7]}")
WORK_G=$(register_work bwrogue  "$PAYEE_G" "${KEYS[8]}")

# All three users subscribe, each funding the pool with one tier fee.
send $U1 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U2 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U3 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE

# --- step 4: content + rates -------------------------------------------------
# 512 KiB of deterministic content per work. Expected bytes for the demo's
# sample row is weight(45e12) x rate(8192) / 1e12 = 368640, so a full download
# clears expectation and clamps to full credit.
step "4. Publishing content and the aggregator's bandwidth rates"
for W in "$WORK_W" "$WORK_G"; do
  head -c 524288 /dev/zero | tr '\0' 'x' > "$CONTENT/${W,,}.bin"
done
RATES="$WORKDIR/rates.json"
jq -n --arg w "${WORK_W,,}" --arg f "${WORK_F,,}" --arg g "${WORK_G,,}" \
  '{($w): 8192, ($f): 8192, ($g): 8192}' > "$RATES"
echo "content=512KiB per work; rate=8192 bytes per 1e12 weight for all three works"

# --- step 5: start both storage nodes ---------------------------------------
step "5. Starting the credentialed node (8546) and the rogue node (8547)"
CONTENT_DIR=$CONTENT PRIVATE_KEY=$GOOD_NODE_KEY EPOCH=$EPOCH PORT=8546 \
  "$STORAGE" > "$WORKDIR/node-good.log" 2>&1 &
GOOD_NODE_PID=$!
CONTENT_DIR=$CONTENT PRIVATE_KEY=$ROGUE_NODE_KEY EPOCH=$EPOCH PORT=8547 \
  "$STORAGE" > "$WORKDIR/node-rogue.log" 2>&1 &
ROGUE_NODE_PID=$!
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8546/health >/dev/null 2>&1 && break; done
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8547/health >/dev/null 2>&1 && break; done

cd "$ROOT"   # zk_submit/settlement resolve chain/zk/*.bin from the repo root

# =========================================================================
# ACT 1 — honest: real bytes from a credentialed node
# =========================================================================
step "ACT 1 — honest consumer downloads real bytes and submits proven usage"
RPC_URL=$RPC PRIVATE_KEY=$U1 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_W \
  "$ZK_SUBMIT" --mode honest || fail "honest submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_W,,} PRIVATE_KEY=$U1 EPOCH=$EPOCH \
  CHUNKS=4 CHUNK_LEN=131072 OUT="$WORKDIR/r1.json" "$CLIENT" \
  || fail "honest consumer failed to collect receipts"

# =========================================================================
# ACT 2 — puppet work: usage claimed, no bytes ever moved
# =========================================================================
step "ACT 2 — puppet work claimed with NO downloads"
RPC_URL=$RPC PRIVATE_KEY=$U2 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_F \
  "$ZK_SUBMIT" --mode honest || fail "puppet submit did not succeed"
# Deliberately no receipts for U2.

# =========================================================================
# ACT 3 — rogue node: real bytes, but from an uncredentialed node
# =========================================================================
step "ACT 3 — real bytes served by an UNCREDENTIALED node"
RPC_URL=$RPC PRIVATE_KEY=$U3 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_G \
  "$ZK_SUBMIT" --mode honest || fail "rogue-node submit did not succeed"
STORAGE_URL=http://127.0.0.1:8547 WORK_ID=${WORK_G,,} PRIVATE_KEY=$U3 EPOCH=$EPOCH \
  CHUNKS=4 CHUNK_LEN=131072 OUT="$WORKDIR/r3.json" "$CLIENT" \
  || fail "rogue-node consumer failed to collect receipts"

# --- settle once over all three submissions ----------------------------------
step "Settling the epoch with the combined receipt bundle"
# Merge both consumers' bundles into the single bundle the aggregator verifies.
BUNDLE="$WORKDIR/receipts.json"
jq -s '{epoch: .[0].epoch, receipts: (.[0].receipts + .[1].receipts)}' \
  "$WORKDIR/r1.json" "$WORKDIR/r3.json" > "$BUNDLE"

OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DEPLOYMENTS=$DEP \
  RECEIPTS=$BUNDLE RATES=$RATES OUT=$OUT "$SETTLE" \
  || fail "settlement (event mode + receipts) failed"

# --- assertions ---------------------------------------------------------------
step "Assertions"
credit_of() { jq -r --arg w "$1" '[.entries[] | select(.work_id == $w) | .amount][0] // "0"' "$OUT"; }
CREDIT_W=$(credit_of "${WORK_W,,}")
CREDIT_F=$(credit_of "${WORK_F,,}")
CREDIT_G=$(credit_of "${WORK_G,,}")
TOTAL=$(jq -r '.total_credits' "$OUT"); UNALLOC=$(jq -r '.unallocated' "$OUT")

# 1. The honest work is paid its consumer's whole fee (single row, full credibility).
[ "$CREDIT_W" = "$FEE" ] || fail "honest work earned $CREDIT_W, expected the full fee $FEE"
# 2. The puppet work earned nothing — a strict loss, not a transfer.
[ "$CREDIT_F" = "0" ] || fail "puppet work earned $CREDIT_F, expected 0"
# 3. The rogue-node-backed work earned nothing either.
[ "$CREDIT_G" = "0" ] || fail "rogue-node work earned $CREDIT_G, expected 0"
# 4. The two failed claims' fees were BURNED, not redistributed.
EXPECTED_BURN=$((2 * FEE))
[ "$UNALLOC" = "$EXPECTED_BURN" ] \
  || fail "expected $EXPECTED_BURN wei burned, got $UNALLOC"
# 5. Conservation across all three subscriptions.
SUM=$((TOTAL + UNALLOC)); THREE=$((3 * FEE))
[ "$SUM" = "$THREE" ] || fail "fees not conserved: $TOTAL + $UNALLOC != $THREE"

echo "  honest work paid $(cast to-unit $CREDIT_W ether) ETH"
echo "  puppet work paid 0; rogue-node work paid 0"
echo "  burned $(cast to-unit $UNALLOC ether) ETH; fees conserved ($SUM wei)"

echo
echo "✅ BANDWIDTH DEMO PASSED — real bytes pay, no-bytes and uncredentialed-node claims are a strict loss."
```

- [ ] **Step 2: Add the Makefile target**

In `ops/Makefile`, add `bandwidth-demo` to the `.PHONY` list and add the target after
`zk-demo`:

```make
bandwidth-demo: ## Run the bandwidth-receipt end-to-end demo (self-contained Anvil)
	bash demo/run_bandwidth_demo.sh
```

- [ ] **Step 3: Run the demo**

Run: `make -C ops bandwidth-demo`
Expected: `✅ BANDWIDTH DEMO PASSED`.

If Act 1's assertion fails with a credit *below* the full fee, print the settlement's
stderr — the `bandwidth:` line reports how many (user, work) pairs were credited. A
credited count of 0 means receipts were dropped (check the node credential and the epoch);
a partial credit means expected bytes exceeded what was served (lower the rate).

- [ ] **Step 4: Add the CI job**

In `.github/workflows/ci.yml`, after the `zk-e2e` job, add (matching its style exactly):

```yaml
  bandwidth-e2e:
    name: Bandwidth-receipt end-to-end demo (Anvil)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Install Foundry
        uses: foundry-rs/foundry-toolchain@v1

      # `jq` builds the rates file and reads the settlement's proof output;
      # `curl` waits for the storage nodes to come up.
      - name: Install jq
        run: sudo apt-get update && sudo apt-get install -y jq

      - name: Run the demo
        run: make -C ops bandwidth-demo
```

- [ ] **Step 5: Commit**

```bash
git add ops/demo/run_bandwidth_demo.sh ops/Makefile .github/workflows/ci.yml
git commit -m "ops: bandwidth-receipt end-to-end demo and its CI job"
```

---

### Task 8: Documentation and status sync

**Files:**
- Modify: `docs/roadmap.md` (the H5 entry, the "Where we are" table, the "What is real vs. stubbed" Storage and Anti-fraud rows, §5's near-term list, the mermaid graph)
- Modify: `ROADMAP.md`
- Modify: `project-map.js`

**Interfaces:** none (documentation only).

**Design notes:** `CLAUDE.md` requires `ROADMAP.md`, `docs/roadmap.md` and
`project-map.js` to flip **together, in one step**. Keep the map factual — status tracks
reality, never aspiration. Do not touch `project-map.html` (the renderer is generic).

- [ ] **Step 1: Update `docs/roadmap.md`**

- Mark **H5** ✅ for cycle 1, describing what shipped (`cwe-receipt`, `cwe-storage` + its
  consumer client, per-row DAPR credibility, receipt verification in settlement's event
  mode, the storage-node credential, `make bandwidth-demo`) and listing the deferred
  sub-items **exactly as the spec §1.2 names them**: ZK bandwidth proof, peer-diversity
  proof, full P2P storage swarm, node compliance & staking/slashing, ephemeral-key
  unlinkability. Note that `RATE(W)` is aggregator-configured this cycle and that
  manifest/registry sourcing needs a protocol floor (spec §4.1).
- In the "Where we are" table, add a **Storage** row (`services/storage` — `cwe-storage`)
  and a **Bandwidth receipts** row (`libs/receipt`), and extend the Payout-math and
  Settlement rows to mention per-row bandwidth credibility.
- In "What is real vs. stubbed": the **Storage** row moves from "none" to the honest
  cycle-1 description (a single HTTP node serving real bytes; swarm/redundancy/
  proof-of-storage deferred). The **Anti-fraud** row gains the live bandwidth signal.
- In the demos line, `make bandwidth-demo` and the `bandwidth-e2e` CI job (now **nine**
  demos).
- In §5, remove H5 from the near-term list and renumber; keep Phase 3, H2 cycle 2 and H4.
- In the §4 mermaid graph, mark H5 done and show `H3 --> H5`.
- Update the **Status date** at the top to the merge date.

- [ ] **Step 2: Update `ROADMAP.md`**

Flip the H5 line to done-for-cycle-1 in the same terms. Read the file first and match its
existing granularity — it is the high-level list, so keep it short.

- [ ] **Step 3: Update `project-map.js`**

- The H5 `roadmap[]` entry → `done`, with a description matching the roadmap wording.
- Add component nodes for `cwe-storage` (`services/storage`) and `cwe-receipt`
  (`libs/receipt`), each with `status: "done"`, a `desc`, and `parts` naming what they
  contain (node HTTP surface, ledger, receipt signing / receipt type, canonical bytes,
  anti-replay).
- Update the settlement and `cwe-dapr` nodes' `desc`/`parts` to mention per-row bandwidth
  credibility, and the `CWEIdentity` node's to mention the storage-node credential.
- Set `project.updated` to the merge date.
- The header stats recompute themselves — do not hand-edit them.

- [ ] **Step 4: Verify the map renders**

Open `file:///home/roland/git/clean-web-economy/project-map.html` and confirm the new
nodes appear, the H5 roadmap entry reads as done, and the header counts moved. A JS syntax
error shows as an empty page — check the browser console if nothing renders.

- [ ] **Step 5: Run the full gate one final time**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd chain && forge test)
```

Expected: all green. This is the gate; do not proceed to merge on a red result.

- [ ] **Step 6: Commit**

```bash
git add ROADMAP.md docs/roadmap.md project-map.js
git commit -m "Roadmap + project map: H5 cycle 1 (storage + real bandwidth receipts) complete"
```

---

## Verification checklist (before merging)

- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green
- [ ] `(cd chain && forge test)` green
- [ ] All **nine** demos pass: `demo`, `hub-demo`, `ownership-demo`, `player-demo`,
      `arbitration-demo`, `antifraud-demo`, `identity-demo`, `zk-demo`, `bandwidth-demo`
- [ ] No pre-existing test's expected values were edited (the neutral-credibility
      bit-for-bit guarantee)
- [ ] No mention of any AI agent, assistant, or vendor anywhere in the diff
