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
/// malformed input (odd length, a non-hex digit, or non-ASCII/multi-byte UTF-8).
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    // Work over the raw bytes rather than `str` slicing: byte-range slices never
    // panic on a char boundary, whereas `&s[i..i+2]` does whenever a multi-byte
    // UTF-8 character straddles that offset. Attacker-controlled input (an
    // unverified signature string) must return `None` here, never panic.
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    (0..bytes.len())
        .step_by(2)
        .map(|i| {
            // Re-validate as UTF-8 before parsing so a stray multi-byte
            // character (which is not valid ASCII hex anyway) yields `None`
            // instead of `from_str_radix` ever seeing an invalid `str`.
            std::str::from_utf8(&bytes[i..i + 2])
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
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
    fn co_sign(
        receipt: Receipt,
        node: &PrivateKeySigner,
        consumer: &PrivateKeySigner,
    ) -> SignedReceipt {
        let msg = receipt.canonical_bytes().unwrap();
        SignedReceipt {
            node_sig: format!(
                "0x{}",
                hex_of(&node.sign_message_sync(&msg).unwrap().as_bytes())
            ),
            consumer_sig: format!(
                "0x{}",
                hex_of(&consumer.sign_message_sync(&msg).unwrap().as_bytes())
            ),
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

    /// A signature string containing a multi-byte UTF-8 character at an odd
    /// byte offset must not panic `decode_hex`'s byte-slicing — it must be
    /// rejected as a bad signature like any other malformed input.
    #[test]
    fn non_ascii_signature_fails_without_panic() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        // "a" (1 byte) + "€" (3 bytes) = 4 bytes total, an even byte length
        // with a multi-byte character straddling the 2-byte decode window.
        signed.node_sig = "0xa\u{20AC}".to_string();
        assert!(signed.verify().is_err());
    }

    /// Valid hex of the wrong length (64 bytes where a 65-byte `r||s||v`
    /// signature is required) is rejected, not accepted or panicked on.
    #[test]
    fn wrong_length_hex_signature_fails() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        signed.node_sig = format!("0x{}", "ab".repeat(64)); // 64 bytes, not 65
        assert!(signed.verify().is_err());
    }

    /// An odd-length hex string is rejected by the byte-length guard.
    #[test]
    fn odd_length_signature_fails() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        signed.node_sig = "0xabc".to_string(); // 3 hex chars, odd
        assert!(signed.verify().is_err());
    }

    /// A signature of the correct length (65 bytes / 130 hex chars) but with
    /// non-hex characters is rejected rather than mis-parsed.
    #[test]
    fn non_hex_characters_signature_fails() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        let mut signed = co_sign(r, &node, &consumer);
        signed.node_sig = format!("0x{}", "zz".repeat(65)); // right length, no hex digits
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
        let bundle = ReceiptBundle {
            epoch: 7,
            receipts: vec![co_sign(r, &node, &consumer)],
        };
        let json = bundle.to_json().unwrap();
        let back = ReceiptBundle::from_json(&json).unwrap();
        assert_eq!(back.epoch, 7);
        assert_eq!(back.receipts.len(), 1);
        back.receipts[0].verify().unwrap();
    }
}
