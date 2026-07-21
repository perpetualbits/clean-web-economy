# cwe-fingerprint

Work-fingerprint library for the Clean Web Economy.

A *fingerprint* is a stable, opaque identifier for a piece of audio, rendered as
`fp:<256 hex>` (a fixed 1024-bit value). The rest of the system uses it to
recognise a work, credit the right creator, and anchor usage commitments.

## Haitsma-Kalker acoustic fingerprint

This crate computes a real, if modest, *acoustic* fingerprint using the
Haitsma-Kalker scheme:

1. The input audio (decoded `f32` samples, not raw bytes) is resampled to a
   canonical rate.
2. It is split into overlapping frames, and each frame's energy is measured
   across 33 logarithmically-spaced sub-bands between 300 Hz and 2000 Hz.
3. Each of a frame's 32 sub-fingerprint bits is the **sign** of a second-order
   energy difference — across adjacent bands and across time (this frame vs.
   the previous one).
4. 32 frames of 32 bits each are kept, giving a fixed-size **1024-bit**
   fingerprint (`FRAMES = 32`, `BITS_PER_FRAME = 32`).

Because every bit is the sign of an energy *difference*, the fingerprint is
**gain-invariant by construction**: scaling the whole signal's amplitude
scales every energy term by the same factor and leaves the sign of each
difference unchanged. This also gives it some robustness to mild
re-encoding, since the underlying band-energy structure is what changes
least under those transforms. It is intentionally a **fallback** recogniser —
simple, deterministic, and dependency-light — not a production-grade one;
that is future work (Chromaprint/AcoustID-class robustness).

Two fingerprints are compared with [`compare`], the **Hamming similarity**
across all 1024 bits: `1 − (differing bits / total bits)`, in `[0.0, 1.0]`.

## API

```rust
use cwe_fingerprint::{compare, Fingerprint};

// From decoded mono f32 samples at their sample rate:
let fp = Fingerprint::compute(&samples, sample_rate);

println!("{fp}");            // -> fp:9f86d0...  (256 hex chars, 1024 bits)
let hex = fp.to_hex();       // the hex form, no prefix
let id = fp.id();            // [u8; 32] keccak256 of the bits — compact dedup key

// Parse the canonical textual form back (strict validation):
let parsed = Fingerprint::parse(&fp.to_string()).unwrap();

// Hamming similarity in [0.0, 1.0]; self-similarity is always 1.0:
let sim = compare(&fp, &parsed);
```

## Test

```sh
cargo test -p cwe-fingerprint
```
