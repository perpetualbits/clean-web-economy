# Clean Web Economy

## ZK Usage Proof Requirements

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This document defines the **zero-knowledge (ZK) usage-proof system** required for the Clean Web Economy (CWE). It establishes:

* What information must be proven
* What must remain private
* The structure of commitments, pseudonyms, and proofs
* How double-counting is prevented
* How proofs interact with tier membership
* How proofs integrate with the DAPR payout process

The requirements ensure CWE remains:

* **Privacy-preserving** (no global consumption logs)
* **Tamper-resistant** (modified clients cannot break rewards logic)
* **Open** (third-party clients can implement proofs)
* **Decentralized** (no central verifier learns usage patterns)

---

## 2. Cryptographic Goals

A valid ZK usage-proof system MUST provide the following guarantees:

### 2.1 Integrity

* Usage events reported by a client MUST be consistent with valid content, valid creator signatures, and correct sequence ordering.
* A client MUST NOT be able to fabricate usage for works they do not have access to.

### 2.2 Privacy

* The verifier MUST NOT learn:

  * Which works a user consumed
  * How many times a specific work was consumed
  * Per-user usage patterns

### 2.3 No Double-Counting

* A user MUST NOT be able to count the same view event more than once in the same epoch.
* A user MUST NOT be able to replay old usage events across epochs.

### 2.4 Non-Linkability

* Reports across different epochs MUST NOT be linkable.
* Two usage events MUST NOT be linkable to each other or to any work ID.

### 2.5 Epoch Soundness

* Proofs MUST bind usage to a specific epoch.
* Old commitments MUST NOT be re-used in later epochs.

---

## 3. Components of the Usage-Proof System

### 3.1 Per-Work Sequence Numbers

For each work `w`, and each consumption event `j`, the client maintains a monotonically increasing sequence number:

```
seq_w,1 < seq_w,2 < seq_w,3 < ...
```

These MUST:

* Never repeat
* Never decrease
* Not be guessable by external observers

### 3.2 Usage Commitments

Each event produces a Pedersen-style commitment:

```
C_w,j = Com(work_id, seq_w,j, randomness)
```

**Requirements:**

* MUST hide work_id and sequence numbers
* MUST commit to valid and monotonic sequence ordering (enforced via ZK)

### 3.3 Epoch Beacon Key

For each epoch `E`, the chain or DMF publishes:

```
K_epoch
```

Used to generate unlinkable pseudonyms.

### 3.4 Pseudonymization Layer

Each commitment is transformed into an epoch-specific pseudonym:

```
P_w,j = H(K_epoch || C_w,j)
```

**Properties:**

* Deterministic within epoch → duplicate detection
* Unlinkable across epochs
* Reveals nothing about content or user

### 3.5 ZK Proof Bundle

A user submits a **proof bundle**:

```
{ P_w,j }  +  Proof_ZK
```

Where `Proof_ZK` shows:

1. Each pseudonym corresponds to a valid commitment.
2. Each commitment corresponds to a valid usage event.
3. Each usage event corresponds to a signed creator manifest.
4. All commitments are unique within the epoch.
5. All commitments follow correct per-work sequence rules.
6. The user is eligible to consume the content (tier proof).
7. No forbidden information is leaked.

---

## 4. Required ZK Circuits

Clients MUST implement a set of mandatory circuits. These circuits may be implemented with SNARKs, STARKs, or equivalent systems that meet the verification interface.

### 4.1 Commitment Correctness Circuit

Proves knowledge of `(work_id, seq_w,j, randomness)` such that:

```
C_w,j = Com(work_id, seq_w,j, randomness)
```

### 4.2 Manifest Verification Circuit

Proves that:

* `work_id` matches a creator-signed manifest
* The manifest signature is valid
* The verifier does NOT learn which work is referenced

### 4.3 Event Uniqueness Circuit

Ensures **no double-counting**:

```
P_w,j values are all distinct
```

This is checked inside ZK without revealing the list.

### 4.4 Sequence Monotonicity Circuit

For each work:

```
seq_w,1 < seq_w,2 < ... < seq_w,n
```

The verifier learns nothing about the values.

### 4.5 Tier Eligibility Circuit

Proves the user possesses the necessary tier token or membership commitment to consume the work.

### 4.6 Epoch Binding Circuit

Proves that each `P_w,j` corresponds to:

```
H(K_epoch || C_w,j)
```

Without exposing the commitment or hash input.

---

## 5. Data Flow Overview

### 5.1 Local Event Creation

* User consumes content
* Client increments sequence number for that work
* Commitment is created and stored locally

### 5.2 Epoch-End Processing

* Chain publishes `K_epoch`
* Client computes `P_w,j` for each commitment
* Client constructs ZK proof bundle

### 5.3 Submission

* Client sends `{P_w,j}`, `Proof_ZK` to aggregator
* No work IDs or sequence numbers leave the device

### 5.4 Aggregation

* Aggregators deduplicate pseudonyms
* Aggregators apply DAPR logic to aggregate work-level stats
* Chain receives only aggregated, anonymous totals

### 5.5 Payout

* Creators receive payouts proportional to `(usage × creator price)` with no ability to trace back users

---

## 6. Security Requirements

### 6.1 Zero Reliance on Client Integrity

The system MUST assume:

* Clients can be modified
* Clients can be malicious
* Clients can attempt inflation attacks

The ZK proof MUST be the only enforcement mechanism.

### 6.2 Bounded Damage per Identity

The design MUST include:

* Diminishing returns per user per work
* Rate-limiting per epoch
* Optional bandwidth receipts for high-weight events

### 6.3 No Work Leakage

Neither commitments nor proofs may leak:

* Work ID
* Creator identity
* Timestamps
* Sequence counts per work

### 6.4 No User-Linkability

Pseudonyms MUST NOT be linkable:

* Across epochs
* Across devices (if user rotates identity keys)
* Across works

### 6.5 Proof Verifiability

The proof MUST verify in:

* < 300 ms on consumer hardware
* < 40 ms on a rollup prover (for batched verification)

---

## 7. Implementation Profiles

Clients MAY choose from:

### 7.1 Full Prover

* Runs full zk-SNARK/STARK prover locally
* Recommended for desktops, laptops, and powerful mobile devices

### 7.2 Outsourced Prover (Privacy Preserved)

* Client performs blind preprocessing
* Offloads only opaque witness values
* Receives a zero-knowledge proof that reveals nothing

### 7.3 Hybrid Approaches

* Small circuits proven on-device
* Large aggregation circuits proven externally

All MUST preserve privacy and non-linkability.

---

## 8. Interaction With Other CWE Layers

### 8.1 Chain Layer

* Receives aggregated usage totals per work
* Never receives user-specific pseudonyms
* Never stores raw proof bundles

### 8.2 DMF (Distributed Microservice Fabric)

* Can host off-chain provers
* Can assist in proof aggregation

### 8.3 Discovery Layer

* Receives only aggregate usage signals
* Never sees commitments or pseudonyms

### 8.4 Governance

* Reviews and updates circuit designs
* Maintains transparency logs
* Performs reproducibility and security audits

---

## 9. Summary

A ZK usage-proof system for CWE MUST:

* Hide all user behavior
* Permit deduplication of events
* Prevent double-counting
* Bind usage to creator-signed content
* Bind usage to epoch keys
* Support tier-gated content access
* Resist hostile or modified clients
* Produce proofs fast enough for global-scale rollups

The combination of commitments, per-work sequence numbers, epoch keys, pseudonyms, and zk-proofs ensures **privacy-preserving yet verifiable usage accounting**, forming the core of the DAPR payout mechanism.

