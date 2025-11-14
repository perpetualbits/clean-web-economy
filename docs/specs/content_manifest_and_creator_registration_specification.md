<!-- File: docs/specs/content_manifest_creator_registration_specification.md -->

# Clean Web Economy

## Content Manifest & Creator Registration Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Content Manifest Format** and the **Creator Registration Workflow** for the Clean Web Economy (CWE). These components:

* Cryptographically bind works to their creators
* Enable decentralized hosting and distribution
* Allow clients to verify content authenticity without revealing user identity
* Provide the data structures needed for:

  * Tier-gated access
  * ZK usage proofs
  * DAPR payouts
  * Storage indexing
  * Discovery layer ranking

Manifests serve as immutable, verifiable “passports” for every piece of content in the ecosystem.

---

## 2. Core Design Principles

### 2.1 Creator Sovereignty

Creators sign their own manifests. No central authority may override creator identity or ownership.

### 2.2 Immutable Registration

Once registered on-chain or in a rollup, a manifest becomes a permanent identity record for the work.

### 2.3 Minimal Leak Surface

Manifests contain **only public metadata**. No user-identifying data is ever present.

### 2.4 Content-Agnostic

Manifests are generic:

* Audio
* Video
* Text
* Games
* Images
* Code
* Scientific papers
* News articles

### 2.5 Compatibility with ZK Proofs

Manifests are structured to support ZK circuits without embedding sensitive data directly.

---

## 3. Creator Registration

A creator registers via the **Creator Portal** or a compatible third-party tool.

### 3.1 Creator Identity Key

A creator MUST generate a **long-term identity keypair**:

```
PK_creator, SK_creator
```

Requirements:

* MUST be stored securely (HSM, TEE, hardware wallet, or offline cold storage)
* MUST NOT be used directly for session or proof keys
* MUST NOT appear in client P2P handshakes

### 3.2 Registration Steps

1. Generate identity keypair
2. Submit public key + metadata to the **Creator Registry Contract**
3. Receive a **creator_id** (hash of PK_creator)
4. Optional: delegate publishing rights to additional keys via multisig or role-based signatures

### 3.3 Registry Contract Requirements

The contract MUST:

* Record PK_creator
* Record optional collaborator splits
* Record metadata pointers (e.g., IPFS, swarm manifests)
* Remain append-only
* Support upgrades only via governance vote

### 3.4 Anti-Sybil Controls

Creators MAY be required to:

* Pay a small registration fee
* Provide a proof-of-humanity (optional, DAO-governed)
* Provide social verifiability signals

None of these affect work verification itself.

---

## 4. Content Manifest Format

A Content Manifest is a signed JSON or CBOR document containing:

```
{
  "version": 1,
  "content_id": "CID256",            // hash of encrypted content
  "manifest_id": "MID256",           // hash of this manifest
  "creator_id": "CRID256",           // hash of PK_creator
  "timestamp": 1234567890,
  "work_type": "video" | "audio" | ...,
  "title": "string",
  "description": "string",
  "tags": [ "tag1", "tag2" ],
  "tier_requirements": [ "basic", "premium", ... ],
  "fingerprint": "HASH256",          // canonical fingerprint
  "fragments": [
      {
        "fragment_hash": "H256",
        "size": 524288,
        "order": 0
      },
      ...
  ],
  "price": 0.0,                        // optional creator price multiplier
  "collaborators": [                   // optional revenue split
      {
        "creator_id": "CRID256",
        "share": 0.15
      }
  ],
  "signature": "SIG_creator_over_all_fields"
}
```

---

## 5. Field Requirements

### 5.1 `content_id`

* MUST be `H(file_encrypted)`
* MUST NOT reveal content type, size, or plaintext structure

### 5.2 `manifest_id`

* MUST be `H(manifest_contents_without_signature)`

### 5.3 `creator_id`

* MUST match the registered creator keypair
* MUST be verified via signature

### 5.4 `work_type`

Examples:

* `video`
* `audio`
* `image`
* `text`
* `interactive`
* `scientific`
* `news`

This informs the Discovery Layer and client capabilities.

### 5.5 `tier_requirements`

Defines which tiers grant access:

* Basic
* Audio-only
* Video
* Premium
* Research

Tier checks are done **via the Access Microservice**, not by storage nodes.

### 5.6 `fingerprint`

A cryptographic fingerprint used for:

* Discovery
* Deduplication
* Dispute resolution

The fingerprint algorithm is defined in the **Fingerprinting Specification**.

### 5.7 `fragments`

List of encrypted fragments:

* Order MUST NOT imply viewing order or metadata
* Fragment sizes may be uniform or variable
* Nodes fetch purely by fragment_hash

### 5.8 `collaborators`

Optional payout splits.
The Chain Layer resolves splits at payout time.

### 5.9 `signature`

Creators MUST sign the manifest using:

```
signature = Sign( SK_creator , H(manifest_content) )
```

Clients MUST verify the signature.

---

## 6. Manifest Validation (Client Side)

A client MUST:

* Verify creator signature
* Verify manifest hash integrity
* Verify that fragments match available storage hashes
* Verify tier requirements BEFORE attempting playback
* Verify collaborator splits sum ≤ 1.0

A client MUST NOT:

* Send manifest contents to storage nodes
* Log manifest IDs tied to user identity

---

## 7. Manifest Registration (Chain Layer)

Creators submit their manifest:

* `manifest_id`
* `content_id`
* `creator_id`
* `fingerprint`
* `tier_requirements`
* Optional collaborator splits

The chain stores:

* Hashes only
* Metadata pointers for off-chain manifest storage

The chain MUST NOT store:

* Titles
* Descriptions
* Tags
* Fragment lists
* Filenames
* Content type

These live in distributed storage.

---

## 8. Manifest Distribution

Manifests are distributed through:

* Distributed storage networks (IPFS, swarm)
* Creator-hosted static endpoints
* Discovery hubs
* CDN mirrors

Clients MUST verify signature before use.

Manifests MAY be cached locally but MUST be pruned after:

* Epoch transitions
* Content updates

---

## 9. Interaction With Other CWE Layers

### 9.1 ZK Usage Proofs

ZK circuits use manifest data to:

* Validate `work_id`
* Validate tier eligibility
* Validate commitment correctness

Manifests MUST be structured so that ZK circuits can reference them via hashed commitments without leaking data.

### 9.2 Access Microservice

Manifests inform:

* Tier checks
* Key distribution

### 9.3 Client–Storage Handshake

Manifests provide fragment hashes but **storage nodes learn nothing** about:

* Titles
* Metadata
* Creator identity

### 9.4 DAPR Payouts

Creators receive:

```
payout = usage × creator_price × split_adjustments
```

Manifest fields inform:

* Price
* Splits
* Fingerprints

### 9.5 Discovery Layer

Manifests provide the metadata used in:

* Ranking
* Deduplication
* Reputation signals

---

## 10. Security Requirements

### 10.1 Against Malicious Creators

A manifest MUST:

* Correctly bind fragment hashes
* Have a unique content_id
* Include a valid signature

Discovery hubs MUST detect and flag:

* Duplicate fingerprints
* Near-duplicate fingerprints

### 10.2 Against Malicious Clients

Clients MUST NOT be able to:

* Forge manifests
* Manipulate creator signatures
* Substitute manifests during ZK proofs

### 10.3 Privacy Constraints

Manifests MUST NOT:

* Include user-tracking fields
* Depend on per-user encryption
* Reveal consumption behavior

### 10.4 Mutability

Manifests are **immutable**.
Edits require a **new manifest** and optionally a chain-level version pointer.

---

## 11. Summary

This specification defines a transparent, secure, and privacy-preserving method for:

* Declaring content
* Registering creator identity
* Cryptographically binding encrypted fragments to creators
* Supporting ZK usage proofs
* Supporting tier-gated access
* Ensuring fair payouts via DAPR

Content Manifests and Creator Registration are foundational to the CWE ecosystem, ensuring authenticity, accountability, interoperability, and decentralization without exposing user behavior or enabling platform control.

