# Recognition & Ownership (H1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recognise works primarily by cryptographic proof (signed content + multi-party
consent) and pay owners directly, with a modest perceptual audio fingerprint as a
fallback whose earnings escrow behind a challenge window — so CWE never pays a non-owner.

**Architecture:** Two recognition tiers. Tier 1 (authoritative): a creator-signed content
manifest binds `content_id = keccak256(content)` and a consent-signed split table; the
client verifies signature + content hash → direct payout. Tier 2 (fallback): a perceptual
fingerprint match on unsigned content routes credit to a `CWEEscrow` with a
first-registration priority rule and an `IArbiter` seam. Reuses Phase 1 contracts/settlement
and the Phase 2 Discovery Hub/manifest infrastructure.

**Tech Stack:** Rust (`libs/fingerprint`, `services/*`, extension WASM core), `rustfft`
(audio DSP), Solidity/Foundry (`CWERegistry`, new `CWEEscrow`/`IArbiter`), alloy, axum,
Node/esbuild (extension), bash + cast (demo).

**Design doc:** `docs/superpowers/specs/2026-07-21-recognition-and-ownership-design.md`

## Global Constraints

- **Language:** Rust everywhere, except Solidity contracts (`chain/`) and the browser
  extension's JS shell.
- **Comments:** every function/method gets a doc comment; non-trivial lines get an inline
  comment only when it adds understanding.
- **No AI attribution anywhere** (code, comments, docs, commit messages — no
  `Co-Authored-By`/"Generated with" trailers — branch names, or GitHub-visible text).
- **Quality gate (stays green):** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `forge test` (in `chain/`).
- **Determinism:** all hashes are keccak256; the fingerprint bit-extraction must be stable
  across platforms (compare energy differences, never rely on exact float equality).
- **Anti-fraud invariants (must hold):** a fingerprint match never pays directly; escrowed
  credit is only released after the challenge window; a challenge with a strictly earlier
  registration reassigns escrow; a signed-exact content match always pays the registrant
  directly (never escrows).
- Work on branch `h1-recognition-ownership`; commit frequently; local is source of truth.

---

## Task 1: Perceptual audio fingerprint (`libs/fingerprint`)

Replace the SHA-256 stub with a modest-but-real acoustic fingerprint (Haitsma-Kalker
sub-fingerprints), gain-invariant by construction, compared by Hamming similarity.

**Files:**
- Modify: `libs/fingerprint/Cargo.toml` (add `rustfft`)
- Rewrite: `libs/fingerprint/src/lib.rs`
- Modify: root `Cargo.toml` (`rustfft` in `[workspace.dependencies]`)
- Modify: `libs/fingerprint/README.md`

**Interfaces:**
- Produces (`cwe_fingerprint`):
  - `const FRAMES: usize = 32; const BITS_PER_FRAME: usize = 32;` (fingerprint = 1024 bits)
  - `struct Fingerprint { sub: [u32; FRAMES] }` derives `Clone, Copy, PartialEq, Eq`
  - `fn Fingerprint::compute(samples: &[f32], sample_rate: u32) -> Fingerprint`
  - `fn Fingerprint::to_hex(&self) -> String` (128-byte hex) / `fn parse(&str) -> Result<Fingerprint, FingerprintError>` / `Display` as `fp:<hex>`
  - `fn Fingerprint::id(&self) -> [u8;32]` (keccak256 of the bits — compact id for exact dedup)
  - `fn compare(a: &Fingerprint, b: &Fingerprint) -> f64` (Hamming similarity in `[0,1]`)
  - `enum FingerprintError`

- [ ] **Step 1: Add the dependency**

Root `Cargo.toml` `[workspace.dependencies]`: `rustfft = "6"`. `libs/fingerprint/Cargo.toml`
`[dependencies]`: add `rustfft.workspace = true` (keep `sha2`? no — remove `sha2`, it is no
longer used; keep `hex`, `thiserror`, add `tiny-keccak = { workspace = true }` for `id()`).

- [ ] **Step 2: Write the failing tests**

Replace the `libs/fingerprint/src/lib.rs` test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Generate `secs` of a mono sine wave at `freq` Hz, amplitude `amp`.
    fn tone(freq: f32, amp: f32, secs: f32, sr: u32) -> Vec<f32> {
        let n = (secs * sr as f32) as usize;
        (0..n).map(|i| amp * (2.0 * PI * freq * i as f32 / sr as f32).sin()).collect()
    }

    /// The same audio yields the same fingerprint (determinism).
    #[test]
    fn compute_is_deterministic() {
        let a = tone(440.0, 0.8, 3.0, 11025);
        assert_eq!(Fingerprint::compute(&a, 11025), Fingerprint::compute(&a, 11025));
    }

    /// Gain invariance: halving the amplitude barely changes the fingerprint
    /// (Haitsma-Kalker bits are signs of energy *differences*, so gain cancels).
    #[test]
    fn robust_to_volume_change() {
        let loud = tone(440.0, 0.9, 3.0, 11025);
        let quiet = tone(440.0, 0.45, 3.0, 11025);
        let sim = compare(&Fingerprint::compute(&loud, 11025), &Fingerprint::compute(&quiet, 11025));
        assert!(sim > 0.95, "gain change should preserve the fingerprint, got {sim}");
    }

    /// Distinct audio is far apart (well below a match threshold).
    #[test]
    fn distinct_audio_differs() {
        let a = tone(440.0, 0.8, 3.0, 11025);
        let b = tone(1200.0, 0.8, 3.0, 11025);
        let sim = compare(&Fingerprint::compute(&a, 11025), &Fingerprint::compute(&b, 11025));
        assert!(sim < 0.75, "distinct tones should differ, got {sim}");
    }

    /// compare() is bounded and self-similarity is 1.0.
    #[test]
    fn compare_bounds() {
        let a = Fingerprint::compute(&tone(440.0, 0.8, 3.0, 11025), 11025);
        assert_eq!(compare(&a, &a), 1.0);
    }

    /// Hex render → parse round-trips.
    #[test]
    fn hex_round_trip() {
        let a = Fingerprint::compute(&tone(440.0, 0.8, 3.0, 11025), 11025);
        assert_eq!(Fingerprint::parse(&a.to_string()).unwrap(), a);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p cwe-fingerprint`  Expected: FAIL to compile (API changed).

- [ ] **Step 4: Implement the fingerprint**

Replace the non-test portion of `libs/fingerprint/src/lib.rs` with the Haitsma-Kalker
implementation:

```rust
//! CWE audio fingerprint — a modest but real acoustic fingerprint.
//!
//! Uses the Haitsma-Kalker scheme: the audio is resampled to a canonical rate, split
//! into overlapping frames, each frame's energy is measured in 33 logarithmically-spaced
//! sub-bands, and each of the 32 sub-fingerprint bits is the SIGN of a second-order energy
//! difference across band and time. Because the bits depend only on energy *differences*,
//! the fingerprint is invariant to overall volume and robust to mild re-encoding — while
//! remaining simple, deterministic, and dependency-light. Two fingerprints are compared by
//! Hamming similarity (1 − bit-error-rate). This is a *fallback* recogniser; production-
//! grade robustness (Chromaprint/AcoustID) is future work.

#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use tiny_keccak::{Hasher, Keccak};

/// Canonical sample rate the input is resampled to before analysis.
const CANONICAL_SR: u32 = 11_025;
/// FFT frame length in samples (~0.19 s at the canonical rate).
const FRAME: usize = 2048;
/// Hop between frames (50% overlap).
const HOP: usize = FRAME / 2;
/// Number of sub-fingerprint frames retained (fixed length for embedding/compare).
pub const FRAMES: usize = 32;
/// Bits per sub-fingerprint frame (33 bands → 32 difference bits).
pub const BITS_PER_FRAME: usize = 32;
/// Number of energy sub-bands.
const BANDS: usize = BITS_PER_FRAME + 1;
/// Frequency range for the band split (Hz).
const F_LO: f32 = 300.0;
const F_HI: f32 = 2000.0;

/// A fixed-length acoustic fingerprint: `FRAMES` 32-bit sub-fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    sub: [u32; FRAMES],
}

/// Errors parsing a fingerprint from text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FingerprintError {
    #[error("fingerprint must start with the 'fp:' prefix")]
    MissingPrefix,
    #[error("fingerprint hex must be {expected} chars, found {found}")]
    BadLength { expected: usize, found: usize },
    #[error("fingerprint contains a non-hexadecimal character")]
    NotHex,
}

/// Textual prefix.
pub const PREFIX: &str = "fp:";
/// Byte length of the fingerprint (FRAMES * 4).
const BYTE_LEN: usize = FRAMES * 4;
const HEX_LEN: usize = BYTE_LEN * 2;

impl Fingerprint {
    /// Compute the fingerprint of mono `samples` recorded at `sample_rate` Hz.
    pub fn compute(samples: &[f32], sample_rate: u32) -> Fingerprint {
        // 1. Resample to the canonical rate with simple linear interpolation.
        let audio = resample(samples, sample_rate, CANONICAL_SR);
        // 2. Precompute the FFT-bin ranges for each logarithmic sub-band.
        let bands = band_ranges();
        // 3. Per-frame band energies.
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME);
        let mut energies: Vec<[f32; BANDS]> = Vec::new();
        let mut pos = 0;
        while pos + FRAME <= audio.len() {
            let mut buf: Vec<Complex<f32>> =
                audio[pos..pos + FRAME].iter().map(|&s| Complex { re: s, im: 0.0 }).collect();
            fft.process(&mut buf);
            let mut e = [0f32; BANDS];
            for (b, &(lo, hi)) in bands.iter().enumerate() {
                // Sum |X|^2 over the band's bins.
                e[b] = buf[lo..hi].iter().map(|c| c.norm_sqr()).sum();
            }
            energies.push(e);
            pos += HOP;
        }
        // 4. Second-order difference sign bits, per Haitsma-Kalker.
        let mut sub = [0u32; FRAMES];
        for f in 0..FRAMES {
            // Pad by repeating the last available frame if the audio was short.
            let cur = *energies.get(f + 1).or_else(|| energies.last()).unwrap_or(&[0.0; BANDS]);
            let prev = *energies.get(f).or_else(|| energies.last()).unwrap_or(&[0.0; BANDS]);
            let mut bits = 0u32;
            for m in 0..BITS_PER_FRAME {
                let d = (cur[m] - cur[m + 1]) - (prev[m] - prev[m + 1]);
                if d > 0.0 {
                    bits |= 1 << m;
                }
            }
            sub[f] = bits;
        }
        Fingerprint { sub }
    }

    /// The raw 128-byte big-endian encoding of the sub-fingerprints.
    fn to_bytes(&self) -> [u8; BYTE_LEN] {
        let mut out = [0u8; BYTE_LEN];
        for (i, w) in self.sub.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    /// 64-hex-char keccak256 id of the fingerprint (compact key for exact dedup).
    pub fn id(&self) -> [u8; 32] {
        let mut h = Keccak::v256();
        h.update(&self.to_bytes());
        let mut out = [0u8; 32];
        h.finalize(&mut out);
        out
    }

    /// The 128-byte fingerprint as hex (no prefix).
    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    /// Parse from the canonical `fp:<256 hex>` form.
    pub fn parse(s: &str) -> Result<Fingerprint, FingerprintError> {
        let hex_part = s.strip_prefix(PREFIX).ok_or(FingerprintError::MissingPrefix)?;
        if hex_part.len() != HEX_LEN {
            return Err(FingerprintError::BadLength { expected: HEX_LEN, found: hex_part.len() });
        }
        let bytes = hex::decode(hex_part).map_err(|_| FingerprintError::NotHex)?;
        let mut sub = [0u32; FRAMES];
        for i in 0..FRAMES {
            sub[i] = u32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Ok(Fingerprint { sub })
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.to_hex())
    }
}

/// Hamming similarity in `[0.0, 1.0]`: 1 − (differing bits / total bits).
pub fn compare(a: &Fingerprint, b: &Fingerprint) -> f64 {
    let mut diff = 0u32;
    for i in 0..FRAMES {
        diff += (a.sub[i] ^ b.sub[i]).count_ones();
    }
    let total = (FRAMES * BITS_PER_FRAME) as f64;
    1.0 - (diff as f64 / total)
}

/// Linear-interpolation resampler from `from` Hz to `to` Hz.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((samples.len() as f64) / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let idx = src as usize;
            let frac = (src - idx as f64) as f32;
            let a = samples[idx];
            let b = *samples.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// FFT-bin `[lo, hi)` ranges for each of the `BANDS` logarithmic sub-bands.
fn band_ranges() -> [(usize, usize); BANDS] {
    let bin = |hz: f32| ((hz / CANONICAL_SR as f32) * FRAME as f32) as usize;
    let mut ranges = [(0usize, 0usize); BANDS];
    for b in 0..BANDS {
        // Logarithmic edges between F_LO and F_HI.
        let lo = F_LO * (F_HI / F_LO).powf(b as f32 / BANDS as f32);
        let hi = F_LO * (F_HI / F_LO).powf((b + 1) as f32 / BANDS as f32);
        let (mut a, mut c) = (bin(lo), bin(hi).max(bin(lo) + 1));
        c = c.min(FRAME / 2);
        a = a.min(c.saturating_sub(1));
        ranges[b] = (a, c);
    }
    ranges
}
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p cwe-fingerprint`  Expected: PASS (5 tests). Then `cargo fmt --all` and
`cargo clippy -p cwe-fingerprint --all-targets -- -D warnings`.

- [ ] **Step 6: Update the README**

Rewrite `libs/fingerprint/README.md`: describe the Haitsma-Kalker acoustic fingerprint,
gain invariance, `compute`/`compare`/`id`, the fixed 1024-bit size, and that it is a
fallback recogniser (production robustness is future work). Note it now consumes decoded
audio samples, not raw bytes.

- [ ] **Step 7: Commit**

```bash
git add libs/fingerprint Cargo.toml
git commit -m "Replace fingerprint stub with a Haitsma-Kalker acoustic fingerprint"
```

> **Downstream note (later tasks handle):** this changes `Fingerprint`'s API and byte size.
> The extension WASM core (Task 7) and the hub/settlement (Tasks 5/6) update their call sites.
> The `cwe-ext-core` `fingerprint()` wasm export changes from bytes→string; Task 7 owns that.

---

## Task 2: `CWERegistry` — content_id, registration time, consent verification

Registration becomes provenance-bearing: it binds a real `content_id`, records a
registration timestamp (priority), and verifies each payee's consent signature.

**Files:**
- Modify: `chain/contracts/CWERegistry.sol`, `chain/contracts/interfaces/ICWERegistry.sol`
- Modify: `chain/test/CWERegistry.t.sol`, `chain/test/CWEPayouts.t.sol` (callers)
- Modify: `ops/demo/run_demo.sh`, `ops/demo/run_hub_demo.sh` (cast `registerWork` calls)

**Interfaces:**
- Produces (Solidity):
  - `registerWork(bytes32 workId, bytes32 contentId, address payable[] payees, uint96[] splits, bytes[] consentSigs, uint256 pricePerMin, bytes32 regionRule)` — now also verifies, for each `i`, that `consentSigs[i]` is a signature by `payees[i]` over `consentDigest(workId, contentId, payees[i], splits[i])`.
  - `function consentDigest(bytes32 workId, bytes32 contentId, address payee, uint96 share) external pure returns (bytes32)`
  - `function contentIdOf(bytes32 workId) external view returns (bytes32)`
  - `function registeredAtOf(bytes32 workId) external view returns (uint256)`
  - existing getters unchanged; `Work` struct gains `bytes32 contentId; uint256 registeredAt;`

- [ ] **Step 1: Write the failing tests**

Add to `chain/test/CWERegistry.t.sol` (use forge-std `vm.sign` to make payee consents). The
consent message is EIP-191 personal-signed over `consentDigest`:

```solidity
    /// A work registers only when every payee has consented to their share, and it
    /// records the content id and registration timestamp.
    function test_register_withConsent() public {
        (address alice, uint256 aliceK) = makeAddrAndKey("alice");
        (address bob, uint256 bobK) = makeAddrAndKey("bob");
        vm.prank(owner);
        registry.setVerifiedCreator(creator, true);

        bytes32 workId = keccak256("song-A");
        bytes32 contentId = keccak256("content-A");
        address payable[] memory payees = new address payable[](2);
        payees[0] = payable(alice); payees[1] = payable(bob);
        uint96[] memory splits = new uint96[](2);
        splits[0] = 700_000; splits[1] = 300_000;

        bytes[] memory sigs = new bytes[](2);
        sigs[0] = _consent(aliceK, workId, contentId, alice, splits[0]);
        sigs[1] = _consent(bobK, workId, contentId, bob, splits[1]);

        vm.warp(1000);
        vm.prank(creator);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));

        assertEq(registry.contentIdOf(workId), contentId);
        assertEq(registry.registeredAtOf(workId), 1000);
    }

    /// A missing/forged consent signature is rejected.
    function test_register_badConsent_reverts() public {
        (address alice, ) = makeAddrAndKey("alice");
        (, uint256 malloryK) = makeAddrAndKey("mallory");
        vm.prank(owner); registry.setVerifiedCreator(creator, true);

        bytes32 workId = keccak256("song-B"); bytes32 contentId = keccak256("content-B");
        address payable[] memory payees = new address payable[](1);
        payees[0] = payable(alice);
        uint96[] memory splits = new uint96[](1); splits[0] = 1_000_000;
        bytes[] memory sigs = new bytes[](1);
        // Signed by mallory, not alice.
        sigs[0] = _consent(malloryK, workId, contentId, alice, splits[0]);

        vm.prank(creator);
        vm.expectRevert(CWERegistry.BadConsent.selector);
        registry.registerWork(workId, contentId, payees, splits, sigs, 1000, bytes32("EU"));
    }

    /// Helper: EIP-191 personal-sign of the consent digest by key `k`.
    function _consent(uint256 k, bytes32 w, bytes32 c, address payee, uint96 share)
        internal view returns (bytes memory)
    {
        bytes32 digest = registry.consentDigest(w, c, payee, share);
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(k, eth);
        return abi.encodePacked(r, s, v);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd chain && forge test --match-contract CWERegistryTest`  Expected: FAIL (new
signature/functions absent).

- [ ] **Step 3: Implement**

In `CWERegistry.sol`: add `bytes32 contentId; uint256 registeredAt;` to `Work`; add
`error BadConsent();`; add `consentDigest`; extend `registerWork` to accept `contentId` and
`bytes[] consentSigs`, and after `_validateSplits`, loop over payees verifying consent:

```solidity
    /// @notice The digest each payee signs to consent to their share of a work.
    function consentDigest(bytes32 workId, bytes32 contentId, address payee, uint96 share)
        public pure returns (bytes32)
    {
        return keccak256(abi.encode(workId, contentId, payee, share));
    }

    // ...inside registerWork, after _validateSplits(payees, splits):
    // Verify every payee consented to their exact share (provenance).
    for (uint256 i = 0; i < payees.length; i++) {
        bytes32 digest = consentDigest(workId, contentId, payees[i], splits[i]);
        // EIP-191 personal-sign prefix, then recover.
        bytes32 eth = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        if (_recover(eth, consentSigs[i]) != payees[i]) revert BadConsent();
    }
    work.contentId = contentId;
    work.registeredAt = block.timestamp;
```

Add a minimal `_recover(bytes32 hash, bytes memory sig) internal pure returns (address)`
that splits `sig` into `r,s,v` and calls `ecrecover` (revert on bad length). Add
`contentIdOf`/`registeredAtOf` view getters and their `ICWERegistry` declarations.

- [ ] **Step 4: Update the existing callers so the suite stays green**

`CWEPayouts.t.sol` and the un-consented `CWERegistry.t.sol` cases call `registerWork` with
the old signature. Update each call to the new one: add a `contentId` (e.g.
`keccak256("c")`) and build a `bytes[] consentSigs` by signing with each payee's key (add a
shared `_consent` helper as above; give payees known keys via `makeAddrAndKey`). In
`run_demo.sh` and `run_hub_demo.sh`, the `cast send ... registerWork(...)` calls must add
the `contentId` arg and a `consentSigs` array — build each consent with
`cast wallet sign` over the EIP-191 digest (compute `consentDigest` via `cast call`, then
`cast wallet sign --no-hash`? use `cast wallet sign` which applies the EIP-191 prefix).
Confirm both existing demos still print their PASS lines.

- [ ] **Step 5: Run the full contract + demo gate**

Run: `cd chain && forge test`; then `make -C ops demo` and `make -C ops hub-demo`.
Expected: all green (contracts + both existing demos).

- [ ] **Step 6: Commit**

```bash
git add chain ops/demo/run_demo.sh ops/demo/run_hub_demo.sh
git commit -m "Registry: content id, registration timestamp, and payee consent verification"
```

---

## Task 3: Consent tooling (`sign-consent` + manifest assembly)

Give payees a way to produce consent signatures and the registrant a way to assemble them,
extending the Phase 2 `sign-manifest` CLI.

**Files:**
- Create: `services/discovery-hub/src/bin/sign_consent.rs`
- Modify: `services/discovery-hub/src/manifest.rs` (add `content_id`, the split table, and a
  consent digest helper matching the contract)

**Interfaces:**
- Produces:
  - `manifest::consent_digest(work_id: Bytes32, content_id: Bytes32, payee: Address, share: u64) -> [u8;32]` (keccak256(abi.encode(...)), matching `CWERegistry.consentDigest`)
  - a `sign-consent` binary: given `PRIVATE_KEY` and `work_id`/`content_id`/`payee`/`share`, prints the payee's `0x`-hex EIP-191 signature over the consent digest
  - `WorkManifest` gains `content_id: Bytes32` and `payees: Vec<(Address, u64)>` (address+share)

- [ ] **Step 1: Write the failing test** (in `manifest.rs`)

```rust
    /// A consent signature produced for a share recovers to the payee, and the digest
    /// matches the encoding the contract uses (keccak256(abi.encode(work,content,payee,share))).
    #[test]
    fn consent_digest_and_recover() {
        use alloy::signers::local::PrivateKeySigner;
        use alloy::signers::SignerSync;
        let signer = PrivateKeySigner::random();
        let digest = consent_digest(Bytes32([1;32]), Bytes32([2;32]), signer.address(), 700_000);
        let sig = signer.sign_message_sync(&digest).unwrap();
        // EIP-191 recover over the 32-byte digest.
        assert_eq!(sig.recover_address_from_msg(&digest).unwrap(), signer.address());
    }
```

- [ ] **Step 2–4: Implement + verify**

Add `consent_digest` using `alloy::primitives::keccak256(alloy::sol_types::SolValue::abi_encode(&(work_id_b256, content_id_b256, payee, U256::from(share))))` — encode exactly as the Solidity `abi.encode(bytes32, bytes32, address, uint96)`. Add `content_id`/`payees`
to `WorkManifest`. Implement `sign_consent.rs` mirroring `sign_manifest.rs` (read the four
fields from args/env, sign the digest, print the hex). Run `cargo test -p cwe-discovery-hub`
and a manual `sign-consent` smoke test; fmt+clippy.

- [ ] **Step 5: Commit**

```bash
git add services/discovery-hub
git commit -m "Add sign-consent CLI and content_id/split table to the manifest"
```

---

## Task 4: `CWEEscrow` + `IArbiter` seam

The escrow that holds fingerprint-matched credit behind a challenge window, resolved by
first-registration priority with an arbitration seam.

**Files:**
- Create: `chain/contracts/CWEEscrow.sol`, `chain/contracts/interfaces/ICWEEscrow.sol`, `chain/contracts/interfaces/IArbiter.sol`, `chain/contracts/EarliestRegistrationArbiter.sol`
- Create: `chain/test/CWEEscrow.t.sol`
- Modify: `chain/script/Deploy.s.sol` (deploy escrow + arbiter)

**Interfaces:**
- Produces (Solidity):
  - `IArbiter.resolve(bytes32 workA, bytes32 workB) -> bytes32 winner` — the earliest-registration stub reads `CWERegistry.registeredAtOf` and returns the earlier work.
  - `CWEEscrow`:
    - `commit(uint256 epochId, bytes32 workId, uint256 amount)` (aggregator-only) → escrows `amount` for `workId`, `releaseEpoch = epochId + CHALLENGE_WINDOW`
    - `challenge(uint256 epochId, bytes32 escrowedWork, bytes32 challengerWork)` → if `challengerWork`'s registration is strictly earlier (or the arbiter picks it), reassign the escrow to `challengerWork`
    - `release(uint256 epochId, bytes32 workId)` → after `releaseEpoch`, split-pay the work's payees (via the registry splits), reentrancy-safe
    - views: `escrowOf`, `releaseEpochOf`, `isReleased`

- [ ] **Step 1: Write the failing tests** (`CWEEscrow.t.sol`)

Cover: commit escrows and is not releasable before the window (`release` reverts
`TooEarly`); after warping past `releaseEpoch`, `release` pays the payees per split; a
`challenge` with an earlier-registered work reassigns escrow (then release pays the
challenger's payees); a challenge with a later registration reverts `ChallengeFailed`; only
the aggregator may `commit`; no double-release. Model the structure on the existing
`CWEPayouts.t.sol` (reentrancy-safe split-pay, `vm.warp`, `vm.deal`). Register the competing
works via `CWERegistry.registerWork` (with consents) at different `vm.warp` times to set
registration priority.

- [ ] **Steps 2–4: Implement + verify**

Implement `EarliestRegistrationArbiter` (reads `registeredAtOf`, returns earlier work; ties
→ `workA`). Implement `CWEEscrow` holding funds (funded from the payout pool / `receive()`),
using `ReentrancyGuard` and the registry for splits, mirroring `CWEPayouts.withdraw`'s
split-pay (last payee absorbs rounding; full dispersal). `challenge` compares registrations
via the `IArbiter`. Run `forge test`; ensure green.

- [ ] **Step 5: Commit**

```bash
git add chain
git commit -m "Add CWEEscrow with challenge window and earliest-registration arbiter"
```

---

## Task 5: Discovery Hub — fingerprint nearest-match + signed-vs-fp resolve

The hub gains a perceptual-fingerprint nearest-match and distinguishes an authoritative
signed-exact hit from a fingerprint candidate.

**Files:**
- Modify: `services/discovery-hub/src/index.rs`, `src/api.rs`, `src/manifest.rs`

**Interfaces:**
- Produces:
  - `index`: store the full `Fingerprint` per work; `resolve_content(content_id) -> Option<Resolved>` (exact, authoritative); `nearest_fingerprint(fp, threshold) -> Option<(Summary, f64)>`
  - `GET /resolve/content/{content_id}` → 200 signed-exact `Resolved` or 404
  - `GET /resolve/fingerprint/{fp}?threshold=` → 200 `{ candidate, similarity }` or 404

- [ ] **Steps:** TDD as in Phase 2. Store the parsed `cwe_fingerprint::Fingerprint` on ingest
  (validate it parses); index by `content_id` too. Add the two endpoints; `nearest_fingerprint`
  scans candidates and returns the best `compare()` above `threshold` (a linear scan is fine
  for the MVP; note LSH as future work). Unit-test: exact content resolves; a volume-changed
  fingerprint matches above threshold; a distinct one does not. Then fmt/clippy/test; commit
  `"Hub: content-id resolve and fingerprint nearest-match"`.

---

## Task 6: Settlement — route signed → direct, fingerprint → escrow

The settlement job splits credits into a direct-payout root (signed works) and an escrow
set (fingerprint-matched), and drives release/reassignment.

**Files:**
- Modify: `services/settlement/src/settle.rs`, `src/chain.rs`, `src/main.rs`, `src/disclosure.rs`

**Interfaces:**
- Consumes: the disclosure now marks each usage record as `signed` (Tier 1, with a verified
  `content_id`) or `fingerprint` (Tier 2, with the matched `work_id`).
- Produces: `Settlement { direct: Vec<Entry>, escrow: Vec<Entry>, .. }`; the chain layer
  calls `CWEPayouts.commitEpoch` for `direct` (as today) and `CWEEscrow.commit` for `escrow`.

- [ ] **Steps:** TDD the pure routing in `settle.rs` (a dataset tagged signed/fp produces the
  two buckets; signed never escrows; totals conserved). Extend the disclosure format with the
  tier tag. In `chain.rs`, commit the direct Merkle root to `CWEPayouts` and each escrow entry
  to `CWEEscrow`. Unit-test routing; the live path is covered by the demo. fmt/clippy/test;
  commit `"Settlement: route signed payouts direct and fingerprint matches to escrow"`.

---

## Task 7: Client (extension) — Tier 1 verify, Tier 2 fingerprint

The extension verifies signed content (Tier 1) and computes the real fingerprint for the
fallback (Tier 2), reflecting escrow status in the UI.

**Files:**
- Modify: `clients/browser-ext/core/src/lib.rs` (wasm: real `fingerprint(samples)` + a
  `content_hash(bytes)` export), `clients/browser-ext/src/background.js`,
  `clients/browser-ext/src/content-script.js`, popup, `test/hub.test.mjs`

**Interfaces:**
- Produces (wasm): `fingerprint(samples: &[f32]) -> String` (now the acoustic fp) and
  `content_hash(bytes: &[u8]) -> String` (keccak256 hex, for Tier 1 content_id).

- [ ] **Steps:** Update the WASM core to the new `Fingerprint` API (samples in, `fp:` out) and
  add `content_hash`. In the background worker: for served/same-origin content, compute the
  content hash and try `GET /resolve/content/{hash}` (Tier 1, authoritative); on miss, tap
  WebAudio samples, compute the fingerprint, and try `GET /resolve/fingerprint/{fp}` (Tier 2),
  marking that usage as escrow-bound in the settle payload/UI. Update the content script to
  deliver audio samples where CORS permits (served content). Rebuild (`npm run build`); update
  the `hub.test.mjs` stub for the two resolve paths; `npm test`. Commit
  `"Extension: signed content verification and acoustic fingerprint fallback"`.

---

## Task 8: `make ownership-demo` + docs + CI

Prove the whole model, including a multi-collaborator work.

**Files:**
- Create: `ops/demo/run_ownership_demo.sh`; Modify: `ops/Makefile`, `.github/workflows/ci.yml`
- Create/Modify: `services/discovery-hub/README.md`, `docs/` walkthrough

- [ ] **Steps:** Write `run_ownership_demo.sh` (self-contained, PID-safe cleanup like the
  other demos) that: deploys (registry + payouts + escrow + arbiter + hub); registers a
  **multi-collaborator song** — a band member, a session musician, and a cover designer, each
  producing a consent signature via `sign-consent`, assembled by the registrant with a real
  `content_id`; plays the **signed** content → asserts a **direct** payout split among the
  three consenting payees; plays an **unsigned copy** → fingerprint match → asserts the credit
  is **escrowed, not paid**; submits a **challenge** from an earlier-registered work → asserts
  reassignment; warps past the window and **releases** → asserts payout. Add the `ownership-demo`
  Makefile target (+`.PHONY`) and a `ownership-e2e` CI job mirroring `hub-e2e`. Document the
  recognition tiers, the consent/provenance procedure (with the band example), and the escrow
  in the README. Run `make -C ops ownership-demo` until it prints its ✅ PASS line, and run the
  full gate. Commit `"Add ownership demo (multi-collaborator), CI job, and docs"`.

---

## Self-Review

**Spec coverage:** Tier 1 signed recognition (T2 registry content_id/consent + T5 hub content
resolve + T7 client verify); multi-party consent provenance (T2 on-chain verify + T3 tooling +
T8 demo); perceptual fingerprint (T1); escrow + challenge + first-registration priority +
arbiter seam (T4); settlement routing signed→direct / fp→escrow (T6); the demo exercising all
four exit-criterion steps incl. a collaborative work (T8). Deferred items (arbitration jury,
SSI, production FP robustness, real-web capture) are seams, not built, per the design.

**Placeholder scan:** the algorithmic/security-critical steps (T1 fingerprint, T2 consent
`ecrecover`, T4 escrow) carry full code; Tasks 5–8 give exact files, interfaces, endpoints,
and test cases and reference the concrete Phase 1/2 patterns they mirror (`CWEPayouts` split-pay,
the Phase 2 hub/index tests, the existing demo scripts) rather than restating them. No
"TBD"/"add error handling"/"write tests for the above" remain.

**Type consistency:** `Fingerprint` (T1) is consumed by the hub (T5), settlement (T6), and the
wasm core (T7); `consentDigest` is defined identically in the contract (T2) and the tooling
(T3) — both `keccak256(abi.encode(workId, contentId, payee, share))` — and the demo (T8) relies
on that agreement; `CWEEscrow.commit/challenge/release` (T4) are called by settlement (T6) and
the demo (T8); `registeredAtOf`/`contentIdOf` (T2) are read by the arbiter and hub.
