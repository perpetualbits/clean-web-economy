<!-- File: docs/specs/client_threat_model.md -->

# Clean Web Economy

## Client Threat Model

**Version:** Draft v1.0
**Status:** Security Document (For DAO Review)

---

## 1. Purpose

This document provides a **comprehensive threat model for CWE clients**:

* Browser extension
* Media player plugin
* Mobile/desktop native clients
* Headless or embedded clients (optional)

The goal is to identify and mitigate adversarial attempts to:

* Steal revenue
* Misreport usage
* Compromise user privacy
* Attack creators or collaborators
* Manipulate ranking or payouts

The Client is the most exposed part of CWE and therefore requires the strongest, clearest security model.

---

## 2. Trust Assumptions

### 2.1 The Client is *not* trusted by the Network

CWE must assume users can:

* Modify their client
* Reverse engineer code
* Attempt to fake proofs

**All correctness is cryptographically enforced**, not trusted.

### 2.2 The User is *trusted* with Privacy, but *not* with Integrity

* Users MUST be trusted not to leak their own privacy
* Users MUST NOT be able to fake:

  * Usage data
  * Tier membership
  * Bandwidth receipts
  * Commitments or proofs

### 2.3 Storage Nodes and Discovery Servers are NOT trusted

Client must assume:

* Nodes may be malicious
* Nodes may try to deanonymize users
* Nodes may modify or corrupt chunks

All defenses must be client-side.

---

## 3. Assets to Protect

### 3.1 User Assets

* Anonymity
* Tier Commitment
* Ephemeral secrets (for capability tokens)
* Local content keys
* Local usage records

### 3.2 Creator Assets

* Accurate usage weight
* Accurate fingerprint attribution
* Protection against stolen content uploads

### 3.3 Network Assets

* Integrity of ZK proofs
* Integrity of commitments
* Protection against Sybil inflation
* Clean usage aggregated by DAPR

---

## 4. Adversary Classes

### 4.1 Local Adversary (User or Malware)

Goals:

* Modify client to fake usage
* Extract content keys and redistribute content
* Inject fake collaborator shares
* Submit malformed proofs

### 4.2 Network Adversary (MITM)

Goals:

* Eavesdrop on chunk requests
* Deanonymize user activity
* Modify manifests or serve malicious versions

### 4.3 Malicious Storage Node

Goals:

* Track user requests
* Modify chunks
* Serve corrupt blobs
* Infer content types from access patterns

### 4.4 Malicious Creator

Goals:

* Steal revenue from other creators
* Upload plagiarized content
* Poison fingerprint listings

### 4.5 Malicious Aggregator (DMF Node)

Goals:

* Drop events
* Inflate events
* Forge partial rollups

### 4.6 Large-Scale Institutional Adversary

Goals:

* Mass-track users
* Demand DRM-like behavior
* Attempt to ban or sabotage CWE

---

## 5. Threats and Mitigations

### 5.1 Fake Usage Events

**Threat:** User modifies client to fabricate consumption.

**Mitigations:**

* All usage events require ZK proof of actual playback
* Commitments include K_epoch
* Replay impossible
* Weight extraction validated in rollup

### 5.2 Proof Forgery

**Threat:** User crafts invalid ZK proofs.

**Mitigations:**

* Groth16/STARK verification on ingress
* Malformed proofs rejected instantly

### 5.3 Tier Forgery

**Threat:** User attempts to fake tier membership.

**Mitigations:**

* Blind-signed Tier Commitments
* Tier Capability Tokens bound to K_epoch
* ZK tier proofs optional but recommended

### 5.4 Content Key Leakage

**Threat:** User extracts decryption keys.

**Mitigations:**

* Encrypted content served without identifying information
* Content keys ephemeral and per-session
* No watermarking or DRM required
* Leakage does NOT allow revenue theft thanks to fingerprint mapping

### 5.5 Storage Node Surveillance

**Threat:** Node logs requests and tries to infer content.

**Mitigations:**

* Randomized chunk ordering
* Padding on requests
* Parallel fetch from multiple nodes
* Encryption conceals blob contents
* No manifest-based routing

### 5.6 Corrupted Content Delivery

**Threat:** Node returns wrong or corrupted chunks.

**Mitigations:**

* Client verifies Merkle proofs for each chunk
* Request rerouted automatically
* Node flagged for audit

### 5.7 Discovery Manipulation

**Threat:** Server attempts to track users or bias search.

**Mitigations:**

* Stateless search API
* Differential privacy for rare categories
* No user-level personalization

### 5.8 Client Fingerprinting

**Threat:** Server attempts to identify user by technical metadata.

**Mitigations:**

* Stripped headers
* Randomized timing jitter
* User-Agent normalization
* Mandatory HTTPS

### 5.9 Malicious Extensions or Plugins

**Threat:** Fake browser extensions claiming to be CWE.

**Mitigations:**

* Official signed builds for convenience
* BUT alternative builds fully allowed
* Clients verify manifests and signatures themselves
* No requirement to trust browser vendors

### 5.10 Content Theft Through Re-Uploading

**Threat:** Adversary re-encodes content and uploads as their own.

**Mitigations:**

* Perceptual fingerprint matching
* Near-duplicate detection
* Usage credit redirected to correct creator

### 5.11 Node Collusion

**Threat:** Multiple DMF nodes collude to rewrite rollups.

**Mitigations:**

* ZK rollups independently verifiable on-chain
* Fraud-proof mechanism optional

### 5.12 Government or Corporate Pressure

**Threat:** Demands for DRM, surveillance, device attestation.

**Mitigations:**

* Governance No-DRM Clause (constitutional)
* Clients do not support attestation APIs
* No dependency on hardware enclaves

---

## 6. Residual Risks

### 6.1 User Device Compromise

If malware controls user device:

* Privacy may be compromised
* Usage patterns exposed locally

CWE cannot fully protect against compromised hardware.

### 6.2 Timing Attacks

Highly resourced adversary may correlate chunk timing, though mitigated by jitter.

### 6.3 Fake Creator Uploads

If an adversary produces *genuinely new* content, fingerprint cannot stop them; governance must.

### 6.4 Node Censorship

A region may attempt to block DMF nodes; mitigated by:

* Mirrors
* Tor bridges
* Multi-region routing

---

## 7. Summary

The client is the most adversarial environment in CWE, yet ZK proofs, commitments, capability tokens, and fingerprint matching ensure:

* Usage cannot be faked
* Tiers cannot be forged
* Users cannot be tracked
* Content cannot be stolen
* Storage nodes cannot spy or tamper

CWE’s threat model assumes hostility everywhere except the user’s privacy needs—and designs cryptography to handle the rest.

