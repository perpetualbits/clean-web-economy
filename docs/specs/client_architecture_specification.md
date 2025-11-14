# Clean Web Economy

## Client Architecture Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Overview

This specification defines the **architecture, responsibilities, security model, and interfaces** of Clean Web Economy (CWE) clients, including:

* Browser extensions
* Media player plugins
* Desktop/mobile applications
* Command-line tools
* Third-party or community implementations

The document formalizes what a CWE client **must**, **should**, and **must not** do.
It is written to ensure:

* Security in the presence of **untrusted clients**
* Privacy of users
* Openness to third-party implementations
* Protection against doctored or malicious plugins
* Resistance to centralization and DRM-style enclosure

---

## 2. Core Principles

### 2.1 Open Participation

Any party may implement a CWE client. No vendor, operating system, browser store, or hardware TEE may be required for full participation.

### 2.2 Cryptographic Enforcement

The protocol MUST be secure even when the client implementation is hostile or modified. Signing, packaging, and distribution channels improve safety but are not part of the trust assumptions.

### 2.3 Privacy by Design

Clients MUST NOT create or expose behavioral logs. All usage metrics MUST remain local and only be disclosed through zero-knowledge proofs.

### 2.4 User Autonomy

Users may run official or unofficial clients. Clients may be signed for user safety, but signing MUST NOT be required for network participation.

---

## 3. Architectural Layers

CWE clients consist of the following conceptual modules:

```
+------------------------------------------------------+
|  Application Interface (UI/UX, player integration)   |
+------------------------------------------------------+
|  Client Runtime (control flow, plugin core)          |
+------------------------------------------------------+
|  Local Accounting Engine (usage events, commitments) |
+------------------------------------------------------+
|  ZK Proving Engine (SNARK/STARK circuits)            |
+------------------------------------------------------+
|  Cryptographic Services (signatures, keys, hashes)   |
+------------------------------------------------------+
|  Networking Layer (swarm, storage, microservices)    |
+------------------------------------------------------+
|  Persistence (local encrypted state, caches)         |
+------------------------------------------------------+
```

Each module is described below.

---

## 4. Module Responsibilities

### 4.1 Application Interface

**Responsibilities:**

* Provide playback controls, search, creator pages, downloads, reading interface, etc.
* Integrate with browser or OS media player APIs.
* Expose local tier status without revealing personal usage.
* Display verification results for content signatures and manifests.

**Security Requirements:**

* MUST sandbox network content from arbitrary code execution.
* MUST NOT provide APIs for arbitrary script injection.
* MAY be signed by OS or browser vendors for safe distribution.
* MUST NOT assume that signing prevents tampering at the protocol level.

---

### 4.2 Client Runtime

**Responsibilities:**

* Manage session state, user identity keypairs, and ephemeral session keys.
* Coordinate fetching encrypted content and acquiring decryption tokens.
* Handle P2P handshake (tier capability intersection).

**Security Requirements:**

* MUST NOT log cleartext content identifiers tied to user identity.
* MUST NOT transmit raw usage events.
* MUST enforce the protocol-defined handshake rules.

---

### 4.3 Local Accounting Engine

**Responsibilities:**

* Track local user consumption events (views, listens, reads, partial rotations).
* Maintain per-work sequence numbers.
* Generate event commitments:
  `C_w,j = Com(work_id, seq_w,j, randomness)`
* Maintain local encrypted storage of the commitments.

**Security Requirements:**

* MUST keep commitments strictly local.
* MUST enforce monotonicity of per-work sequence numbers.
* MUST be able to produce proof inputs without exposing raw usage.
* MUST support pruning based on epoch expiration.

---

### 4.4 ZK Proving Engine

**Responsibilities:**

* Implement the standardized zk-circuits required by the CWE protocol:

  * Correctness of commitments
  * Valid creator signatures
  * Event uniqueness per epoch
  * Tier eligibility proofs
  * Aggregation proofs over pseudonymized event tags
* Produce proofs that are verifiable by the Chain Layer or the DMF.

**Security Requirements:**

* MUST ensure ZK circuits do not leak input data.
* MUST use domain parameters approved by the DAO (curve, field, hash, etc.).
* MUST support proof batching and recursion for mobile devices.
* MUST be open to alternate proving systems (SNARK/STARK) as long as they verify equivalently.

---

### 4.5 Cryptographic Services

**Responsibilities:**

* Hold user identity keys (signing, encryption, ZK identity).
* Derive per-epoch pseudonyms:
  `P_w,j = H(K_epoch || C_w,j)`
* Handle content decryption keys (`K_content`) provided by Access Microservices.
* Verify creator signatures and manifest signatures.

**Security Requirements:**

* MUST isolate long-term identity keys from application scripting environments.
* MUST support user-chosen key storage options (plain FS, OS keystore, TEE).
* MUST NOT depend on TEE availability.

---

### 4.6 Networking Layer

**Responsibilities:**

* Communicate with:

  * Distributed storage nodes (swarm)
  * Access microservices (for key wrapping)
  * Tier beacon contract / RPC nodes
  * Discovery Layer
* Participate in storage/bandwidth receipt protocols.

**Security Requirements:**

* MUST NOT reveal or infer user identity over P2P connections.
* MUST NOT leak local sequence numbers or commitments.
* MUST verify all received signatures and proofs.

---

### 4.7 Persistence Layer

**Responsibilities:**

* Store:

  * Commitments
  * Encrypted content fragments
  * Cached manifests
  * Pending proof bundles
* All storage MUST be encrypted at rest.

**Security Requirements:**

* MUST allow pruning of old epochs.
* MUST maintain forward secrecy for ephemeral keys.
* MAY store data in browser storage, sandboxed FS, or mobile keychain.

---

## 5. Client Execution Trust Model

### 5.1 Assumed Threats

CWE assumes:

* Attackers will distribute doctored plugins/extensions.
* Malware may intercept or modify client logic.
* Users may run modified or unverified clients.
* Third parties may attempt inflation or bot attacks.
* Some OS/browser vendors may be hostile or compromised.

### 5.2 What the Protocol MUST Withstand

CWE protocols MUST remain correct even if:

* A client is unsigned
* A client is running under debugger
* A client is intentionally modified
* A client injects fake UI or fake usage
* A client refuses to run honest accounting logic

**No security guarantee may depend on client-side code integrity.**

---

## 6. Signing & Distribution Rules

### 6.1 Official Client Builds

Official clients:

* MUST be signed by DAO-controlled keys or delegated maintainers.
* MUST be reproducible builds when possible.
* MUST be available in package repositories and extension stores.

### 6.2 Third-Party Clients

Third-party clients:

* MAY be signed by their authors.
* MUST NOT be excluded from network participation.
* MUST follow protocol rules or their proofs will be rejected.

### 6.3 Preventing Doctored / Malicious Plugins

The CWE addresses this threat through:

1. **Signed official builds** (protects most users)
2. **Clear trust messages** in UI ("Verified implementation", "Community build")
3. **Mandatory cryptographic checks**
4. **Proof-based accounting**
5. **Zero reliance on execution integrity**

**Even if a malicious plugin runs, it cannot:**

* Forge creator signatures
* Fake zk-proofs
* Double-count events
* Claim ownership of works
* Redirect payouts for registered content
* Compromise user anonymity
* Violate the No-DRM Clause

---

## 7. Mandatory Client Behaviors

A compliant client MUST:

* Produce zk-proofs exactly conforming to circuit definitions.
* Handle tier eligibility via ZK membership proofs.
* Fetch encrypted content and unwrap decryption keys properly.


