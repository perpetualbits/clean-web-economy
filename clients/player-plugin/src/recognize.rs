//! Two-tier recognition against the Discovery Hub.
//!
//! Tier 1 is authoritative: the exact `keccak256(content)` id is resolved via
//! `/resolve/content/{id}`; a hit is signed, provable ownership → direct payout.
//! Tier 2 is a cautious fallback: the perceptual fingerprint is resolved via
//! `/resolve/fingerprint/{fp}`; a hit is escrow-bound. The HTTP layer is behind
//! a [`HubTransport`] trait so recognition is unit-testable without a network.

use cwe_fingerprint::Fingerprint;
use cwe_wallet_zk::{keccak256, Bytes32};

use crate::decode::DecodedAudio;

/// Which recognition tier produced a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Exact signed-content match — pays directly.
    Signed,
    /// Perceptual-fingerprint match — escrow-bound.
    Fingerprint,
}

/// A resolved work plus how it was recognised.
#[derive(Debug, Clone)]
pub struct ResolvedWork {
    /// The on-chain work id (`0x`-hex `bytes32`).
    pub work_id: String,
    /// The work's per-minute price (policy input).
    pub price_per_min: u64,
    /// The tier that recognised it.
    pub tier: Tier,
}

/// An HTTP GET returning parsed JSON, or `None` on any miss/error. Injectable so
/// recognition can be tested offline.
pub trait HubTransport {
    /// GET `url` and parse the body as JSON; `None` on non-2xx or error.
    fn get_json(&self, url: &str) -> Option<serde_json::Value>;
}

/// The Tier 1 content id of raw bytes: `keccak256(content)` as `0x`-hex.
pub fn content_id_of(bytes: &[u8]) -> String {
    Bytes32(keccak256(bytes)).to_string()
}

/// Recognise `audio`: try the signed content id first, then the fingerprint.
/// Returns the resolved work with its tier, or `None` if nothing matched.
pub fn recognize(
    hub_url: &str,
    audio: &DecodedAudio,
    transport: &dyn HubTransport,
) -> Option<ResolvedWork> {
    let base = hub_url.trim_end_matches('/');

    // Tier 1: exact content id — authoritative.
    let cid = content_id_of(&audio.bytes);
    if let Some(v) = transport.get_json(&format!("{base}/resolve/content/{cid}")) {
        if let Some(work) = parse_work(&v, Tier::Signed) {
            return Some(work);
        }
    }

    // Tier 2: perceptual fingerprint — escrow-bound fallback. The endpoint wraps
    // the work under `candidate`, alongside a similarity score.
    let fp = Fingerprint::compute(&audio.samples, audio.sample_rate).to_string();
    if let Some(v) = transport.get_json(&format!("{base}/resolve/fingerprint/{fp}")) {
        let candidate = v.get("candidate").unwrap_or(&serde_json::Value::Null);
        if let Some(work) = parse_work(candidate, Tier::Fingerprint) {
            return Some(work);
        }
    }
    None
}

/// Parse `{ work_id, price_per_min }` from a resolver body into a `ResolvedWork`.
/// Returns `None` if the required fields are absent, so a malformed answer is a
/// miss rather than a panic.
fn parse_work(v: &serde_json::Value, tier: Tier) -> Option<ResolvedWork> {
    let work_id = v.get("work_id")?.as_str()?.to_string();
    let price_per_min = v.get("price_per_min")?.as_u64()?;
    Some(ResolvedWork {
        work_id,
        price_per_min,
        tier,
    })
}

/// A [`HubTransport`] backed by a blocking `reqwest` client.
pub struct ReqwestTransport {
    /// The shared blocking HTTP client.
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Build a transport with a default blocking client.
    pub fn new() -> ReqwestTransport {
        ReqwestTransport {
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HubTransport for ReqwestTransport {
    fn get_json(&self, url: &str) -> Option<serde_json::Value> {
        // Any transport error or non-success status is simply a miss: the caller
        // treats an unrecognised work as "nothing to account", never an error.
        let resp = self.client.get(url).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A transport backed by a fixed URL→JSON map, so tests never hit the network.
    struct MockHub(HashMap<String, serde_json::Value>);
    impl HubTransport for MockHub {
        fn get_json(&self, url: &str) -> Option<serde_json::Value> {
            self.0.get(url).cloned()
        }
    }

    fn audio() -> DecodedAudio {
        DecodedAudio {
            bytes: b"the-song".to_vec(),
            samples: vec![0.1; 200_000],
            sample_rate: 44_100,
        }
    }

    /// A signed (content) hit wins even when a fingerprint would also resolve.
    #[test]
    fn prefers_signed() {
        let a = audio();
        let cid = content_id_of(&a.bytes);
        let mut m = HashMap::new();
        m.insert(
            format!("http://h/resolve/content/{cid}"),
            serde_json::json!({ "work_id": "0xSIGNED", "price_per_min": 10 }),
        );
        // A fingerprint answer also exists, but must not be consulted.
        let fp = Fingerprint::compute(&a.samples, a.sample_rate).to_string();
        m.insert(
            format!("http://h/resolve/fingerprint/{fp}"),
            serde_json::json!({ "candidate": { "work_id": "0xFP", "price_per_min": 10 } }),
        );
        let w = recognize("http://h", &a, &MockHub(m)).unwrap();
        assert_eq!(w.work_id, "0xSIGNED");
        assert!(matches!(w.tier, Tier::Signed));
    }

    /// A content miss falls back to a fingerprint (escrow-bound) match.
    #[test]
    fn falls_back_to_fingerprint() {
        let a = audio();
        let fp = Fingerprint::compute(&a.samples, a.sample_rate).to_string();
        let mut m = HashMap::new();
        m.insert(
            format!("http://h/resolve/fingerprint/{fp}"),
            serde_json::json!({ "candidate": { "work_id": "0xFP", "price_per_min": 7 } }),
        );
        let w = recognize("http://h", &a, &MockHub(m)).unwrap();
        assert_eq!(w.work_id, "0xFP");
        assert_eq!(w.price_per_min, 7);
        assert!(matches!(w.tier, Tier::Fingerprint));
    }

    /// No content and no fingerprint match resolves to nothing.
    #[test]
    fn total_miss_is_none() {
        let a = audio();
        assert!(recognize("http://h", &a, &MockHub(HashMap::new())).is_none());
    }
}
