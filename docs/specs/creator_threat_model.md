<!-- File: docs/specs/creator_threat_model.md -->

# Clean Web Economy

## Creator Threat Model

**Version:** Draft v1.0
**Status:** Security Document (For DAO Review)

---

## 1. Purpose

This document defines the **Creator Threat Model** for the Clean Web Economy (CWE).
Creators are central to CWE’s mission, but they face adversaries on multiple fronts:

* Theft or plagiarism of works
* Revenue hijacking
* Manipulation of fingerprint similarity
* Malicious uploads or impersonation
* Incorrect attribution or split fraud
* Sabotage of reputation or metadata
* Harassment or targeted censorship

The CWE must protect creators while preserving full openness, privacy, and decentralization.

---

## 2. Trust Assumptions

### 2.1 The Network is *not* fully trusted

Creators must assume:

* DMF nodes can be malicious
* Discovery servers may host incorrect or outdated metadata
* Storage nodes may mis-serve or withhold chunks

### 2.2 Users are honest about consumption but not about redistribution

Creators must assume:

* Users may redistribute files
* Users may re-encode content
* Users may strip metadata

CWE design ensures creators still receive correct revenue.

### 2.3 Blockchain Contracts *are* trusted

Smart contracts:

* Enforce payout splits
* Enforce fingerprint uniqueness constraints
* Provide canonical manifest verification

### 2.4 Governance strives to be neutral

DAO aims to:

* Protect against manipulation
* Preserve speech rights
* Avoid centralized moderation

---

## 3. Assets to Protect

### 3.1 Creator Intellectual Property

* Audio, video, text, image works
* Derivative rights
* Collaborator structures

### 3.2 Revenue Integrity

* Usage-based payouts
* Direct purchase royalties
* NFT sale royalties

### 3.3 Attribution Authority

* Fingerprint ownership
* Manifest signatures
* Public profile reputation

### 3.4 Creator Privacy

Creators may:

* Use pseudonyms
* Use decentralized IDs
* Publish without exposing personal data

---

## 4. Adversary Classes

### 4.1 Plagiarists / Content Thieves

Goals:

* Upload stolen works
* Hijack usage-based revenue
* Manipulate fingerprint similarity

### 4.2 Malicious Competitors

Goals:

* Spam metadata under a competitor’s name
* Upload confusingly similar works
* Poison search results

### 4.3 Rogue Discovery Nodes

Goals:

* Manipulate ranking
* Downrank or shadowban creators
* Inject bias into search results

### 4.4 Malicious Storage Nodes

Goals:

* Withhold chunks
* Serve corrupted content
* Attempt fingerprint-based censorship

### 4.5 Malicious Collaborators

Goals:

* Misrepresent their role
* Inflate their share
* Hijack royalty flow

### 4.6 Harassment Groups / State Actors

Goals:

* Target creators for censorship
* Pressure DMF nodes to delist content
* Attempt legal coercion

---

## 5. Threats and Mitigations

### 5.1 Content Theft / Unauthorized Uploads

**Threat:** Adversary uploads stolen work to earn revenue.

**Mitigations:**

* Strong perceptual fingerprinting
* Near-duplicate detection
* Client redirects usage credit to rightful manifest
* Signed creator manifests
* Discovery flags plagiarized content

### 5.2 Revenue Hijacking

**Threat:** Plagiarist steals usage-based or direct-purchase revenue.

**Mitigations:**

* Canonical manifest enforced by fingerprint
* Payout splits tied to manifest signatures
* Rollup-linked attribution

### 5.3 Fingerprint Collision Attacks

**Threat:** Malicious uploader tries to produce a fingerprint collision.

**Mitigations:**

* Multi-modal fingerprinting (audio + video + text + image)
* Hash strengthening (Poseidon/SHA-3/Blake3)
* Optional 512‑bit high-security mode

### 5.4 Metadata Poisoning

**Threat:** Malicious competitor publishes misleading metadata.

**Mitigations:**

* Only creator-signed manifests are authoritative
* Discovery nodes validate signatures
* DMF rejects unsigned manifests

### 5.5 Version Confusion Attacks

**Threat:** Adversary uploads slightly modified versions to confuse ranking.

**Mitigations:**

* Manifest versioning rules
* Discovery canonicalization
* Optional grouping of variants under primary fingerprint

### 5.6 Collaborator Fraud

**Threat:** Collaborator claims incorrect roles or demands shares retroactively.

**Mitigations:**

* Immutable split structure bound to manifest
* Arbitration service for disputes
* DAO review for complex cases

### 5.7 Revenue Manipulation by Nodes

**Threat:** DMF node drops attribution events or inflates others.

**Mitigations:**

* Rollups verify aggregation in zero-knowledge
* Ingress nodes validate all proofs
* Settlement contract rejects malformed proofs

### 5.8 Unwanted Redistribution (Piracy)

**Threat:** Files are shared without permission.

**Mitigations:**

* Fingerprint redirection ensures creators still get paid
* No DRM needed
* No dependency on signed binaries

### 5.9 Harassment & Targeted Censorship

**Threat:** Actors attempt to deplatform creators.

**Mitigations:**

* Federated discovery (no central server)
* Multiple DMF mirrors
* No-DRM governance clause
* Optional pseudonymity

### 5.10 Legal Overreach

**Threat:** A state actor demands backdoors or surveillance.

**Mitigations:**

* No centralized control point
* No identifying user data exists to be handed over
* Governance rules ban attestation/DRM

---

## 6. Residual Risks

### 6.1 False Positives in Fingerprinting

Rare but possible; mitigated by:

* Appeals process
* Arbitration service

### 6.2 Coordinated Harassment Off-Platform

CWE cannot prevent external harassment campaigns.

### 6.3 State-Level Censorship

Nation-states may block DMF nodes; mitigated by:

* Mirrors
* Tor and VPN support
* Peer-to-peer fallback

### 6.4 Collaborator Relationship Complexity

Disputes may require human arbitration; cryptography cannot solve interpersonal conflicts.

---

## 7. Summary

The CWE protects creators through:

* Strong fingerprinting
* Signed manifests
* Immutable payout splits
* Privacy-preserving usage accounting
* Federated discovery
* ZK‑verified rollups

Adversaries can attack metadata, storage, discovery, or collaborators — but cannot:

* Steal revenue
* Override manifest signatures
* Manipulate payouts
* Hijack fingerprints
* Track users

CWE gives creators unprecedented protection in a fully decentralized, privacy-first economic ecosystem.

