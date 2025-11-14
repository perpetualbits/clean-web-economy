<!-- File: docs/specs/governance_no_drm_clause.md -->

# Clean Web Economy

## Governance No‑DRM Clause

**Version:** Draft v1.0
**Status:** Binding Governance Policy (For DAO Ratification)

---

## 1. Purpose

This clause establishes a **strict, binding prohibition** on the introduction of Digital Rights Management (DRM), Trusted Execution–enforced access controls, platform‑level tracking, or any mechanism that restricts user freedom or prevents independent client implementations within the Clean Web Economy (CWE).

The clause:

* Protects user autonomy
* Protects developer freedom
* Prevents ecosystem enclosure
* Prevents the emergence of gatekeepers
* Ensures long‑term openness and verifiability
* Aligns governance with privacy and decentralization principles

It serves as a **constitutional constraint** on all future protocol changes, smart contract upgrades, DAO decisions, and service implementations.

---

## 2. Definitions

### 2.1 DRM (Digital Rights Management)

For the purposes of the CWE, DRM includes **any technical mechanism** that attempts to:

* Prevent users from running their own clients
* Prevent access to content based on device identity
* Bind content access to hardware or proprietary runtime environments
* Enforce playback restrictions based on remote attestation
* Track consumption behavior at the device or OS level
* Introduce tie‑ins between approval keys and proprietary vendors

### 2.2 Trusted Execution Enforcement

The use of Trusted Execution Environments (TEEs) becomes DRM when the system:

* Requires TEEs for content access
* Uses TEEs for user‑identifying attestations
* Uses TEEs to hide logic that affects protocol correctness
* Uses attestation to block alternative clients

### 2.3 Platform Gatekeeping

Any requirement that content can only be consumed if:

* A browser vendor approves the plugin
* An app store approves the binary
* A hardware vendor’s attestation service approves the device

This is prohibited.

---

## 3. Guiding Principles

### 3.1 Open Participation

Any user, developer, or researcher MUST be allowed to:

* Write their own client
* Modify any client
* Inspect any client’s behavior
* Build compatible open‑source or closed‑source implementations

No certification or approval channel may be mandatory.

### 3.2 Cryptographic, Not Platform‑Based, Enforcement

All correctness guarantees MUST be:

* Cryptographic (e.g., ZK proofs, signatures, commitments)
* Verifiable on any conforming implementation

No correctness guarantee may depend on the runtime environment.

### 3.3 User Privacy and Anonymity

No component of the CWE may:

* Track user identity
* Track device identity
* Require persistent identifiers
* Bind usage to hardware IDs or browser profiles

### 3.4 Creator Autonomy

Creators may encrypt content, but MUST NOT:

* Enforce proprietary playback restrictions
* Require proprietary clients
* Upload per‑device encrypted versions

---

## 4. Prohibited Technologies and Practices

The following MUST NOT be introduced into the CWE ecosystem by governance, developers, creators, microservices, or storage providers.

### 4.1 Hardware‑Bound Access Restrictions

Examples:

* Device fingerprinting gates
* Mandatory secure enclave keys
* HDCP‑like encryption link requirements
* TPM attestation as a condition for playback

### 4.2 Binary Approval or Sign‑In Requirements

* "Only binaries signed by X may play content"
* "Only store‑approved apps may run"
* "Only clients with valid vendor attestations may access the system"

### 4.3 Obfuscated, Enforced Client Logic

* Hidden usage counting mechanisms
* Remote‑verifiable playback attestations
* Hidden anti‑tamper logic that affects payouts

### 4.4 Watermarking or Fingerprinting of Users

* Embedding user IDs in responses
* Per‑user encryption
* Per‑device watermarking

### 4.5 Encrypted Execution Policies

* Logic that can only be executed inside a TEE
* Correctness dependent on proprietary enclaves
* TEEs used to prove what content was consumed

---

## 5. Allowed but Optional Mechanisms

Some technologies are allowed **only if they enhance user security** and do not restrict ecosystem openness.

### 5.1 Optional User‑Controlled TEE Storage

Users MAY store:

* Private keys
* Tier membership secrets
  inside TEEs **at their own discretion**.

TEEs MUST NOT:

* Become a requirement for content access
* Influence ZK proofs
* Become part of protocol‑level validation

### 5.2 Signed Official Clients (Optional)

The CWE Foundation MAY sign official builds **for safety and distribution convenience**.

Signing MUST NOT be used to:

* Reject other clients
* Block alternatives
* Assert execution control

### 5.3 Network‑Wide Public Keys (Non‑Private)

Keys used to:

* Sign manifests
* Sign microservice endpoints
* Sign client binaries for distribution

are allowed if they do not restrict client choice.

---

## 6. Binding Governance Rules

These rules are **constitutionally binding** on all evolutions of the CWE protocol.

### 6.1 No Mandatory DRMs

The DAO MUST NOT:

* Approve protocol upgrades that introduce DRM
* Approve smart contracts requiring DRM
* Approve microservices requiring DRM
* Approve governance decisions enabling DRM

### 6.2 No Runtime Attestation Gates

The DAO MUST reject proposals requiring:

* Remote attestation for playback
* Hardware certification checks
* Enclave‑restricted logic

### 6.3 No Enforcement via OS or Browser Vendors

Protocol MUST NOT rely on:

* Chrome Web Store limitations
* Apple/Google/Windows restrictions
* Device vendor permission

### 6.4 No Breaking Alternative Clients

Any protocol change MUST:

* Remain fully implementable by independent developers
* Be interoperable with open‑source clients

### 6.5 Transparency and Public Review

Changes touching:

* Client integrity
* Privacy guarantees
* Signature or verification rules

MUST undergo a minimum review period and cryptographic audit.

---

## 7. Enforcement and Redress

### 7.1 Violating Components

Any component (client, microservice, protocol change) violating this clause MUST be:

* Flagged publicly
* Delisted from official repositories
* Removed from governance reference implementations

### 7.2 Arbitration Rights

Creators, users, or developers may file a formal challenge via:

* Arbitration Service
* Governance Council

### 7.3 Rollback Authority

Governance MUST be able to rollback:

* Contract changes
* Microservice deployments
* Specification amendments

if they violate this clause.

### 7.4 Permanent Prohibition

The No‑DRM Clause can **only** be amended or removed by:

* A supermajority governance vote
* A constitutional‑level multi‑stage referendum

---

## 8. Summary

This Governance No‑DRM Clause ensures that the CWE remains:

* Open
* Privacy‑preserving
* Transparent
* Resistant to platform control
* Future‑proof against enclosure

By forbidding any form of DRM, attestation‑based gating, or platform‑vendor dependence, CWE guarantees:

* User freedom
* Creator autonomy
* Sustainable openness
* Trustworthy, verifiable cryptographic security

The CWE can only succeed globally if **no corporation, government, or hardware vendor** can control how users access culture.

This clause makes that guarantee permanent.

