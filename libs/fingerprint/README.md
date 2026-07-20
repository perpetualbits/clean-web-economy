# cwe-fingerprint

Work-fingerprint library for the Clean Web Economy.

A *fingerprint* is a stable, opaque identifier for a piece of media, rendered as
`fp:<64 hex>` (a 256-bit digest). The rest of the system uses it to recognise a
work, credit the right creator, and anchor usage commitments.

## ⚠️ Phase 1 is a stub

This crate currently computes the fingerprint as the **SHA-256 of the raw sample
bytes** (plan decision D3). That is a *cryptographic* hash, **not** a perceptual
one:

- Identical bytes → identical fingerprint (fully deterministic).
- A re-encode, trim, or added noise → a **different** fingerprint, even though a
  human hears the same track.

The production system needs a *perceptual* fingerprint that survives recompression,
format changes, and noise (see `docs/specs/fingerprinting_specification.md` §5).
That work is Phase 2, tracked in `docs/issues/004-fingerprint-tests.md`. When it
lands it replaces only the internals of this crate — the API below is unchanged.

## API

```rust
use cwe_fingerprint::{Fingerprint, compare};

// From raw sample bytes:
let fp = Fingerprint::compute(sample_bytes);
// Or from decoded f32 audio (endian-stable):
let fp = Fingerprint::compute_f32(&samples);

println!("{fp}");           // -> fp:9f86d0...
let hex = fp.to_hex();      // 64 hex chars, no prefix
let bytes = fp.as_bytes();  // &[u8; 32], e.g. for a commitment

// Parse the canonical textual form back (strict validation):
let parsed = Fingerprint::parse("fp:e3b0c4...").unwrap();

// Similarity in [0.0, 1.0]; the Phase 1 stub is binary (1.0 equal / 0.0 not):
let sim = compare(&a, &b);
```

## Test

```sh
cargo test -p cwe-fingerprint
```
