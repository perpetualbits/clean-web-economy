<!-- File: docs/specs/fingerprinting_specification.md -->

# Clean Web Economy

## Fingerprinting Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Perceptual Fingerprinting System** used throughout the Clean Web Economy (CWE) to:

* Detect duplicate or near-duplicate works
* Prevent creators from uploading stolen content
* Resolve equivalence between signed and unsigned media
* Provide robust identity for content regardless of container format
* Support ZK usage proofs and DAPR aggregation
* Anchor discovery, trending, and anti-fraud systems

The fingerprint must:

* Survive format shifts, compression, trimming, re-encoding, scaling, and noise
* Be content-dependent but container-independent
* Never leak user identity
* Never embed watermarks or DRM

---

## 2. Design Principles

### 2.1 Content-Intrinsic

Fingerprint MUST depend only on the underlying perceptual content, not metadata.

### 2.2 Collision-Resistant (Practically)

Distinct works must not collide except under adversarial conditions requiring major effort.

### 2.3 Format-Independent

Audio/video/text/images must yield the same fingerprint even after:

* Recompression
* Format conversion
* Scaling/cropping (within limits)
* Loudness or color adjustment

### 2.4 Non-Reversible

Fingerprints MUST NOT:

* Allow reconstruction of the original content
* Leak semantic content
* Serve as an ownership watermark

### 2.5 ZK-Friendly

Fingerprints MUST be short enough to embed into circuits and commitments.

### 2.6 Multi-Modal

Supports:

* Audio
* Video
* Images
* Text
* Mixed media

---

## 3. High-Level Architecture

The fingerprint is computed by a **Three-Layer Pipeline**:

1. **Canonicalization Layer** – normalize the content
2. **Feature Extraction Layer** – modality-specific perceptual features
3. **Hashing & Binning Layer** – produce fixed-length content hash

```
Content → Canonicalization → Feature Extractor → Perceptual Hash → F_w
```

Where `F_w` is the final fingerprint.

---

## 4. Fingerprint Format

Final fingerprint format:

```
F_w = H( feature_vector )
```

Where:

* `feature_vector` = modality-specific signature
* `H()` = Poseidon, SHA-3, or Blake3 (governance defined)

Length:

* 256 bits (default)
* Optional 512 bits for high-stakes media (films, AAA releases)

---

## 5. Modality-Specific Methods

### 5.1 Audio Fingerprint

Based on stable spectrogram features.

Pipeline:

* Resample audio to canonical rate (e.g., 8–12 kHz mono)
* Compute log-frequency spectrogram
* Extract landmark constellation (Shazam-like) or robust hashes (Chromaprint-like)
* Reduce to binary signature (~512–2048 bits)
* Hash to 256-bit fingerprint

Resilient to:

* MP3/AAC recompression
* Noise injection
* EQ and filtering

### 5.2 Video Fingerprint

Pipeline:

* Extract keyframes evenly (e.g., 1 per second)
* Convert to luminance-only
* Compute blur-resistant DCT or wavelet features
* Combine with audio fingerprint
* Hash to 256-bit fingerprint

Resilient to:

* Re-encoding (H264/H265/AV1)
* Scaling
* Letterboxing

### 5.3 Image Fingerprint

Pipeline:

* Normalize to fixed dimensions
* Apply perceptual hash (pHash or wavelet hash)
* Hash output to 256-bit

Resilient to:

* Resolution changes
* Color changes
* Cropping

### 5.4 Text Fingerprint

Pipeline:

* Normalize Unicode
* Strip whitespace and formatting
* Compute rolling n‑gram or MinHash sketch
* Hash to 256-bit fingerprint

Resilient to:

* Reformatting
* Minor edits
* PDF → TXT extraction differences

### 5.5 Mixed Media

For interactive works, games, or scientific datasets:

* Combine modality-specific fingerprints into a multi-hash structure
* Hash the concatenation to 256-bit

---

## 6. Similarity Metrics & Thresholding

To detect duplicates or derivatives, nodes compute:

```
sim = similarity( F_w1, F_w2 )
```

Where similarity uses:

* Hamming distance (binary signatures before hashing)
* L2 distance on feature vectors (optional)
* Multi-modal consensus score

Governance defines thresholds:

* `sim > T_duplicate` → treated as same work
* `sim between T_related_low and T_related_high` → derivative/cover/remix
* `sim < T_unrelated` → unrelated

---

## 7. Chain Integration

Chain contracts store **only the final fingerprint hash**:

```
fingerprint = H256
```

No modality-specific features appear on-chain.

Used for:

* Registration uniqueness checks
* Duplicate detection
* Dispute resolution

---

## 8. Discovery Layer Integration

Discovery uses fingerprints for:

* Deduplication of search results
* Flagging plagiarized content
* Grouping covers, remixes, extended editions
* Trending algorithms (per-fingerprint families)

---

## 9. Anti-Fraud & Anti-Theft Integration

Fingerprinting works with DAPR & ZK proofs to prevent:

* Uploading stolen content and farming views
* Using re-encoded versions to bypass manifests
* Forking files to steal traffic from creators

### 9.1 Handling Unsigned Content

If user views **unsigned content** whose fingerprint matches a registered manifest:

1. **Client detects fingerprint match locally**
2. **Allocates usage credit** to the *signed* version of that work
3. **Generates usage commitment C_w,j** using the manifest that matches the fingerprint
4. Produces ZK proof normally (content ID remains private)

This ensures:

* Creators still get paid
* Users can view arbitrary file formats
* No DRM or signed-only restriction
* Rogue distributions cannot steal value

This **works exactly as you anticipated** — fingerprint matching redirects credit.

---

## 10. ZK Proof Integration

ZK circuits use only:

* Manifest hash
* Fingerprint hash
* Commitment hash

Circuits DO NOT see:

* Content itself
* Feature vectors
* Matching thresholds

---

## 11. Anti-Collusion Defenses

Fingerprinting resists:

### 11.1 Minor Perturbation Attacks

Adversaries cannot:

* Slightly pitch‑shift audio
* Resize video
* Adjust colors

…to avoid fingerprint equivalence.

### 11.2 Format-Spoofing

Changing MP4 → MKV → TS does not change fingerprint.

### 11.3 Re-Encoding

480p → 1080p → AV1/H265 re‑encoding preserves perceptual features.

### 11.4 Content Mixup Attacks

Multi-modal fingerprints detect:

* Replacing audio under video
* Cutting intro/outro
* Substituting scenes

If similarity drops below threshold, relabel as **derivative**, not original.

Governance may define derivative payout rules later.

---

## 12. Governance Controls

Governance sets and adjusts:

* Similarity thresholds
* Feature extractor versions
* Hashing primitives
* Multi-modal fusion rules

Upgrades MUST be:

* Versioned
* Publicly reviewed
* Backward-compatible where possible

---

## 13. Summary

The CWE Fingerprinting System:

* Identifies works reliably
* Enables duplicate detection
* Prevents content theft
* Supports ZK usage accounting
* Empowers discovery without tracking
* Supports unsigned playback while crediting creators
* Preserves privacy and decentralization

Fingerprinting is a core pillar ensuring CWE remains **fair, open, resilient, and adversarially robust**, without resorting to DRM or surveillance.

