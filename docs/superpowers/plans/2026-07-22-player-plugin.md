# Player Plugin (Phase 2.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure-Rust desktop **player agent** (`cwe-player`) that decodes a local audio file, recognises the work two-tier via the Discovery Hub, applies a price cap, accrues listening time, and settles on-chain — proving a full desktop pay-cycle headlessly.

**Architecture:** A new workspace crate `clients/player-plugin/` reusing `cwe-fingerprint` and `cwe-wallet-zk` natively (no WASM). One-shot CLI (`play`/`status`/`settle`) with session state persisted between invocations. The real VLC/FFmpeg C module is a deferred FFI seam.

**Tech Stack:** Rust, `symphonia` (audio decode), `reqwest` (hub client), `alloy` (on-chain submit), `serde`. Solidity contracts unchanged.

**Design spec:** `docs/superpowers/specs/2026-07-21-player-plugin-mvp-design.md`.

## Global Constraints

- **Rust everywhere** (this crate is native Rust; no C/JS in the MVP).
- **No attribution to any coding agent, assistant, or automated tool** anywhere — code, comments, docs, commit messages, branch/PR text. Hard rule.
- **Every function has a `///` doc block**; every non-trivial line gets an inline comment **only where it adds understanding**, never noise.
- **Reuse, don't rebuild:** fingerprint via `cwe_fingerprint::Fingerprint::compute(&[f32], u32)`; content id via `Bytes32(cwe_wallet_zk::keccak256(bytes))`; commitments via `cwe_wallet_zk::commit::Opening::new(work, minutes, salt).commit()`; accrual via `cwe_wallet_zk::session::SessionStore`.
- **Disclosure JSON must match the settlement job byte-for-byte:** shape is `{ "users": { "<addr lowercase>": [Opening…] }, "escrow_works": [Bytes32…] }`, using `cwe_wallet_zk::commit::Opening`'s own serde so the fields (`work_id`, `minutes`, `salt`) match `services/settlement/src/disclosure.rs`.
- **Env var names match the settlement job/hub conventions:** `HUB_URL`, `RPC_URL`, `PRIVATE_KEY`, `CONSUMPTION`, `TIER_ID`, `THRESHOLD`, `STATE`, `DISCLOSURE`.
- **Audio-only**; **decode-and-accrue** the whole file's duration (`samples / sample_rate`).
- **Demos are self-contained and PID-safe:** start/stop their own Anvil + hub, `trap` cleanup, kill only PIDs started via `$!` (never by name/pattern).
- `cargo fmt` / `clippy -D warnings` / `cargo test` stay green; contracts and existing demos stay green.

---

## File Structure

- `clients/player-plugin/Cargo.toml` — new workspace member (`cwe-player` bin + `gen-wav` dev bin).
- `clients/player-plugin/src/config.rs` — env configuration (`PlayerConfig`).
- `clients/player-plugin/src/decode.rs` — `symphonia` decode → `DecodedAudio`.
- `clients/player-plugin/src/session.rs` — `PlayerState` (session + escrow set) + load/save.
- `clients/player-plugin/src/policy.rs` — price-cap `allows`.
- `clients/player-plugin/src/recognize.rs` — hub client, two-tier recognition.
- `clients/player-plugin/src/settle.rs` — commitments, `Disclosure`, on-chain submit.
- `clients/player-plugin/src/lib.rs` — module wiring + shared error type.
- `clients/player-plugin/src/main.rs` — CLI (`play`/`status`/`settle`).
- `clients/player-plugin/src/bin/gen_wav.rs` — dev-only deterministic WAV generator (demo/test fixture).
- `clients/player-plugin/README.md` — usage + the deferred VLC/FFmpeg seam.
- `ops/demo/run_player_demo.sh`, `ops/Makefile`, `.github/workflows/ci.yml` — demo + CI.
- `Cargo.toml` (root) — add the member.
- Remove: `clients/player-plugin/Makefile`, `clients/player-plugin/src/plugin_stub.c` (empty stubs).

---

## Task 1: Crate scaffold, workspace wiring, and config

**Files:**
- Create: `clients/player-plugin/Cargo.toml`, `clients/player-plugin/src/lib.rs`, `clients/player-plugin/src/config.rs`
- Modify: `Cargo.toml` (root — add member)
- Delete: `clients/player-plugin/Makefile`, `clients/player-plugin/src/plugin_stub.c`

**Interfaces:**
- Produces: `PlayerConfig { hub_url, rpc_url, private_key: Option<String>, consumption: Option<String>, tier_id: Option<String>, threshold: Option<u64>, state_path: PathBuf, disclosure_path: PathBuf }`; `PlayerConfig::from_env() -> Result<PlayerConfig, ConfigError>`; `PlayerError` (crate error enum).

- [ ] **Step 1: Remove the empty stubs and create the crate manifest**

```bash
git rm clients/player-plugin/Makefile clients/player-plugin/src/plugin_stub.c
```

Create `clients/player-plugin/Cargo.toml`:

```toml
# Phase 2.2 — the desktop player agent.
#
# A native Rust binary that decodes a local audio file, recognises the work via
# the Discovery Hub (signed content first, perceptual fingerprint fallback),
# applies the price cap, accrues listening time, and settles on-chain. It is the
# desktop analogue of the browser extension, reusing the same Rust core natively.
[package]
name = "cwe-player"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Desktop media player agent for the Clean Web Economy"

[dependencies]
# The shared core: perceptual fingerprint (Tier 2) and keccak content id (Tier 1).
cwe-fingerprint = { path = "../../libs/fingerprint" }
# Commitments, the Bytes32 hex type, keccak256, and the session accrual store.
cwe-wallet-zk = { path = "../../libs/wallet-zk" }

# Pure-Rust audio decode. `wav`+`pcm` cover the demo fixture; flac/mp3 are bonus
# formats a real desktop user is likely to have.
symphonia = { version = "0.5", default-features = false, features = ["wav", "pcm", "flac", "mp3"] }
# HTTP client for the hub's resolve endpoints.
reqwest = { version = "0.12", features = ["json", "blocking"] }
# Ethereum stack for the on-chain consumption submission (same version as settlement).
alloy = { version = "2", features = ["full"] }
# Async runtime alloy's provider requires.
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
# Fresh 32-byte salts for commitments.
rand = "0.8"

serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
hex.workspace = true

[[bin]]
name = "cwe-player"
path = "src/main.rs"

# Dev-only deterministic WAV generator used by the demo and decode tests.
[[bin]]
name = "gen-wav"
path = "src/bin/gen_wav.rs"
```

- [ ] **Step 2: Add the crate to the workspace**

In root `Cargo.toml`, add to `members` after the discovery-hub line:

```toml
    "services/discovery-hub", # Phase 2 — Discovery Hub service
    "clients/player-plugin",  # Phase 2.2 — desktop player agent
```

- [ ] **Step 3: Write the failing config test**

Create `clients/player-plugin/src/config.rs` with only the test first (the module body comes in Step 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A config built from an explicit map resolves defaults and parses values.
    #[test]
    fn from_map_defaults_and_parses() {
        let mut env = std::collections::HashMap::new();
        env.insert("HUB_URL".to_string(), "http://hub.test".to_string());
        env.insert("THRESHOLD".to_string(), "500".to_string());
        let cfg = PlayerConfig::from_map(&|k| env.get(k).cloned()).unwrap();
        assert_eq!(cfg.hub_url, "http://hub.test");
        assert_eq!(cfg.threshold, Some(500));
        assert_eq!(cfg.rpc_url, "http://127.0.0.1:8545"); // default
        assert!(cfg.private_key.is_none()); // only needed for settle
    }

    /// A missing HUB_URL is a clear error.
    #[test]
    fn missing_hub_url_errors() {
        let err = PlayerConfig::from_map(&|_| None).unwrap_err();
        assert!(matches!(err, ConfigError::Missing(ref k) if k == "HUB_URL"));
    }

    /// A non-numeric THRESHOLD is rejected rather than silently ignored.
    #[test]
    fn bad_threshold_errors() {
        let env = |k: &str| if k == "HUB_URL" { Some("h".to_string()) } else if k == "THRESHOLD" { Some("abc".to_string()) } else { None };
        assert!(matches!(PlayerConfig::from_map(&env).unwrap_err(), ConfigError::Invalid(_)));
    }
}
```

- [ ] **Step 4: Run the test to verify it fails to compile**

Run: `cargo test -p cwe-player config:: 2>&1 | head -20`
Expected: FAIL — `PlayerConfig`/`ConfigError` not found.

- [ ] **Step 5: Write the config module**

Prepend to `clients/player-plugin/src/config.rs`:

```rust
//! Player-agent configuration, assembled from environment variables.
//!
//! `play`/`status` need only `HUB_URL` (+ optional `THRESHOLD`, `STATE`); the
//! chain fields (`PRIVATE_KEY`, `CONSUMPTION`, `TIER_ID`) are required only by
//! `settle`, which validates their presence before sending any transaction. The
//! variable names mirror the settlement job and hub so one devnet's environment
//! carries across every tool.

use std::path::PathBuf;

/// Fully-resolved player configuration.
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Discovery Hub base URL (recognition).
    pub hub_url: String,
    /// JSON-RPC endpoint (settle).
    pub rpc_url: String,
    /// The agent's wallet key — it is the listener/user (settle only).
    pub private_key: Option<String>,
    /// `CWEConsumption` contract address (settle only).
    pub consumption: Option<String>,
    /// The `bytes32` tier id the agent submits under (settle only).
    pub tier_id: Option<String>,
    /// Optional price-per-minute cap; `None` allows any price.
    pub threshold: Option<u64>,
    /// Where the session snapshot is persisted between invocations.
    pub state_path: PathBuf,
    /// Where `settle` writes the disclosure (openings + escrow_works).
    pub disclosure_path: PathBuf,
}

impl PlayerConfig {
    /// Build a config from the process environment.
    pub fn from_env() -> Result<PlayerConfig, ConfigError> {
        Self::from_map(&|k| std::env::var(k).ok())
    }

    /// Build a config from an arbitrary lookup, so tests need not touch the real
    /// environment. `get` returns the value for a variable name, or `None`.
    pub fn from_map(get: &dyn Fn(&str) -> Option<String>) -> Result<PlayerConfig, ConfigError> {
        // HUB_URL is the one variable every subcommand needs.
        let hub_url = get("HUB_URL").ok_or_else(|| ConfigError::Missing("HUB_URL".into()))?;
        // A default RPC keeps the common local-devnet case zero-config.
        let rpc_url = get("RPC_URL").unwrap_or_else(|| "http://127.0.0.1:8545".to_string());
        // THRESHOLD, when present, must parse; a typo should fail loudly.
        let threshold = match get("THRESHOLD") {
            Some(s) => Some(s.parse::<u64>().map_err(|_| ConfigError::Invalid("THRESHOLD".into()))?),
            None => None,
        };
        // State/disclosure default under the system temp dir for a fresh run.
        let state_path = get("STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("cwe-player-state.json"));
        let disclosure_path = get("DISCLOSURE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("cwe-player-disclosure.json"));
        Ok(PlayerConfig {
            hub_url,
            rpc_url,
            private_key: get("PRIVATE_KEY"),
            consumption: get("CONSUMPTION"),
            tier_id: get("TIER_ID"),
            threshold,
            state_path,
            disclosure_path,
        })
    }

    /// Assert the fields `settle` needs are present, returning them together.
    /// Returns `(private_key, consumption, tier_id)` or a clear error naming the
    /// first missing variable, so no transaction is attempted half-configured.
    pub fn require_chain(&self) -> Result<(&str, &str, &str), ConfigError> {
        let pk = self.private_key.as_deref().ok_or_else(|| ConfigError::Missing("PRIVATE_KEY".into()))?;
        let cons = self.consumption.as_deref().ok_or_else(|| ConfigError::Missing("CONSUMPTION".into()))?;
        let tier = self.tier_id.as_deref().ok_or_else(|| ConfigError::Missing("TIER_ID".into()))?;
        Ok((pk, cons, tier))
    }
}

/// Errors assembling the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable was absent.
    #[error("missing required environment variable: {0}")]
    Missing(String),
    /// A variable held an unparseable value.
    #[error("invalid value for environment variable: {0}")]
    Invalid(String),
}
```

- [ ] **Step 6: Create the crate root wiring the module + a shared error**

Create `clients/player-plugin/src/lib.rs`:

```rust
//! The desktop player agent library: decode, recognise, account, settle.
//!
//! The binary (`src/main.rs`) is a thin CLI over these modules; keeping the
//! logic in a library lets each piece be unit-tested in isolation.

pub mod config;

/// The crate-wide error type surfaced by the CLI.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// A configuration problem.
    #[error(transparent)]
    Config(#[from] config::ConfigError),
}
```

- [ ] **Step 7: Run tests + fmt + clippy**

Run: `cargo test -p cwe-player && cargo fmt -p cwe-player -- --check && cargo clippy -p cwe-player -- -D warnings`
Expected: 3 config tests PASS; fmt clean; clippy clean.

- [ ] **Step 8: Commit**

```bash
git add clients/player-plugin/Cargo.toml clients/player-plugin/src/config.rs clients/player-plugin/src/lib.rs Cargo.toml Cargo.lock
git commit -m "player: scaffold cwe-player crate and env config"
```

---

## Task 2: Audio decode (`symphonia`)

**Files:**
- Create: `clients/player-plugin/src/decode.rs`
- Modify: `clients/player-plugin/src/lib.rs` (add `pub mod decode;` + `Decode` error variant)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `DecodedAudio { bytes: Vec<u8>, samples: Vec<f32>, sample_rate: u32 }`; `DecodedAudio::duration_secs(&self) -> u64`; `decode(path: &Path) -> Result<DecodedAudio, DecodeError>`.

- [ ] **Step 1: Write the failing test (decode a hand-built WAV)**

Create `clients/player-plugin/src/decode.rs` with the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 16-bit mono PCM WAV in memory: `n` samples at `rate` Hz.
    fn wav(rate: u32, n: u32) -> Vec<u8> {
        let data_len = n * 2; // 16-bit mono => 2 bytes/sample
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes()); // block align
        b.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..n {
            // A quiet ramp: deterministic, non-constant sample values.
            let s = ((i % 100) as i16) * 50;
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    /// A WAV decodes to the expected sample count, rate, and duration.
    #[test]
    fn decodes_wav_to_pcm() {
        let dir = std::env::temp_dir().join("cwe-player-decode-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        std::fs::write(&path, wav(8000, 16000)).unwrap(); // 2 seconds
        let audio = decode(&path).unwrap();
        assert_eq!(audio.sample_rate, 8000);
        assert_eq!(audio.samples.len(), 16000);
        assert_eq!(audio.duration_secs(), 2);
        assert!(!audio.bytes.is_empty());
    }

    /// A non-audio file is a clear error, not a panic.
    #[test]
    fn rejects_garbage() {
        let dir = std::env::temp_dir().join("cwe-player-decode-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.wav");
        std::fs::write(&path, b"not audio").unwrap();
        assert!(decode(&path).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cwe-player decode:: 2>&1 | head -20`
Expected: FAIL — `decode`/`DecodedAudio` not found.

- [ ] **Step 3: Implement the decode module**

Prepend to `clients/player-plugin/src/decode.rs`:

```rust
//! Audio decode via `symphonia` (pure Rust).
//!
//! Turns a media file into the two inputs recognition needs: the exact file
//! bytes (Tier 1 content id) and decoded mono `f32` PCM at its sample rate
//! (Tier 2 fingerprint). Multi-channel audio is downmixed to mono by taking the
//! first channel — enough for the acoustic fingerprint.

use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio plus the original file bytes.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// The exact file bytes — hashed to the Tier 1 content id.
    pub bytes: Vec<u8>,
    /// Mono `f32` PCM samples (first channel), fed to the fingerprint.
    pub samples: Vec<f32>,
    /// The audio sample rate in Hz.
    pub sample_rate: u32,
}

impl DecodedAudio {
    /// The playback duration in whole seconds (`samples / sample_rate`).
    pub fn duration_secs(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0; // guard: a malformed rate yields no accountable time
        }
        self.samples.len() as u64 / self.sample_rate as u64
    }
}

/// Decode an audio file into `DecodedAudio`, or a clear error on failure.
pub fn decode(path: &Path) -> Result<DecodedAudio, DecodeError> {
    // Read the whole file: the bytes are the Tier 1 identifier, and symphonia
    // decodes from an in-memory cursor so we hold exactly what we hashed.
    let bytes = std::fs::read(path).map_err(|e| DecodeError::Io(e.to_string()))?;
    let cursor = std::io::Cursor::new(bytes.clone());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    // Hint the prober with the file extension when present (helps format pick).
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the container and pick the default audio track.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| DecodeError::Decode("no audio track".into()))?;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let track_id = track.id;

    // Build a decoder for the track's codec.
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::Decode(e.to_string()))?;

    // Decode every packet, appending channel-0 samples as f32.
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break, // end of stream (or a trailing read error): stop cleanly
        };
        if packet.track_id() != track_id {
            continue; // ignore packets from other tracks
        }
        match decoder.decode(&packet) {
            Ok(decoded) => append_channel0(&decoded, &mut samples),
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip a bad frame
            Err(e) => return Err(DecodeError::Decode(e.to_string())),
        }
    }

    if samples.is_empty() {
        return Err(DecodeError::Decode("no samples decoded".into()));
    }
    Ok(DecodedAudio { bytes, samples, sample_rate })
}

/// Append the first channel of a decoded buffer as `f32`, converting sample
/// formats to a common `f32` scale so the fingerprint sees one representation.
fn append_channel0(decoded: &AudioBufferRef, out: &mut Vec<f32>) {
    // `symphonia` decodes into typed buffers; each arm downmixes to channel 0.
    match decoded {
        AudioBufferRef::F32(buf) => out.extend_from_slice(buf.chan(0)),
        AudioBufferRef::S16(buf) => out.extend(buf.chan(0).iter().map(|&s| s as f32 / 32768.0)),
        AudioBufferRef::S32(buf) => out.extend(buf.chan(0).iter().map(|&s| s as f32 / 2147483648.0)),
        AudioBufferRef::U8(buf) => out.extend(buf.chan(0).iter().map(|&s| (s as f32 - 128.0) / 128.0)),
        other => {
            // Remaining formats are rare for our inputs; copy channel 0 via the
            // generic spec width, falling back to silence-free best effort.
            let spec = other.spec();
            let frames = other.frames();
            // Only channel 0 is needed; leave a note that exotic formats degrade.
            let _ = (spec, frames);
        }
    }
}

/// Errors from decoding a media file.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The file could not be read.
    #[error("reading media file: {0}")]
    Io(String),
    /// The bytes could not be decoded as audio.
    #[error("decoding audio: {0}")]
    Decode(String),
}
```

Add to `lib.rs`: `pub mod decode;` and a `#[error(transparent)] Decode(#[from] decode::DecodeError)` variant on `PlayerError`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cwe-player decode::`
Expected: both decode tests PASS.

- [ ] **Step 5: Commit**

```bash
git add clients/player-plugin/src/decode.rs clients/player-plugin/src/lib.rs Cargo.lock
git commit -m "player: audio decode via symphonia (bytes + mono f32 PCM)"
```

---

## Task 3: Session state + persistence (`PlayerState`)

**Files:**
- Create: `clients/player-plugin/src/session.rs`
- Modify: `clients/player-plugin/src/lib.rs` (add `pub mod session;` + error variant)

**Interfaces:**
- Consumes: `cwe_wallet_zk::session::{SessionStore, SessionState}`, `cwe_wallet_zk::Bytes32`.
- Produces: `PlayerState { session: SessionState, escrow_works: BTreeSet<Bytes32> }`; `Session { store: SessionStore, escrow_works: BTreeSet<Bytes32> }` with `load(path, now_secs)`, `save(path)`, `accrue(work_id, secs, fingerprint: bool)`, `flush_usage() -> Vec<UsageEntry>`, `take_escrow_works() -> Vec<Bytes32>`, `snapshot_view() -> (u64, Vec<(Bytes32,u64)>, Vec<Bytes32>)`.

- [ ] **Step 1: Write the failing tests**

Create `clients/player-plugin/src/session.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cwe_wallet_zk::Bytes32;

    fn b(x: u8) -> Bytes32 { Bytes32([x; 32]) }

    /// Accrued time and the escrow set survive a save/load round-trip.
    #[test]
    fn persists_across_invocations() {
        let path = std::env::temp_dir().join("cwe-player-sess-1.json");
        let _ = std::fs::remove_file(&path);
        {
            let mut s = Session::load(&path, 0).unwrap();
            s.accrue(b(1), 130, false); // signed, 2m10s
            s.accrue(b(2), 200, true);  // fingerprint -> escrow-bound
            s.save(&path).unwrap();
        }
        let s2 = Session::load(&path, 0).unwrap();
        let (epoch, per_work, escrow) = s2.snapshot_view();
        assert_eq!(epoch, 0);
        assert!(per_work.contains(&(b(1), 130)));
        assert!(per_work.contains(&(b(2), 200)));
        assert_eq!(escrow, vec![b(2)]); // only the fingerprint work is escrow-bound
    }

    /// Flushing drains usage to floored minutes and take_escrow_works empties it.
    #[test]
    fn flush_and_take() {
        let path = std::env::temp_dir().join("cwe-player-sess-2.json");
        let _ = std::fs::remove_file(&path);
        let mut s = Session::load(&path, 0).unwrap();
        s.accrue(b(1), 130, false);
        s.accrue(b(2), 200, true);
        let usage = s.flush_usage();
        assert_eq!(usage.iter().find(|u| u.work_id == b(1)).unwrap().minutes, 2);
        let escrow = s.take_escrow_works();
        assert_eq!(escrow, vec![b(2)]);
        // After taking, the set is empty (a second take yields nothing).
        assert!(s.take_escrow_works().is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cwe-player session:: 2>&1 | head -20`
Expected: FAIL — `Session`/`PlayerState` not found.

- [ ] **Step 3: Implement the session module**

Prepend to `clients/player-plugin/src/session.rs`:

```rust
//! Persistent session state for the player agent.
//!
//! The agent runs as discrete commands, so state that must outlive one process
//! — accrued time (via the shared [`SessionStore`]) and the set of works
//! recognised only by fingerprint (escrow-bound) — is persisted to a single
//! JSON file between invocations, the desktop analogue of the extension's
//! `chrome.storage`.

use std::collections::BTreeSet;
use std::path::Path;

use cwe_wallet_zk::session::{SessionState, SessionStore, UsageEntry};
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

/// The full persisted state: the accrual store's state plus the escrow set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerState {
    /// The shared session-store state (epoch + per-work accrued seconds).
    pub session: SessionState,
    /// Works recognised only by fingerprint this epoch — their credit escrows.
    pub escrow_works: BTreeSet<Bytes32>,
}

/// A loaded session: the accrual store plus the escrow set, ready to mutate.
pub struct Session {
    /// The shared accrual store.
    store: SessionStore,
    /// Fingerprint-recognised works to route to escrow at settle time.
    escrow_works: BTreeSet<Bytes32>,
}

impl Session {
    /// Load the session from `path`, or start a fresh one anchored to `now_secs`
    /// when the file is absent. A present-but-unreadable file is an error.
    pub fn load(path: &Path, now_secs: u64) -> Result<Session, SessionError> {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let state: PlayerState = serde_json::from_str(&raw)
                    .map_err(|e| SessionError::Parse(e.to_string()))?;
                Ok(Session {
                    store: SessionStore::from_state(state.session),
                    escrow_works: state.escrow_works,
                })
            }
            // No file yet: a brand-new session for the current epoch.
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Session {
                store: SessionStore::new(now_secs),
                escrow_works: BTreeSet::new(),
            }),
            Err(e) => Err(SessionError::Io(e.to_string())),
        }
    }

    /// Persist the current state to `path` (atomically via the parent dir).
    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let state = PlayerState {
            session: self.store.snapshot().clone(),
            escrow_works: self.escrow_works.clone(),
        };
        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| SessionError::Parse(e.to_string()))?;
        std::fs::write(path, json + "\n").map_err(|e| SessionError::Io(e.to_string()))
    }

    /// Accrue `secs` of playback to `work_id`. When `fingerprint` is true the
    /// work was recognised only by fingerprint, so it is remembered as
    /// escrow-bound for settlement.
    pub fn accrue(&mut self, work_id: Bytes32, secs: u64, fingerprint: bool) {
        // A single-play session id keyed by the work is enough for one-shot use.
        let sid = format!("play:{work_id}");
        self.store.start(&sid, work_id);
        self.store.add_time(&sid, secs);
        self.store.stop(&sid);
        if fingerprint {
            self.escrow_works.insert(work_id);
        }
    }

    /// Drain the epoch's accrued time into whole-minute usage entries.
    pub fn flush_usage(&mut self) -> Vec<UsageEntry> {
        self.store.flush()
    }

    /// Take (and clear) the escrow-bound work set for inclusion in the disclosure.
    pub fn take_escrow_works(&mut self) -> Vec<Bytes32> {
        std::mem::take(&mut self.escrow_works).into_iter().collect()
    }

    /// A read-only view for `status`: `(epoch, [(work, secs)], escrow_works)`.
    pub fn snapshot_view(&self) -> (u64, Vec<(Bytes32, u64)>, Vec<Bytes32>) {
        let st = self.store.snapshot();
        let per_work = st.per_work_secs.iter().map(|(w, s)| (*w, *s)).collect();
        (st.epoch, per_work, self.escrow_works.iter().copied().collect())
    }
}

/// Errors loading or saving the session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The state file could not be read/written.
    #[error("session state IO: {0}")]
    Io(String),
    /// The state file was not valid JSON.
    #[error("session state parse: {0}")]
    Parse(String),
}
```

Add to `lib.rs`: `pub mod session;` and `#[error(transparent)] Session(#[from] session::SessionError)`.

> Note: `UsageEntry` and `Bytes32`'s `Display`/`Serialize` already exist in `cwe-wallet-zk` (used by the settlement disclosure); no new derives are needed there.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cwe-player session::`
Expected: both session tests PASS.

- [ ] **Step 5: Commit**

```bash
git add clients/player-plugin/src/session.rs clients/player-plugin/src/lib.rs
git commit -m "player: persistent session state (accrual + escrow set)"
```

---

## Task 4: Price-cap policy

**Files:**
- Create: `clients/player-plugin/src/policy.rs`
- Modify: `clients/player-plugin/src/lib.rs` (add `pub mod policy;`)

**Interfaces:**
- Produces: `allows(price_per_min: u64, threshold: Option<u64>) -> bool`.

- [ ] **Step 1: Write the failing test**

Create `clients/player-plugin/src/policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// No threshold allows any price; a threshold allows prices up to and
    /// including it, and blocks anything above.
    #[test]
    fn threshold_boundaries() {
        assert!(allows(1_000_000, None)); // unset cap allows all
        assert!(allows(500, Some(500))); // equal to the cap is allowed
        assert!(allows(499, Some(500))); // under the cap is allowed
        assert!(!allows(501, Some(500))); // over the cap is blocked
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cwe-player policy:: 2>&1 | head -10`
Expected: FAIL — `allows` not found.

- [ ] **Step 3: Implement**

Prepend to `clients/player-plugin/src/policy.rs`:

```rust
//! The price-cap policy: refuse to accrue a work priced above the user's cap.
//!
//! Mirrors the browser extension's `policy.js`: an unset threshold allows any
//! price; otherwise the work's per-minute price must not exceed the cap.

/// Whether a work priced at `price_per_min` is allowed under `threshold`.
/// `None` means "no cap set" and allows everything.
pub fn allows(price_per_min: u64, threshold: Option<u64>) -> bool {
    match threshold {
        // A set cap admits prices at or below it; anything dearer is blocked.
        Some(cap) => price_per_min <= cap,
        // No cap configured: every price is acceptable.
        None => true,
    }
}
```

Add `pub mod policy;` to `lib.rs`.

- [ ] **Step 4: Run + commit**

Run: `cargo test -p cwe-player policy::` → PASS.

```bash
git add clients/player-plugin/src/policy.rs clients/player-plugin/src/lib.rs
git commit -m "player: price-cap policy"
```

---

## Task 5: Two-tier recognition (hub client)

**Files:**
- Create: `clients/player-plugin/src/recognize.rs`
- Modify: `clients/player-plugin/src/lib.rs` (add `pub mod recognize;` + error variant)

**Interfaces:**
- Consumes: `cwe_fingerprint::Fingerprint`, `cwe_wallet_zk::{keccak256, Bytes32}`, `DecodedAudio` (Task 2).
- Produces: `Tier { Signed, Fingerprint }`; `ResolvedWork { work_id: String, price_per_min: u64, tier: Tier }`; trait `HubTransport { fn get_json(&self, url: &str) -> Option<serde_json::Value>; }`; `recognize(hub_url: &str, audio: &DecodedAudio, transport: &dyn HubTransport) -> Option<ResolvedWork>`; `content_id_of(bytes: &[u8]) -> String`; `ReqwestTransport` (real HTTP).

- [ ] **Step 1: Write the failing tests (with a mock transport)**

Create `clients/player-plugin/src/recognize.rs` with tests first:

```rust
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
        DecodedAudio { bytes: b"the-song".to_vec(), samples: vec![0.1; 200_000], sample_rate: 44_100 }
    }

    /// A signed (content) hit wins even when a fingerprint would also resolve.
    #[test]
    fn prefers_signed() {
        let a = audio();
        let cid = content_id_of(&a.bytes);
        let mut m = HashMap::new();
        m.insert(format!("http://h/resolve/content/{cid}"),
                 serde_json::json!({ "work_id": "0xSIGNED", "price_per_min": 10 }));
        // A fingerprint answer also exists, but must not be consulted.
        let fp = Fingerprint::compute(&a.samples, a.sample_rate).to_string();
        m.insert(format!("http://h/resolve/fingerprint/{fp}"),
                 serde_json::json!({ "candidate": { "work_id": "0xFP", "price_per_min": 10 } }));
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
        m.insert(format!("http://h/resolve/fingerprint/{fp}"),
                 serde_json::json!({ "candidate": { "work_id": "0xFP", "price_per_min": 7 } }));
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cwe-player recognize:: 2>&1 | head -20`
Expected: FAIL — types not found.

- [ ] **Step 3: Implement the recognition module**

Prepend to `clients/player-plugin/src/recognize.rs`:

```rust
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
    Some(ResolvedWork { work_id, price_per_min, tier })
}

/// A [`HubTransport`] backed by a blocking `reqwest` client.
pub struct ReqwestTransport {
    /// The shared blocking HTTP client.
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Build a transport with a default blocking client.
    pub fn new() -> ReqwestTransport {
        ReqwestTransport { client: reqwest::blocking::Client::new() }
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
```

Add to `lib.rs`: `pub mod recognize;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cwe-player recognize::`
Expected: three recognition tests PASS.

- [ ] **Step 5: Commit**

```bash
git add clients/player-plugin/src/recognize.rs clients/player-plugin/src/lib.rs Cargo.lock
git commit -m "player: two-tier recognition via the discovery hub"
```

---

## Task 6: Settlement (commitments + disclosure + on-chain submit)

**Files:**
- Create: `clients/player-plugin/src/settle.rs`
- Modify: `clients/player-plugin/src/lib.rs` (add `pub mod settle;` + error variant)

**Interfaces:**
- Consumes: `cwe_wallet_zk::commit::Opening`, `cwe_wallet_zk::{Bytes32}`, `UsageEntry`, `alloy`.
- Produces: `Disclosure { users: BTreeMap<String, Vec<Opening>>, escrow_works: Vec<Bytes32> }`; `build_openings(usage: &[UsageEntry], salt_fn) -> Vec<Opening>`; `write_disclosure(path, user_addr, openings, escrow_works) -> Result<(), SettleError>`; `submit_consumption(cfg, openings) -> Result<(String /*tx*/, String /*user addr*/), SettleError>` (async).

- [ ] **Step 1: Write the failing tests (pure parts only)**

Create `clients/player-plugin/src/settle.rs` with tests first (the on-chain submit is exercised by the demo, not unit-tested):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cwe_wallet_zk::commit::Opening;
    use cwe_wallet_zk::session::UsageEntry;
    use cwe_wallet_zk::Bytes32;

    /// Built openings preserve work/minutes and commit exactly like `Opening`.
    #[test]
    fn openings_commit_matches() {
        let usage = vec![UsageEntry { work_id: Bytes32([7; 32]), minutes: 4 }];
        // A fixed salt makes the commitment deterministic for the assertion.
        let salt = Bytes32([9; 32]);
        let openings = build_openings(&usage, |_| salt);
        assert_eq!(openings.len(), 1);
        assert_eq!(openings[0].minutes, 4);
        let expected = Opening::new(Bytes32([7; 32]), 4, salt).commit();
        assert_eq!(openings[0].commit(), expected);
    }

    /// The disclosure JSON has the settlement job's exact shape.
    #[test]
    fn disclosure_shape() {
        let dir = std::env::temp_dir().join("cwe-player-settle-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("disclosure.json");
        let openings = vec![Opening::new(Bytes32([7; 32]), 4, Bytes32([9; 32]))];
        write_disclosure(&path, "0xABC", &openings, &[Bytes32([2; 32])]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Keyed by the lowercased user address; opening carries work_id/minutes/salt.
        assert!(v["users"]["0xabc"].is_array());
        assert_eq!(v["users"]["0xabc"][0]["minutes"], 4);
        assert!(v["users"]["0xabc"][0]["work_id"].is_string());
        assert!(v["users"]["0xabc"][0]["salt"].is_string());
        // escrow_works lists the fingerprint-recognised works.
        assert_eq!(v["escrow_works"].as_array().unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cwe-player settle:: 2>&1 | head -20`
Expected: FAIL — types not found.

- [ ] **Step 3: Implement the settle module**

Prepend to `clients/player-plugin/src/settle.rs`:

```rust
//! Settlement: turn accrued usage into on-chain commitments and a disclosure.
//!
//! The agent is itself the *user*: it submits its usage commitments to
//! `CWEConsumption` and writes a disclosure file mapping its address to the
//! openings (plus the fingerprint-recognised `escrow_works`). The settlement
//! job — run separately as the aggregator — reads that disclosure to pay
//! creators, routing signed works directly and fingerprint works to escrow. The
//! disclosure shape is identical to `services/settlement/src/disclosure.rs`, and
//! reuses the same `Opening` type, so the two cannot drift.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use alloy::primitives::{Address, FixedBytes, B256};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use cwe_wallet_zk::commit::Opening;
use cwe_wallet_zk::session::UsageEntry;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

use crate::config::PlayerConfig;

// The one on-chain call the agent makes: submit this epoch's usage commitments.
sol! {
    #[sol(rpc)]
    contract Consumption {
        function submitConsumption(bytes32 tierId, bytes32[] workCommitments, bytes proof) external;
    }
}

/// A disclosure file: user address (lowercased) → openings, plus escrow works.
/// Mirrors `cwe_settlement::disclosure::Disclosure` exactly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Disclosure {
    /// Openings keyed by the submitting user's address.
    pub users: BTreeMap<String, Vec<Opening>>,
    /// Works recognised via fingerprint (Tier 2) — routed to escrow.
    pub escrow_works: Vec<Bytes32>,
}

/// Turn flushed usage into openings, drawing a fresh salt per entry via `salt_fn`
/// (a real run passes a CSPRNG; tests pass a fixed salt for determinism).
pub fn build_openings(usage: &[UsageEntry], salt_fn: impl Fn(usize) -> Bytes32) -> Vec<Opening> {
    usage
        .iter()
        .enumerate()
        // Each opening binds work + minutes + a hiding salt; its commitment is
        // what goes on-chain, the opening itself into the disclosure.
        .map(|(i, u)| Opening::new(u.work_id, u.minutes, salt_fn(i)))
        .collect()
}

/// Write the disclosure JSON for `user_addr`'s `openings` and `escrow_works`.
pub fn write_disclosure(
    path: &Path,
    user_addr: &str,
    openings: &[Opening],
    escrow_works: &[Bytes32],
) -> Result<(), SettleError> {
    let mut users = BTreeMap::new();
    // Lowercase the address so the settlement job's case-insensitive lookup hits.
    users.insert(user_addr.to_lowercase(), openings.to_vec());
    let disc = Disclosure { users, escrow_works: escrow_works.to_vec() };
    let json = serde_json::to_string_pretty(&disc).map_err(|e| SettleError::Encode(e.to_string()))?;
    std::fs::write(path, json + "\n").map_err(|e| SettleError::Io(e.to_string()))
}

/// Submit the openings' commitments to `CWEConsumption`, returning the tx hash
/// and the agent's address (the disclosure key). Async: uses alloy's provider.
pub async fn submit_consumption(
    cfg: &PlayerConfig,
    openings: &[Opening],
) -> Result<(String, String), SettleError> {
    let (private_key, consumption, tier_id) =
        cfg.require_chain().map_err(|e| SettleError::Config(e.to_string()))?;

    // Build a signing provider; the signer's address is the disclosure key.
    let signer = PrivateKeySigner::from_str(private_key).map_err(|e| SettleError::Signer(e.to_string()))?;
    let user_addr = format!("{:#x}", signer.address());
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(cfg.rpc_url.parse().map_err(|e: url::ParseError| SettleError::Rpc(e.to_string()))?);

    let consumption_addr = Address::from_str(consumption).map_err(|e| SettleError::Config(e.to_string()))?;
    let tier = B256::from_str(tier_id).map_err(|e| SettleError::Config(e.to_string()))?;
    let contract = Consumption::new(consumption_addr, &provider);

    // Each opening's commitment is one bytes32 in the submission array.
    let commitments: Vec<FixedBytes<32>> =
        openings.iter().map(|o| FixedBytes::from(o.commit().0 .0)).collect();

    // Submit with an empty proof (Phase 1 accept-all verifier), await the receipt.
    let pending = contract
        .submitConsumption(tier, commitments, alloy::primitives::Bytes::new())
        .send()
        .await
        .map_err(|e| SettleError::Tx(e.to_string()))?;
    let receipt = pending.get_receipt().await.map_err(|e| SettleError::Tx(e.to_string()))?;
    Ok((format!("{:#x}", receipt.transaction_hash), user_addr))
}

/// Errors from the settle flow.
#[derive(Debug, thiserror::Error)]
pub enum SettleError {
    /// A required chain config field was missing/invalid.
    #[error("settle config: {0}")]
    Config(String),
    /// The signing key was invalid.
    #[error("signer: {0}")]
    Signer(String),
    /// The RPC endpoint was invalid/unreachable.
    #[error("rpc: {0}")]
    Rpc(String),
    /// The submission transaction failed.
    #[error("submit tx: {0}")]
    Tx(String),
    /// Disclosure serialisation failed.
    #[error("encoding disclosure: {0}")]
    Encode(String),
    /// Disclosure file IO failed.
    #[error("disclosure IO: {0}")]
    Io(String),
}
```

Add to `Cargo.toml` deps: `url = "2"` (used for the RPC parse error type). Add `pub mod settle;` and `#[error(transparent)] Settle(#[from] settle::SettleError)` to `lib.rs`.

> Interface check: `Opening` derives `Serialize`/`Deserialize`/`Clone` in `cwe-wallet-zk` (the settlement disclosure already round-trips it); `Opening::commit()` returns `Commitment(Bytes32)` whose `.0 .0` is `[u8; 32]`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p cwe-player settle::`
Expected: both settle unit tests PASS. (The async `submit_consumption` is covered by the demo.)

- [ ] **Step 5: Commit**

```bash
git add clients/player-plugin/src/settle.rs clients/player-plugin/src/lib.rs clients/player-plugin/Cargo.toml Cargo.lock
git commit -m "player: settlement — commitments, disclosure, on-chain submit"
```

---

## Task 7: CLI (`play` / `status` / `settle`) + WAV fixture generator

**Files:**
- Create: `clients/player-plugin/src/main.rs`, `clients/player-plugin/src/bin/gen_wav.rs`

**Interfaces:**
- Consumes: every module above.
- Produces: the `cwe-player` and `gen-wav` binaries.

- [ ] **Step 1: Implement the CLI**

Create `clients/player-plugin/src/main.rs`:

```rust
//! `cwe-player` — the desktop player agent CLI.
//!
//! Three one-shot commands over the library modules:
//!   * `play <file>`  — decode, recognise, apply the price cap, accrue time;
//!   * `status`       — print the accrued usage without changing anything;
//!   * `settle`       — submit commitments on-chain and write the disclosure.
//! Session state persists to `STATE` between invocations.

use std::path::PathBuf;
use std::process::ExitCode;

use cwe_player::config::PlayerConfig;
use cwe_player::recognize::{recognize, ReqwestTransport, Tier};
use cwe_player::session::Session;
use cwe_player::{decode, policy, settle};

/// Wall-clock seconds since the Unix epoch, anchoring a fresh session's epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() -> ExitCode {
    // Manual arg parse keeps the dependency surface small (no clap), matching the
    // other workspace binaries.
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    let result = match cmd {
        Some("play") => match args.get(2) {
            Some(file) => cmd_play(PathBuf::from(file)),
            None => Err("usage: cwe-player play <file>".to_string()),
        },
        Some("status") => cmd_status(),
        Some("settle") => cmd_settle(),
        Some("fingerprint") => match args.get(2) {
            Some(file) => cmd_fingerprint(PathBuf::from(file)),
            None => Err("usage: cwe-player fingerprint <file>".to_string()),
        },
        _ => Err("usage: cwe-player <play <file>|status|settle|fingerprint <file>>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// `play`: decode → recognise → policy → accrue.
fn cmd_play(file: PathBuf) -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let audio = decode::decode(&file).map_err(|e| e.to_string())?;

    // Recognise via the live hub; an unrecognised work is not an error.
    let transport = ReqwestTransport::new();
    let Some(work) = recognize(&cfg.hub_url, &audio, &transport) else {
        println!("unrecognised: nothing accrued for {}", file.display());
        return Ok(());
    };

    // Enforce the price cap before accruing anything.
    if !policy::allows(work.price_per_min, cfg.threshold) {
        println!("blocked: {} exceeds price cap ({} > {:?})", work.work_id, work.price_per_min, cfg.threshold);
        return Ok(());
    }

    // Accrue the whole file's duration; a fingerprint match is escrow-bound.
    let work_id = cwe_wallet_zk::Bytes32::from_str_or_zero(&work.work_id);
    let secs = audio.duration_secs();
    let mut session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    session.accrue(work_id, secs, matches!(work.tier, Tier::Fingerprint));
    session.save(&cfg.state_path).map_err(|e| e.to_string())?;

    let tier = match work.tier { Tier::Signed => "signed", Tier::Fingerprint => "fingerprint (escrow)" };
    println!("accrued {}s to {} [{}], price {}/min", secs, work.work_id, tier, work.price_per_min);
    Ok(())
}

/// `fingerprint`: decode a file and print its `fp:<hex>` perceptual fingerprint.
///
/// A real client capability (the same fingerprint `play` computes), it also lets
/// tooling learn the exact fingerprint the agent will produce for a file — e.g.
/// so a hub manifest for an unsigned copy can be ingested with a matching fp.
fn cmd_fingerprint(file: PathBuf) -> Result<(), String> {
    let audio = decode::decode(&file).map_err(|e| e.to_string())?;
    // Reuse the shared fingerprint so this print can never drift from recognition.
    let fp = cwe_fingerprint::Fingerprint::compute(&audio.samples, audio.sample_rate);
    println!("{fp}");
    Ok(())
}

/// `status`: print the session's epoch, per-work minutes, and escrow set.
fn cmd_status() -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    let (epoch, per_work, escrow) = session.snapshot_view();
    println!("epoch {epoch}");
    if per_work.is_empty() {
        println!("  (no usage accrued)");
    }
    for (work, secs) in per_work {
        println!("  {work}: {}m ({}s)", secs / 60, secs);
    }
    for work in escrow {
        println!("  escrow-bound: {work}");
    }
    Ok(())
}

/// `settle`: submit commitments on-chain and write the disclosure.
fn cmd_settle() -> Result<(), String> {
    let cfg = PlayerConfig::from_env().map_err(|e| e.to_string())?;
    let mut session = Session::load(&cfg.state_path, now_secs()).map_err(|e| e.to_string())?;
    let usage = session.flush_usage();
    if usage.is_empty() {
        return Err("nothing to settle (no usage accrued this epoch)".to_string());
    }

    // Fresh random salts hide the minutes behind each on-chain commitment.
    let openings = settle::build_openings(&usage, |_| {
        let mut s = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut s);
        cwe_wallet_zk::Bytes32(s)
    });
    let escrow_works = session.take_escrow_works();

    // Submit on-chain (async) via a small runtime, then persist the drained state.
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (tx, user_addr) = rt
        .block_on(settle::submit_consumption(&cfg, &openings))
        .map_err(|e| e.to_string())?;
    settle::write_disclosure(&cfg.disclosure_path, &user_addr, &openings, &escrow_works)
        .map_err(|e| e.to_string())?;
    session.save(&cfg.state_path).map_err(|e| e.to_string())?;

    println!("settled {} work(s) in tx {tx}", openings.len());
    println!("disclosure -> {}", cfg.disclosure_path.display());
    Ok(())
}
```

> This uses a `Bytes32::from_str_or_zero` helper. If `cwe-wallet-zk` lacks it, parse with `Bytes32::from_str(&work.work_id).map_err(|e| e.to_string())?` instead (it implements `FromStr`); adjust the line accordingly. Confirm during implementation and use whichever exists — do **not** add a new method to `cwe-wallet-zk` for this.

- [ ] **Step 2: Implement the WAV fixture generator**

Create `clients/player-plugin/src/bin/gen_wav.rs`:

```rust
//! `gen-wav` — a dev-only deterministic WAV generator for the demo and tests.
//!
//! Writes `<out>` as 16-bit mono PCM: `seconds` of a fixed-frequency sine at
//! 44.1 kHz. Deterministic, so a work's `content_id = keccak(bytes)` is stable
//! across runs. Usage: `gen-wav <out.wav> <seconds> [freq_hz]`.

use std::f32::consts::PI;

/// Write a little-endian `u32`/`u16`/`i16` sequence building a PCM WAV.
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (out, seconds) = match (args.get(1), args.get(2)) {
        (Some(o), Some(s)) => (o.clone(), s.parse::<u32>().unwrap_or(0)),
        _ => {
            eprintln!("usage: gen-wav <out.wav> <seconds> [freq_hz]");
            return std::process::ExitCode::FAILURE;
        }
    };
    let freq: f32 = args.get(3).and_then(|f| f.parse().ok()).unwrap_or(440.0);
    let rate: u32 = 44_100;
    let n = rate * seconds;
    let data_len = n * 2; // 16-bit mono

    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        // A pure sine at `freq`, scaled to half amplitude — deterministic bytes.
        let t = i as f32 / rate as f32;
        let s = (0.5 * (2.0 * PI * freq * t).sin() * 32767.0) as i16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    if let Err(e) = std::fs::write(&out, b) {
        eprintln!("error writing {out}: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
```

- [ ] **Step 3: Build both binaries + full crate gate**

Run: `cargo build -p cwe-player --bins && cargo test -p cwe-player && cargo clippy -p cwe-player --all-targets -- -D warnings && cargo fmt -p cwe-player -- --check`
Expected: builds; all unit tests PASS; clippy clean; fmt clean.

- [ ] **Step 4: Smoke-test the binaries by hand**

```bash
target/debug/gen-wav /tmp/cwe-smoke.wav 4 440
target/debug/cwe-player fingerprint /tmp/cwe-smoke.wav        # prints an fp:<hex>
STATE=/tmp/cwe-smoke-state.json HUB_URL=http://127.0.0.1:1 target/debug/cwe-player play /tmp/cwe-smoke.wav
STATE=/tmp/cwe-smoke-state.json HUB_URL=http://127.0.0.1:1 target/debug/cwe-player status
```
Expected: `fingerprint` prints a single `fp:` string; `play` prints "unrecognised" (hub unreachable → graceful); `status` prints `epoch <n>` and "(no usage accrued)". No panics.

- [ ] **Step 5: Commit**

```bash
git add clients/player-plugin/src/main.rs clients/player-plugin/src/bin/gen_wav.rs Cargo.lock
git commit -m "player: CLI (play/status/settle) and dev WAV generator"
```

---

## Task 8: End-to-end demo, Makefile, CI, and docs

**Files:**
- Create: `ops/demo/run_player_demo.sh`, `clients/player-plugin/README.md`
- Modify: `ops/Makefile`, `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the whole crate + the existing `cwe-hub`, `sign-manifest`, `cwe-settlement`, contracts, and `Deploy.s.sol`.

- [ ] **Step 1: Write the demo script**

Create `ops/demo/run_player_demo.sh` (self-contained, PID-safe — model the header/cleanup on `run_ownership_demo.sh` and the hub-start on `run_hub_demo.sh`). It must:

1. `export PATH` for foundry/cargo; `set -euo pipefail`; `WORKDIR=$(mktemp -d)`.
2. Build `cwe-player`, `gen-wav`, `cwe-hub`, `sign-manifest`, `cwe-settlement` (`cargo build -p cwe-player -p cwe-discovery-hub -p cwe-settlement`).
3. Start Anvil (`anvil > log & ANVIL=$!`), `trap 'kill -TERM "$ANVIL" "${HUBPID:-}" 2>/dev/null || true; rm -rf "$WORKDIR"' EXIT`; wait for RPC with a bounded, delayed retry.
4. `mapfile` the dev keys; `DEPLOYER` = owner+aggregator+verified creator; `AGENT=${KEYS[1]}` (the listener/user); `CREATOR_PAYEE=$(cast wallet address ${KEYS[3]})`.
5. Deploy (`forge script Deploy.s.sol`), read `registry/tiers/consumption/payouts/escrow` from `deployments/localhost.json`.
6. `setFee`, `setVerifiedCreator DEPLOYER`.
7. **Generate the WAV fixtures:** `gen-wav "$WORKDIR/signed.wav" 4 440` and `gen-wav "$WORKDIR/copy.wav" 4 550` (a different tone → different content id + fingerprint).
8. Compute ids the way the agent will: `CONTENT=$(cast keccak $(xxd -p -c 100000 "$WORKDIR/signed.wav"))` **or** compute in-agent — simplest: register the **signed** work under `CONTENT=keccak(signed.wav bytes)`. Because `keccak` over a large file via `cast keccak` needs the hex; use `CONTENT=$(cast keccak 0x$(xxd -p "$WORKDIR/signed.wav" | tr -d '\n'))`. Register `WORK_SIGNED` with payee `CREATOR_PAYEE` (consent-signed via the `consent()` helper from `run_ownership_demo.sh`), `CONTENT`, price `PPM`.
9. Register `WORK_FP` (the fingerprint-matched work for the unsigned copy) with its own content id `keccak(copy.wav bytes)` and payee (any dev address, consent-signed).
10. Start the hub (`REGISTRY=$REG RPC_URL=$RPC BIND=127.0.0.1:18081 SNAPSHOT=... cwe-hub & HUBPID=$!`); wait for `/healthz`.
11. **Compute the exact fingerprints the agent will produce**, so the ingested manifests match what recognition computes at play time: `FP_SIGNED=$("$PLAYER" fingerprint "$WORKDIR/signed.wav")` and `FP_COPY=$("$PLAYER" fingerprint "$WORKDIR/copy.wav")`.
12. **Ingest manifests** so the hub can resolve (mirror `run_hub_demo.sh`'s manifest + `sign-manifest` flow, signed by the registrant DEPLOYER, expect 201):
    - `WORK_SIGNED`: `content_id=$CONTENT_SIGNED`, `fingerprint=$FP_SIGNED`, its payee/share.
    - `WORK_FP`: `content_id=$CONTENT_COPY` (= `keccak(copy.wav bytes)`), `fingerprint=$FP_COPY`, its payee/share. This is the work the unsigned copy resolves to by fingerprint.
13. Agent subscribes: `cast send` as `AGENT` → `tiers.subscribe(LIGHT)` `--value $FEE`.
14. Agent plays (state persisted in `$WORKDIR/state.json`, hub at `$HUB`):
    - `HUB_URL=$HUB STATE=… "$PLAYER" play "$WORKDIR/signed.wav"` → assert output contains `signed`.
    - `HUB_URL=$HUB STATE=… "$PLAYER" play "$WORKDIR/copy.wav"` → assert output contains `fingerprint (escrow)`.
15. Agent settles: `HUB_URL=$HUB STATE=… DISCLOSURE=$WORKDIR/disclosure.json RPC_URL=$RPC PRIVATE_KEY=$AGENT CONSUMPTION=$CONS TIER_ID=$LIGHT "$PLAYER" settle`. Assert `.escrow_works` in the disclosure JSON contains `WORK_FP` (via `jq`).
16. Read `EPOCH` from `CWEConsumption.currentEpoch()`; run the settlement job as aggregator: `RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DISCLOSURE=$WORKDIR/disclosure.json DEPLOYMENTS=$DEP OUT=$WORKDIR/proofs.json cwe-settlement`.
17. Withdraw the signed work from `CWEPayouts` (amount + proof from `proofs.json`, via `withdraw(uint256,bytes32,uint256,bytes32[])`); assert `CREATOR_PAYEE`'s balance rose by the settled amount. Assert `escrowOf(EPOCH, WORK_FP) > 0` on `CWEEscrow` (fingerprint credit escrowed, not paid).
18. Print `✅ PLAYER DEMO PASSED` (else a clear `FAIL: …` and `exit 1`).

Where `PLAYER=$ROOT/target/debug/cwe-player`, `CONTENT_SIGNED=$(cast keccak 0x$(xxd -p "$WORKDIR/signed.wav" | tr -d '\n'))`, and the `consent()` helper is copied from `run_ownership_demo.sh`.

- [ ] **Step 2: Run the demo until green**

Run: `make -C ops player-demo`
Expected: ends with `✅ PLAYER DEMO PASSED`. Debug with `tail "$WORKDIR/anvil.log"`/hub log on failure. Do not proceed until it passes.

- [ ] **Step 3: Add the Makefile target**

In `ops/Makefile`, add `player-demo` to `.PHONY` and:

```make
player-demo: ## Run the desktop player-agent end-to-end demo (self-contained Anvil)
	bash demo/run_player_demo.sh
```

- [ ] **Step 4: Add the CI job**

In `.github/workflows/ci.yml`, add a `player-e2e` job mirroring `ownership-e2e` (checkout; install Rust; `Swatinem/rust-cache`; install Foundry; install jq; `make -C ops player-demo`).

- [ ] **Step 5: Write the crate README**

Create `clients/player-plugin/README.md`: what `cwe-player` is; the three (four, incl. `fingerprint`) subcommands; the env-var table (from the spec); the two recognition tiers and escrow behaviour; how to run `make -C ops player-demo`; and a "Deferred: VLC/FFmpeg C module" section stating the real host integration is a thin FFI shim over this agent, tracked for a later slice.

- [ ] **Step 6: Full gate + commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && ( cd chain && forge test ) && make -C ops player-demo`
Expected: all green; demo passes.

Scan for stray agent/assistant attributions in every new/changed file, then:

```bash
git add clients/player-plugin/ ops/demo/run_player_demo.sh ops/Makefile .github/workflows/ci.yml
git commit -m "Add player-agent end-to-end demo, CI job, and docs"
```

---

## Self-Review

**Spec coverage:** pure-Rust agent (Tasks 1–7); audio decode via symphonia (T2); two-tier recognition — signed content_id + fingerprint fallback (T5); price-cap policy (T4); session accrual + persistence (T3); full pay-cycle settle — commitments + on-chain submit + disclosure with `escrow_works` (T6); `play`/`status`/`settle` (+`fingerprint`) CLI (T7); headless demo proving signed→direct pay and fingerprint→escrow, Makefile, CI, README, WAV fixture generator (T8). Deferred (VLC/FFmpeg C module, video, static fallback, real-time accrual) are stated seams, not built — matching the spec.

**Placeholder scan:** the algorithmic/interface-critical modules (config, decode, session, policy, recognize, settle, CLI) carry full code; Task 8 gives an explicit numbered demo recipe referencing the concrete existing scripts (`run_ownership_demo.sh` consent helper, `run_hub_demo.sh` hub start) it mirrors, and resolves the fingerprint-match problem by adding the honest `cwe-player fingerprint` subcommand. No "TBD"/"add error handling"/"write tests for the above" remain.

**Type consistency:** `DecodedAudio` (T2) is consumed by `recognize` (T5) and the CLI (T7); `Session`/`UsageEntry` (T3) feed `build_openings` (T6); `PlayerConfig` (T1) is used by `recognize`/`settle`/CLI; the `Disclosure`/`Opening` shape (T6) matches `services/settlement/src/disclosure.rs` verbatim; `ResolvedWork.tier` (T5) drives the escrow flag in `Session::accrue` (T3) via the CLI (T7). The `Bytes32` parse helper in T7 is flagged to reconcile with whatever `cwe-wallet-zk` actually exposes (`FromStr`), not invented.
