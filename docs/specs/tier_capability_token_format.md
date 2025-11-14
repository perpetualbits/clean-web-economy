<!-- File: docs/specs/tier_capability_token_format.md -->

# Clean Web Economy

## Tier Capability Token Format

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Tier Capability Token (TCT)** used by CWE clients to prove **tier membership** to the Access Microservice without revealing:

* User identity
* Device identity
* Payment history
* Exact subscription tier
* Any linkability across epochs

TCTs are designed to be:

* Privacy-preserving
* Unlinkable
* Short-lived
* Non-trackable
* Verifiable entirely on the client
* Compatible with ZK tier proofs

TCTs provide minimal, non-identifying capability information required for tier-gated content access.

---

## 2. Design Goals

### 2.1 Privacy

The microservice MUST NOT be able to:

* Identify a user
* Link multiple requests together
* Infer viewing patterns

### 2.2 Cryptographic Soundness

Tokens MUST:

* Prove tier membership accurately
* Resist forgery
* Bind to the correct epoch
* Be non-transferable

### 2.3 Short-Lived & Stateless

* No sessions
* No persistent identifiers
* Tokens expire automatically per epoch

### 2.4 Compatibility with ZK Proofs

TCTs must fit inside ZK circuits for usage verification.

---

## 3. Architecture Overview

The Tier Capability Token Format combines:

1. **Tier Commitment** (long-lived, private)
2. **Epoch Blind** (provides unlinkability)
3. **Ephemeral Token** (per-request)

```
Tier Commitment + Epoch Blind → Epoch Capability Token → Request Capability Token (one-time)
```

Each stage increases privacy and breaks correlation.

---

## 4. Components

### 4.1 Tier Commitment (Long-Lived)

Generated when the user activates a tier.

```
T_commit = Com( tier_id , user_secret , randomness )
```

Properties:

* Never leaves the device
* Not shared with microservices
* Bound to user but not linkable publicly

### 4.2 Epoch Blind

Derived from epoch beacon `K_epoch`:

```
T_epoch = H( K_epoch || T_commit )
```

Properties:

* Changes every epoch
* Cannot be linked across epochs
* Cannot identify user

### 4.3 Epoch Capability Token

The client constructs:

```
T_cap_epoch = Sign_user( T_epoch )
```

This signature:

* Proves possession of the hidden tier
* Does NOT reveal user public keys (uses blinded signing)

### 4.4 Request Capability Token (One-Time)

Before making an access request, the client generates:

```
req_nonce
PK_req (ephemeral)
T_request = H( T_cap_epoch || req_nonce || PK_req )
```

This is the final token sent to the Access Microservice.

---

## 5. Final Request Structure

Clients send the following to Access Microservice:

```
{
  "capability_token": "T_request",
  "tier_proof": "ZK_tier",               // optional but recommended
  "client_ephemeral_pubkey": "PK_req",
  "nonce": "client_nonce"
}
```

Microservice NEVER sees:

* Tier Commitment
* Epoch Blind
* T_cap_epoch
* User secret keys

---

## 6. Verification Rules (Microservice Side)

The Access Microservice MUST:

1. Verify the structure & freshness of `T_request`
2. Verify optional `ZK_tier` if provided
3. Check that the token corresponds to a valid tier class
4. NOT store any capability tokens
5. NOT correlate requests based on structure similarities

If ZK tier proofs are omitted (rare), microservice performs a blind-signature verification of `T_request` using epoch public parameters.

---

## 7. Optional Zero-Knowledge Tier Proof

To enhance privacy and robustness:

Clients MAY embed a ZK proof:

```
ZK_tier = Proof( "I possess a valid tier commitment for tier T" )
```

ZK circuits prove:

* User has a valid Tier Commitment
* The commitment is part of the approved tier set
* No identifying data is revealed

Microservice MUST accept both:

* Tokens with ZK proofs
* Tokens without ZK proofs (if blind signatures suffice)

---

## 8. Token Lifetime & Rotation

### 8.1 Tier Commitment

* Lifetime: subscription duration
* Rotated: only when upgrading/downgrading tier

### 8.2 Epoch Capability Token

* Lifetime: single epoch
* Rotated: automatically when `K_epoch` updates

### 8.3 Request Capability Token

* Lifetime: single request
* Rotated: every content access

This ensures **three layers of unlinkability**.

---

## 9. Security Properties

### 9.1 Non-Linkability

Microservice cannot:

* Link access requests
* Link across epochs
* Correlate tokens with bandwidth receipts

### 9.2 Blind Verification

The microservice learns only:

* "This requester holds tier X capabilities"
* Nothing else

### 9.3 Replay Resistance

Each `T_request` includes:

* A unique nonce
* A unique ephemeral public key

Replay MUST cause rejection.

### 9.4 Unforgeability

Requires:

* Valid epoch parameters
* Correct blind signature or ZK proof

### 9.5 Non-Transferability

Because tokens depend on:

```
user_secret
randomness
epoch beacon
```

Only the rightful subscriber can derive them.

---

## 10. Integration With Other CWE Layers

### 10.1 Access Microservice

* Uses `T_request` only to grant wrapped content keys
* Does not learn tier commitment

### 10.2 ZK Usage Proofs

Proof circuits use Tier Commitment internally to show:

* User was authorized for content
* Without revealing tier or identity

### 10.3 Discovery Layer

Receives only aggregated tier usage signals, never TCTs.

### 10.4 Chain Layer

Chain contracts do NOT interact with TCTs.

---

## 11. Implementation Considerations

### 11.1 Cryptographic Choices

* Pedersen or Poseidon commitments
* BLS or Schnorr blind signatures
* STARK or SNARK tier circuits

### 11.2 Browsers & Sandboxing

Clients MUST:

* Not expose Tier Commitment to scripting
* Store commitments in encrypted local storage or keystore

### 11.3 Mobile Environments

TEE storage is optional, never required.

---

## 12. Summary

The Tier Capability Token Format provides:

* Private, anonymous tier verification
* Blind, unlinkable access tokens
* Compatibility with ZK usage proofs
* Stateless, privacy-preserving interactions with microservices
* Full independence from browser vendors, hardware TEEs, and DRM systems

This enables CWE to support a flexible tier system **without compromising user anonymity or developer freedom**.

