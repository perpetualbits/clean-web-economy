<!-- File: docs/specs/legal_interoperability_guidelines.md -->

# Clean Web Economy

## Legal Interoperability Guidelines

**Version:** Draft v1.0
**Status:** Policy Document (For DAO & Legal Working Group Review)

---

## 1. Purpose

The Clean Web Economy (CWE) aims to operate across dozens of jurisdictions, from open societies to restrictive environments. This document defines the **legal interoperability requirements and recommended practices** for:

* Creators
* Storage node operators
* DMF operators
* Client implementers
* Aggregators and federation participants

The purpose is to ensure compliance with international IP law, privacy law, consumer protection law, and platform liability frameworks, while preserving the openness and decentralization at the heart of CWE.

CWE is deliberately constructed to avoid the recurring legal pitfalls of:

* DRM systems
* User surveillance
* Ad-based data extraction
* Centralized copyright enforcement
* Jurisdiction-dependent censorship

---

## 2. Core Principles

### 2.1 Creator Rights Are Paramount

CWE guarantees that misattribution, plagiarism, and revenue theft are cryptographically prevented.
Creators retain:

* Ownership
* Attribution
* Revenue rights
* Royalty splits
* Control over metadata and manifests

### 2.2 User Privacy Is Non-Negotiable

Legal demands for surveillance, logs, or identification cannot be satisfied, because:

* Clients do not generate identifying data
* DMF nodes do not track users
* Storage nodes cannot see content usage

### 2.3 No DRM, No Device Attestation

CWE explicitly prohibits:

* DRM
* Hardware binding
* Secure enclave enforcement
* HDCP-style controls
* Device fingerprinting

This principle aligns with consumer protection frameworks and mitigates liability risks.

### 2.4 Decentralized Storage Without Liability Burden

Storage nodes store **encrypted, non-identifiable binary chunks**.
They cannot:

* Identify content
* Control usage
* Perform censorship

This preserves safe-harbor protections similar to those for ISPs and decentralized networks.

### 2.5 Legal Neutrality of the Network

DMF nodes and storage operators do not curate content.
Content discovery is decentralized, and decisions are governed by DAO policy, not per-node discretion.

---

## 3. Interoperability With Copyright Legislation

### 3.1 Strong Attribution & Anti-Plagiarism

CWE provides:

* Fingerprint-based attribution
* Canonical manifests
* Version tracking
* Immutable royalty splits

These satisfy moral rights and attribution requirements under EU, US, and international copyright law.

### 3.2 Private Copying & Distribution

CWE **does not distribute unencrypted files**.
Storage nodes only serve:

* Encrypted chunks
* Without any metadata
* Without visibility into content

This preserves compliance with:

* EU InfoSoc Directive
* DMCA safe harbors
* WIPO treaties

### 3.3 Purchased Content and NFTs

Direct purchases are treated as licensed transfers.
Creators may choose:

* Non-transferable personal license
* Fully transferable ownership
* Resale royalties (where legal)

NFT-like rights tokens avoid DRM restrictions by storing rights on-chain, not enforcing device locks.

---

## 4. Interoperability With Privacy Legislation

### 4.1 GDPR Compliance

CWE is designed so **personal data cannot be collected**, thus avoiding triggering GDPR obligations.

CWE clients:

* Do not store identifiable logs
* Do not transmit personal data
* Do not generate device identifiers

DMF nodes:

* Are stateless
* Do not set cookies
* Do not track IPs beyond short-lived rate limits

### 4.2 CCPA / CPRA Compliance

No sale of personal data, no behavioral profiling, no advertising — therefore CWE is naturally compliant.

### 4.3 ePrivacy

CWE clients do not set cookies or persistent identifiers; DMF nodes do not either.

### 4.4 Data Residency

Content is encrypted at rest; residency constraints apply only to node operators’ hardware.

---

## 5. DMF Operator Guidelines

### 5.1 Transparency Requirements

Operators must publish:

* Node manifest
* Operator identity (optional pseudonymous DID)
* Service schema
* Security contact

### 5.2 Liability Protection

Operators are protected because:

* They do not know what data they store or serve
* They cannot inspect usage patterns
* Content is encrypted and unidentifiable
* They cannot perform DMCA-like takedowns

Primary responsibility lies with creators who publish manifests.

### 5.3 Jurisdictional Resilience

Operators may:

* Host in privacy-friendly jurisdictions
* Run nodes over Tor
* Mirror content across borders

No operator holds unique or irreplaceable data.

---

## 6. Creator Guidelines

### 6.1 Manifest Signing Legal Status

A signed manifest legally corresponds to:

* A public declaration of authorship
* A rights grant to the network
* A royalty split definition binding by contract

### 6.2 Content Licensing Options

Creators may choose:

* All rights reserved
* Creative Commons variants
* Commercial-only licenses
* Public domain

Choice encoded directly into manifest.

### 6.3 Handling Disputes

CWE provides:

* Arbitration service
* DAO governance process
* Fingerprint evidence archive

These form a structured, transparent dispute resolution path.

---

## 7. Client Developer Guidelines

### 7.1 Avoid Legal Risky Features

Client implementations must NOT:

* Implement DRM
* Perform geoblocking
* Log identifiable user data

### 7.2 Respect Contractual Rules

Clients must:

* Respect manifest-defined usage terms
* Only submit valid commitments
* Honor creator metadata

### 7.3 App Store Compliance

Client builds may:

* Avoid platforms which require DRM attestation
* Distribute via F-Droid or direct install
* Use browser extension signing where required without restricting alternative builds

---

## 8. Anti-Piracy Posture

CWE is **anti-piracy by design**, offering:

* Fingerprinting that redirects usage credit
* Usage accounting even for unsigned files
* Elimination of revenue loss from redistribution

This model meets legal expectations for:

* Rights protection
* Revenue fairness

Without enforcing control over user devices.

---

## 9. Censorship Resistance & Legal Pressure

### 9.1 No Central Authority to Pressure

Law enforcement or governments cannot demand:

* Deplatforming specific content
* Backdoors
* User logs
* IP-level tracking

### 9.2 Mirrors & Federated Discovery

discovery is distributed across many DMF nodes, preventing centralized filtering.

### 9.3 Governance Controls

DAO may de-list illegal content **only via due process**, such as:

* Clear illegality (e.g., abuse material)
* Court orders that apply internationally
* Transparent governance vote

---

## 10. Summary

CWE achieves global legal interoperability by:

* Eliminating personal data collection
* Cryptographically preserving rights attribution
* Avoiding DRM and device-control obligations
* Structuring storage for safe-harbor compliance
* Allowing creators to define rights unambiguously via manifests
* Providing decentralized, transparent governance

The system is engineered to satisfy the broadest spectrum of international legal frameworks while protecting creators, users, and operators from unreasonable liability or coercion.

