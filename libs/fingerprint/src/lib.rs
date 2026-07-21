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
    /// The string did not begin with the required `fp:` prefix.
    #[error("fingerprint must start with the 'fp:' prefix")]
    MissingPrefix,
    /// The hex portion had the wrong number of characters.
    #[error("fingerprint hex must be {expected} chars, found {found}")]
    BadLength {
        /// How many hex characters a valid fingerprint has.
        expected: usize,
        /// How many were actually supplied.
        found: usize,
    },
    /// The hex portion contained a non-hexadecimal character.
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
            let mut buf: Vec<Complex<f32>> = audio[pos..pos + FRAME]
                .iter()
                .map(|&s| Complex { re: s, im: 0.0 })
                .collect();
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
        for (f, slot) in sub.iter_mut().enumerate() {
            // Pad by repeating the last available frame if the audio was short.
            let cur = *energies
                .get(f + 1)
                .or_else(|| energies.last())
                .unwrap_or(&[0.0; BANDS]);
            let prev = *energies
                .get(f)
                .or_else(|| energies.last())
                .unwrap_or(&[0.0; BANDS]);
            let mut bits = 0u32;
            for m in 0..BITS_PER_FRAME {
                let d = (cur[m] - cur[m + 1]) - (prev[m] - prev[m + 1]);
                if d > 0.0 {
                    bits |= 1 << m;
                }
            }
            *slot = bits;
        }
        Fingerprint { sub }
    }

    /// The raw 128-byte big-endian encoding of the sub-fingerprints.
    fn to_bytes(self) -> [u8; BYTE_LEN] {
        let mut out = [0u8; BYTE_LEN];
        for (i, w) in self.sub.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    /// The 32-byte keccak256 id of the fingerprint (a compact key for exact dedup).
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
        let hex_part = s
            .strip_prefix(PREFIX)
            .ok_or(FingerprintError::MissingPrefix)?;
        if hex_part.len() != HEX_LEN {
            return Err(FingerprintError::BadLength {
                expected: HEX_LEN,
                found: hex_part.len(),
            });
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
    /// Render the fingerprint in its canonical `fp:<hex>` form.
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
    // A zero source rate is meaningless; treat it (and the no-op case) as identity
    // rather than dividing by zero and allocating an unbounded buffer.
    if from == 0 || from == to || samples.is_empty() {
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
    for (b, slot) in ranges.iter_mut().enumerate() {
        // Logarithmic edges between F_LO and F_HI.
        let lo = F_LO * (F_HI / F_LO).powf(b as f32 / BANDS as f32);
        let hi = F_LO * (F_HI / F_LO).powf((b + 1) as f32 / BANDS as f32);
        let (mut a, mut c) = (bin(lo), bin(hi).max(bin(lo) + 1));
        c = c.min(FRAME / 2);
        a = a.min(c.saturating_sub(1));
        *slot = (a, c);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Generate `secs` of a mono sine wave at `freq` Hz, amplitude `amp`.
    fn tone(freq: f32, amp: f32, secs: f32, sr: u32) -> Vec<f32> {
        let n = (secs * sr as f32) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    /// The same audio yields the same fingerprint (determinism).
    #[test]
    fn compute_is_deterministic() {
        let a = tone(440.0, 0.8, 3.0, 11025);
        assert_eq!(
            Fingerprint::compute(&a, 11025),
            Fingerprint::compute(&a, 11025)
        );
    }

    /// Gain invariance: halving the amplitude barely changes the fingerprint
    /// (Haitsma-Kalker bits are signs of energy *differences*, so gain cancels).
    #[test]
    fn robust_to_volume_change() {
        let loud = tone(440.0, 0.9, 3.0, 11025);
        let quiet = tone(440.0, 0.45, 3.0, 11025);
        let sim = compare(
            &Fingerprint::compute(&loud, 11025),
            &Fingerprint::compute(&quiet, 11025),
        );
        assert!(
            sim > 0.95,
            "gain change should preserve the fingerprint, got {sim}"
        );
    }

    /// Distinct audio is far apart (well below a match threshold).
    #[test]
    fn distinct_audio_differs() {
        let a = tone(440.0, 0.8, 3.0, 11025);
        let b = tone(1200.0, 0.8, 3.0, 11025);
        let sim = compare(
            &Fingerprint::compute(&a, 11025),
            &Fingerprint::compute(&b, 11025),
        );
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

    /// A zero sample rate is handled gracefully (no panic / unbounded allocation).
    #[test]
    fn zero_sample_rate_does_not_panic() {
        let _ = Fingerprint::compute(&tone(440.0, 0.8, 1.0, 11025), 0);
    }
}
