<!-- File: docs/specs/client_storage_handshake_specification.md -->

# Clean Web Economy

## Client–Storage Handshake Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Client–Storage Handshake Protocol (CSHP)** that governs how CWE clients interact with distributed storage nodes when fetching encrypted content fragments.

The goals are:

* Strong **privacy** (no content IDs tied to users)
* Strong **anti-abuse** and **anti-fraud** properties
* No reliance on **client trust** or **storage node trust**
* No leakage of **usage behavior**
* Stateless interaction between clients and storage nodes
* Compatibility with:

  * ZK Usage Proofs
  * Anti-Fraud & Bandwidth Receipt Protocol (AFBRP)
  * Tier-gated access (via Access Microservice)

This handshake is deliberately minimal and cryptographically constrained to prevent profiling, content tracking, or manipulation.

---

## 2. Scope

CSHP applies when:

* A client retrieves fragments from storage nodes (IPFS-like, BitTorrent-like, or custom swarm)
* A storage node serves encrypted blocks
* Bandwidth receipts are generated (if enabled by AFBRP)

It does **not** handle:

* Decryption (done client-side)
* Tier validation (Access Microservice)
* Usage accounting (ZK Usage Proofs)
* Discovery and routing logic (Discovery Layer)

---

## 3. Design Principles

### 3.1 Unlinkability

A storage node MUST NOT be able to determine:

* Which user requested a fragment
* Whether two requests belong to the same user
* Whether two fragments belong to the same work

### 3.2 Minimal Metadata

Clients MUST reveal the **minimum possible metadata** to storage nodes.

### 3.3 Statelessness

Storage nodes MUST NOT maintain:

* Sessions
* Tokens
* Long-lived request identifiers
* Logs tying content to IPs (beyond operational or legal minimal requirements)

### 3.4 Ephemeral Keys Everywhere

All cryptographic material MUST be:

* Ephemeral
* Non-linkable across requests
* Non-reusable across epochs

### 3.5 Compatibility With Zero-Knowledge

All structure MUST support (not conflict with):

* ZK Usage Proof circuits
* Bandwidth receipt proofs

---

## 4. Handshake Flow Overview

The Client–Storage Handshake consists of **three steps**:

1. **Client → Storage:** Anonymous fragment request using ephemeral request key
2. **Storage → Client:** Encrypted fragment response + ephemeral server signature
3. **Peer Receipts:** Optional mutual receipts for AFBRP

```
Client                               Storage Node
  |                                          |
  |-----(1) Fragment Request (Anon)--------->|
  |                                          |
  |<-----(2) Fragment Response (Enc)---------|
  |                                          |
  |-----(3 - optional) Receipt Exchange----->|
```

---

## 5. Message Formats

### 5.1 Fragment Request

```
{
  "fragment_hash": "H256",        // identifies encrypted fragment
  "req_pubkey": "PK_req",          // ephemeral
  "client_nonce": "N_client",      // random, per-request
  "protocol_version": 1
}
```

#### Requirements:

* `req_pubkey` MUST be freshly generated every request.
* `fragment_hash` MUST NOT encode any work ID or metadata.
* `client_nonce` MUST prevent replay attacks.
* No user identifiers.
* No cookies.
* No long-lived tokens.

---

### 5.2 Fragment Response

```
{
  "fragment_data": "BYTES",         // encrypted data
  "srv_pubkey": "PK_srv",          // ephemeral
  "srv_signature": "SIG_srv",      // proves data integrity
  "srv_nonce": "N_srv"             // server challenge
}
```

#### Requirements:

* `srv_pubkey` MUST be ephemeral.
* `srv_signature` MUST cover:

  * `fragment_hash`
  * `fragment_data`
  * `PK_req`
  * `PK_srv`
  * `N_client`
  * `N_srv`
* `fragment_data` MUST always be encrypted with `K_content`.

Storage nodes MUST NOT know or infer:

* Who is requesting
* The tier of the requester
* Whether the client has decryption rights

---

## 6. Optional: Peer Bandwidth Receipt Exchange

If AFBRP is enabled, after successful transmission:

```
{
  "receipt_commitment": "R",        // commitment from AFBRP
  "sig_client": "SIG_client_R",    // ephemeral
  "sig_server": "SIG_server_R"     // ephemeral
}
```

These receipts are:

* Anonymous
* Ephemeral
* Bound to `fragment_hash`, `N_client`, `N_srv`
* Safe for inclusion into AFBRP circuits

---

## 7. Protocol Details

### 7.1 Request Freshness

Storage nodes MUST reject:

* Old `client_nonce` values
* Request replays
* Unrecognized protocol versions

### 7.2 Fragment Identification

`fragment_hash` MUST be:

* A raw hash of **encrypted** fragment payload
* Stable
* Content-addressable

It MUST NOT:

* Encode work_id
* Encode manifest hash
* Correlate multiple fragments to same work

### 7.3 Transport Requirements

The handshake MAY operate over:

* QUIC
* TLS
* Noise protocol
* Tor-like anonymity overlays
* Onion-routed P2P connections

Transport MUST NOT break statelessness or anonymity.

### 7.4 Storage Node Verification

Clients MUST verify:

* `srv_signature`
* `srv_pubkey` curve correctness
* Nonces
* No replay of fragments

### 7.5 Privacy Constraints on Storage Nodes

Storage nodes MUST NOT:

* Fingerprint clients
* Use TLS session resumption
* Maintain connection cookies
* Correlate requests across sessions
* Log fragment request sequences

---

## 8. Anti-Abuse Controls

### 8.1 Rate Limiting

Storage nodes MAY:

* Apply blind, per-IP-region rate limits
* Apply global rate smoothing

They MUST NOT:

* Rate limit based on request patterns
* Apply user-specific throttling

### 8.2 Denial of Service Protection

Nodes MAY:

* Use proof-of-work tokens (hashcash-style) **if non-linkable**
* Perform global congestion control

### 8.3 Fragment Existence Proof

Nodes MUST provide a verifiable signature proving fragment integrity.

---

## 9. Integration With Other Layers

### 9.1 ZK Usage Proofs

The handshake ensures:

* Commitments can be generated locally
* No reveal of which fragments belong to which work
* Monotonicity and uniqueness can be proven independently

### 9.2 Access Microservice

Storage nodes do not perform tier validation.
Clients obtain decryption keys elsewhere.

### 9.3 AFBRP

Receipt exchange becomes input to:

* Bandwidth ZK proofs
* Anti-inflation measures

### 9.4 Discovery Layer

Only fragment hashes, not content IDs, are used.

---

## 10. Security Guarantees

CSHP ensures:

* Storage nodes learn **nothing** about:

  * User identity
  * Consumption habits
  * Content being accessed

* Clients learn:

  * Fragment integrity
  * Stage for optional receipt generation

* The ecosystem gains:

  * Bandwidth-backed credibility of usage
  * Resistance to inflation
  * Zero reliance on trusted clients

---

## 11. Summary

The Client–Storage Handshake Protocol is a foundational element of CWE privacy and security. It:

* Eliminates user tracking at the storage layer
* Obscures content relationships
* Supports ZK usage proofs
* Enables anonymous bandwidth receipts
* Maintains stateless, uncorrelated communication
* Enforces minimal, privacy-safe metadata exposure

The result is a **resilient, privacy-preserving, adversarially robust content-fetching mechanism** that aligns fully with the core design principles of the Clean Web Economy.

