<!-- File: docs/specs/anti_fraud_bandwidth_receipt_protocol.md -->

# Clean Web Economy

## Anti-Fraud & Bandwidth Receipt Protocol

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Anti-Fraud & Bandwidth Receipt Protocol (AFBRP)** for the Clean Web Economy (CWE). The purpose is to ensure:

* **Usage inflation becomes expensive**
* **Fake view events cannot be cheaply generated**
* **High-value content access is backed by verifiable bandwidth activity**
* **Nodes contributing bandwidth/storage receive credit**
* **Privacy remains fully preserved**

AFBRP complements, but does not replace, the ZK Usage Proof Requirements. It adds a layer of **economic and cryptographic pressure** that makes cheating unprofitable.

---

## 2. Threat Model

AFBRP protects against:

### 2.1 Local Inflation Attacks

Adversaries attempt to:

* Fake many usage events for their own works
* Inflate payouts to puppet creators
* Generate artificial traffic without actual data transfer

### 2.2 Collusion Attacks

Groups of nodes attempt to:

* Feed each other fabricated bandwidth receipts
* Create fake swarms that resemble legitimate traffic

### 2.3 Replay Attacks

Attackers replay old receipts to:

* Fake repeated views/downloads
* Bypass bandwidth requirements

### 2.4 Bandwidth-Free Consumption Claims

Clients claim to have consumed content while never downloading or exchanging data.

AFBRP is designed to make these attacks **detectable, costly, or unscalable**.

---

## 3. Key Principles

### 3.1 Zero Knowledge Still Holds

AFBRP MUST NOT:

* Reveal which works a user accessed
* Reveal who downloaded from whom
* Enable reconstruction of usage patterns

### 3.2 Receipts Are Aggregate-Only

Aggregators MUST only see **anonymous bandwidth receipts** that prove:

* Data moved
* Parties were eligible swarm peers
* No details about which content or which user

### 3.3 Cost-Proportional Integrity

Fake traffic MUST cost real:

* Bandwidth
* Storage
* Time

### 3.4 No User Tracking

Receipts MUST NOT:

* Include IP addresses
* Include long-lived node IDs
* Correlate across epochs

---

## 4. Receipt Construction

When a user (consumer) downloads a piece of encrypted content from a peer (storage node or relay), the two parties generate a **shared bandwidth receipt**.

### 4.1 Ephemeral Node Keys

Both peers derive ephemeral session keys:

```
PK_A, SK_A
PK_B, SK_B
```

Keys MUST:

* Be freshly generated per-transfer
* Not identify long-term node IDs
* Not be linkable across transfers

### 4.2 Receipt Core

For each transfer chunk, both peers compute:

```
R = Com( bytes_transferred , session_nonce , randomness )
```

This commitment hides:

* Chunks
* Amounts
* Timing

### 4.3 Mutual Signatures

Both peers sign:

```
SIG_A_over_R
SIG_B_over_R
```

This proves:

* Two distinct entities agree data moved
* Neither party can fabricate a unilateral receipt

### 4.4 Receipt Bundle

At epoch end, the client submits a **receipt bundle**:

```
{ R_i , SIG_peer_i } + Proof_ZK_bandwidth
```

Peer identities are pseudonymized and ephemeral.

---

## 5. Zero-Knowledge Bandwidth Proof

The client generates a ZK proof that shows:

### Statements Proven:

1. **At least X bytes were transferred** for eligible content.
2. All receipts `R_i` correspond to mutually signed commitments.
3. Receipts belong to the correct epoch.
4. No receipt is double-counted.
5. All receipts relate to content for which the user had valid tier access.
6. No information about peers or content is leaked.

ZK circuits MUST NOT reveal:

* Which receipts correspond to which works
* Which peers were involved
* Total per-work bandwidth
* Timing or ordering

---

## 6. Anti-Collusion Measures

### 6.1 Random Swarm Peering

Swarm routing SHOULD:

* Randomize peers
* Prefer geographically diverse nodes
* Rotate swarm participants per epoch

Colluding peers will find it difficult to simulate diversity.

### 6.2 Minimum Peer Diversity Requirement

ZK proof MUST include:

* A proof that receipts originate from **at least D distinct peers**.
* Distinctness proven inside ZK WITHOUT revealing peer identities.

### 6.3 Stake-Based Storage Nodes

Optional governance mechanism:

* Storage nodes may post stake
* Misbehavior (fraudulent receipts) may result in slashing

### 6.4 Rate-Limited Receipts

There MUST be an upper bound of receipts per second from a single peer session.
Prevents scripted inflation.

---

## 7. Anti-Replay Protection

Each receipt includes:

```
epoch_number
session_nonce
chunk_nonce
```

Receipts MUST be rejected if:

* Epoch does not match
* Session nonce is reused
* Chunk nonce repeats

These are proven inside ZK.

---

## 8. Integration with DAPR

Bandwidth receipts integrate with DAPR as **usage credibility multipliers**.

### 8.1 Diminishing Returns Model

For each user:

* The first bandwidth-proven access has full weight
* Replays reduce weight exponentially

### 8.2 Sybil Resistance

Aggregators use:

* Anonymous receipt diversity
* Bandwidth magnitude
* ZK monotonicity proofs

to identify unlikely patterns.

### 8.3 Creator Payout Weighting

Creators with:

* Higher bandwidth demand
* Wider swarm participation

receive proportionally adjusted payouts.

No per-user detail is needed.

---

## 9. Privacy Guarantees

AFBRP MUST ensure:

* No actor can link receipts to specific users
* No actor can infer content consumed
* No actor can reconstruct peer networks
* Storage nodes cannot profile consumers
* Microservices cannot profile consumers
* Aggregators only see anonymous aggregated proofs

If a malicious microservice and malicious peer collude, they still cannot:

* Identify user
* Identify content
* Fake bandwidth receipts without actual data transfer

---

## 10. Deployment Requirements

### 10.1 Reproducible Builds

All official swarm nodes MUST support reproducibility.

### 10.2 Key Rotation

Ephemeral keys MUST rotate:

* Per session
* Per peer
* Per content chunk group

### 10.3 Logging Limitations

Nodes MUST NOT log:

* Long-term peer IDs
* IP addresses tied to content
* Raw receipts

### 10.4 Governance Auditing

DAO SHOULD audit:

* Randomness sources
* Receipt format compliance
* Diversity requirements

---

## 11. Summary

The Anti-Fraud & Bandwidth Receipt Protocol ensures:

* Usage inflation becomes difficult and costly
* Collusion attacks become unscalable
* Zero-knowledge privacy remains intact
* DAPR receives credible, bandwidth-backed signals
* No central or peripheral actor can track users
* No DRM-like behavior is introduced

AFBRP is a **cryptographic-economic shield**, reinforcing CWE’s core goals:

* Privacy
* Fairness
* Decentralization
* Resistance to adversarial pressure

