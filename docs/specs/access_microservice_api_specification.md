# Clean Web Economy

## Access Microservice API Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

The Access Microservice is the **cryptographic gatekeeper** of the Clean Web Economy (CWE). It provides users with the **content decryption material** needed to access encrypted works, while ensuring:

* **Tier-gated access control**
* **Creator-controlled distribution**
* **No user tracking**
* **No central visibility of what work a user is requesting**
* **No persistent logs**
* **Compatibility with zero-knowledge usage proofs**

This document defines the API, required cryptographic flows, privacy properties, and mandatory behaviors.

---

## 2. Architectural Role

The Access Microservice:

* Receives **tier eligibility proofs**
* Receives a **fresh ephemeral public key** from the client
* Verifies manifest signatures and tier rules
* Returns **encrypted content keys** (`K_content_wrapped`)
* Keeps *no memory* of which user requested which content

It **does not**:

* Decrypt or see any user usage
* Maintain user sessions
* Log content requests (beyond minimal rotating telemetry if enabled by governance)

---

## 3. Cryptographic Overview

### 3.1 Hybrid Encryption

Creators encrypt content using a symmetric key:

```
File_encrypted = Enc_sym(K_content, File)
```

The Access Microservice returns:

```
K_content_wrapped = Enc_asym(PK_session, K_content)
```

Only the client, holding `SK_session`, can unwrap it.

### 3.2 Client Ephemeral Keys

Clients generate a **fresh ephemeral keypair** per request:

```
(PK_session, SK_session)
```

These MUST NOT be reused or linkable across requests.

### 3.3 Tier Eligibility Proofs

The client submits one of:

* zk-SNARK tier membership proof
* Signature-based Tier Capability Token (short-lived, unlinkable)
* Encrypted/MACed tier token derived from the Tier contract or a rollup

The microservice MUST NOT learn:

* User identity
* User history
* Device fingerprint

---

## 4. API Endpoints

Endpoints follow a minimal, privacy-preserving design.

### 4.1 `POST /v1/access/request`

Request wrapped content keys.

#### Request Body

```
{
  "content_id": "CID",
  "manifest_hash": "HASH256",
  "creator_signature": "SIG_CREATOR",
  "tier_proof": { ... },
  "client_ephemeral_pubkey": "PK_session",
  "nonce": "client_challenge_nonce"
}
```

#### Required Checks

* Verify creator signature over manifest.
* Verify manifest hash matches content ID.
* Verify tier eligibility proof for required tier.
* Verify request structure and freshness (nonce replay prevention).

#### Response

```
{
  "k_content_wrapped": "ENC(PK_session, K_content)",
  "ms_nonce": "server_challenge_nonce",
  "signature": "SIG_ms_over_all_fields"
}
```

### 4.2 `POST /v1/access/verify`

Optional endpoint used by clients to verify server authenticity.

#### Request Body

```
{
  "challenge_nonce": "nonce_from_client"
}
```

#### Response

```
{
  "signature": "SIG_ms_over_nonce",
  "ms_pubkey": "public_key_of_microservice"
}
```

### 4.3 `GET /v1/health`

* Returns operational status.
* MUST NOT leak implementation details.

```
{
  "status": "ok"
}
```

---

## 5. Required Cryptographic Validation

### 5.1 Manifest Verification

Microservice MUST verify:

* Manifest was signed by the creator’s public key
* Manifest matches on-chain or rollup-published metadata

### 5.2 Content ID Binding

The microservice MUST reject:

* Any request where `manifest_hash != H(content_manifest)`
* Any mismatch between `content_id` and manifest data

### 5.3 Tier Eligibility

Tier proof MUST bind the user to a tier allowed for this content class.
Microservice MUST NOT learn:

* Which user submitted the proof
* How many proofs the user has submitted

### 5.4 Ephemeral Key Validity

`PK_session` MUST be validated for curve correctness.

### 5.5 Nonce Handling

* Each request MUST include a client nonce.
* Server responds with an MS nonce.
* Nonces MUST NOT be stored after request completion.

---

## 6. Privacy Requirements

The Access Microservice MUST:

### 6.1 Non-Logging

* NOT record client IPs tied to content IDs beyond what is legally required (configurable governance policy)
* NOT store tier proofs
* NOT store ephemeral keys
* NOT store nonces
* NOT store request history

### 6.2 Non-Linkability

Access Microservices MUST NOT:

* Use cookies
* Use browser fingerprinting
* Use persistent sessions
* Correlate requests over time

### 6.3 Statelessness

All requests MUST be stateless.

### 6.4 Multi-Microservice Deployment

Clients SHOULD rotate across multiple microservice instances to avoid correlation.

---

## 7. Security Guarantees

### 7.1 What the Microservice CAN Guarantee

* Only tier-eligible users can obtain `K_content`.
* Only users with valid manifests can request access keys.
* No long-term logs or user-identifying records exist.
* Returned wrapped keys are safe against man-in-the-middle attacks.

### 7.2 What the Microservice MUST NOT Guarantee

The microservice cannot and must not try to:

* Enforce playback integrity
* Validate usage correctness
* Perform DRM-like functions
* Learn what specific user is accessing content
* Maintain per-user state or profiles

Usage correctness is enforced strictly via **ZK proofs**, independent of microservice trust.

---

## 8. Failure Modes & Responses

### 8.1 Invalid Tier Proof

Response:

```
403 Forbidden
```

No additional detail.

### 8.2 Invalid Manifest Signature

Response:

```
400 Bad Request
```

No content leak.

### 8.3 Suspicious Abuse Pattern (Governance-Configurable)

* Temporary rate limit applied to source IP region (configurable and must be privacy-aware)
* MUST NOT apply user-specific blocks

### 8.4 Misconfigured or Compromised Instance

* Instances MUST be auditable via reproducible builds
* DAO governance may revoke signing key
* Clients MUST support instance rotation

---

## 9. Deployment Requirements

### 9.1 Reproducible Builds

All official microservices MUST support verified reproducibility.

### 9.2 DAO Controlled Keys

* Microservice signing and TLS keys MUST be managed under DAO-approved HSM or multisig.

### 9.3 Federation

Multiple creator cooperatives MAY host their own Access Microservices.

### 9.4 Standard Interop

All microservices MUST follow this specification exactly to ensure client interoperability.

---

## 10. Summary

The Access Microservice is a **stateless, privacy-preserving decryption oracle** for CWE. It enables:

* Tier-gated content access
* Creator-controlled key distribution
* Strong privacy protections
* Zero reliance on client trust
* Compatibility with ZK usage accounting

It MUST:

* Remain stateless
* Stay non-linkable
* Never log user behavior

It MUST NOT:

* Enforce DRM
* Track users
* Store access histories
* Rely on persistent sessions

This design ensures CWE remains open, decentralized, and privacy-protecting while enabling creators to securely distribute protected works.

