<!-- File: docs/specs/collaborator_split_royalty_flow_specification.md -->

# Clean Web Economy

## Collaborator Split & Royalty Flow Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines **how revenue flows from users to creators and collaborators** within the Clean Web Economy (CWE), including:

* Work registration and collaborator declarations
* Royalty splits and payout shares
* How DAPR usage weights translate into monetary flows
* How collaborators (producers, writers, actors, editors, translators, musicians, etc.) are compensated
* How upgrades to splits are handled over time
* How one-time purchases, ownership NFTs, and collectible artifacts integrate

This system ensures:

* Transparent, programmable payout logic
* Cryptographic integrity
* Zero behavioral data leakage
* Support for traditional *and* novel creator business models

---

## 2. Revenue Sources

CWE revenue flows to creators and collaborators through two channels:

### 2.1 Subscription-Based Usage Revenue (Default)

Collected via:

* Flat-tier user subscriptions
* Aggregated via DAPR
* Distributed per work based on usage weights

### 2.2 Direct Purchases (New Mode)

Creators may optionally sell:

* Albums
* Singles
* Full-length films
* Books, chapters, issues
* **Creator-signed NFTs** (digital autographs, art, messages, etc.)

Direct purchases flow directly to the creator and declared collaborators using the same split logic.

NFTs or digital collectibles:

* **Must NOT be stored on-chain** beyond a hash
* Content/artwork stored in decentralized storage (IPFS, Arweave, Filecoin)
* Ownership recorded via ERC-721 or minimal custom NFT on the CWE L2

---

## 3. Work Registration & Collaborator Structure

Every work has a **Creator Manifest**, which includes a list of collaborators and split percentages.

### 3.1 Collaborator Entry Structure

```
collaborator = {
  id: DID / SSI,
  role: "producer" | "writer" | "actor" | ...,
  share_bps: integer, // basis points (1/10,000)
  payout_address: address,
  metadata: optional
}
```

### 3.2 Manifest-Level Constraints

* `sum(share_bps) = 10000` (100%)
* Creator may assign 0–100% to themselves
* DAO governance may enforce maximum collaborator count per work to reduce complexity

### 3.3 Mutability

Collaborator list is:

* Immutable *per version*
* New versions allowed (with versioned fingerprints)
* Older versions remain valid for royalties unless sunset by creator

---

## 4. Royalty Flow Model

### 4.1 DAPR Revenue Flow

At the end of each epoch:

```
work_revenue = pool_total * (usage_weight_w / sum_all_weights)
```

Then:

```
collaborator_payout_i = work_revenue * (share_bps_i / 10000)
```

### 4.2 Direct Purchase Flow

If a user buys a work or NFT:

```
purchase_revenue = purchase_price
collaborator_payout_i = purchase_revenue * (share_bps_i / 10000)
```

Direct purchases bypass DAPR entirely.

### 4.3 Mixed Mode

A work can earn:

* Subscription usage royalties **and**
* Direct purchase royalties **and**
* NFT royalties (optional)

All payments are routed through the same collaborator split logic.

---

## 5. Enforcement & Verification

All collaborator splits are:

* Declared in the manifest
* Cryptographically signed by the creator
* Published on-chain as a cryptographic commitment
* Validated by:

  * Discovery hubs
  * DMF aggregation nodes
  * Storage gateways

Smart contracts ensure:

* Immutable split structure per manifest version
* Correct distribution of funds
* No administrator interference

---

## 6. Payout Contracts

The chain contains:

* **Tier contract** – handles subscription flow
* **Consumption contract** – receives DAPR aggregated weights
* **Payout contract** – distributes revenue to collaborators

### 6.1 Payout Contract Interface (Simplified)

```
distribute( work_id , amount ) {
  for each collaborator in work.manifest:
    send( collaborator.address , amount * share_bps / 10000 )
}
```

All collaborators are paid **atomically**.

### 6.2 Batch Distributions

For efficiency, the system supports:

* `distributeBatch( [work_ids], [amounts] )`
* Gas-optimized multi-send
* Merkle-aggregate proofs

---

## 7. NFT-Based Business Models

Creators may define **premium editions** of their works:

* Signed digital poster
* Direct audio master
* Personalized artwork
* A unique autograph NFT
* A private message NFT

### 7.1 Storage of NFT Content

NFT metadata includes:

* `content_hash`
* `storage_uri` (IPFS CID, Arweave ID, etc.)
* Thumbnail/preview metadata

**Full content NEVER stored on-chain.**

### 7.2 NFT Ownership Benefits

NFT owners may receive:

* Early access
* Bonus materials
* Access to private Discord/Matrix rooms
* Merch discounts
* Special tiers or creator perks

All optional.

### 7.3 Reselling

If governance approves royalty-on-resale:

* Creators may receive a % of secondary sales

Optional but widely requested in creative ecosystems.

---

## 8. Versioning & Evolution

Creators may:

* Update the manifest
* Change collaborator definitions
* Add new versions of the same work
* Publish remasters, director’s cuts, alternate mixes

Each version:

* Has its own fingerprint
* May inherit payout structure from previous version
* May define new collaborator structures

Users’ clients match versions using fingerprint similarity rules.

---

## 9. Handling Derivatives, Covers, & Remixes

### 9.1 Derivative Works

If fingerprint similarity indicates a derivative:

* Creator MAY claim a share of derivative revenue
* Governance defines default rules
* Optional creator-to-creator agreements supported

### 9.2 Cover Versions

Cover artists may:

* Register their own manifest
* Assign share to original composer if required by law
* Assign the rest to themselves

### 9.3 Sampling

If sample is detected:

* Optional automated revenue attribution
* Governance defines thresholds and grace rules

---

## 10. Dispute Resolution

Collaborator disputes handled by:

1. **Arbitration Service**
2. **Creator Council** (if elected)
3. **DAO final vote** (rare, only hard cases)

Disputes MUST NOT:

* Freeze unrelated creator revenue
* Block unrelated payouts

---

## 11. Integration With Fingerprinting

Fingerprinting enables:

* Automatic redirect of unsigned file usage to canonical manifest
* Detection of stolen content
* Automated matching of derivative content

Usage credit ALWAYS flows to:

* The rightful canonical manifest
* Unless a newer version override is chosen by the user

---

## 12. Privacy Guarantees

Collaborator payout flow MUST NOT expose:

* User identities
* User behavior
* Pseudonyms or usage events

All money flow depends only on:

* Aggregated DAPR outputs
* Manifest-defined splits

---

## 13. Summary

This specification defines a fair, extensible, cryptographically secure royalty system that:

* Supports arbitrarily complex collaborator structures
* Ensures automatic, tamper-proof payouts
* Enables direct purchases and collectibles without centralization
* Integrates with fingerprints for theft protection
* Protects user privacy at every step

Creators can innovate freely while users retain full privacy and autonomy.
CWE becomes a truly hybrid cultural economy: **streaming + ownership + collectibles**, all decentralized, transparent, and fair.

