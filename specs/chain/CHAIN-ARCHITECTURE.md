# CWE Chain Architecture (Draft)

## 1. High-Level Components

The CWE chain layer is decomposed into the following on-chain modules:

1. **WorkRegistry** – canonical registry of creative works.
2. **UsageProofs** – verifier and aggregator for user-submitted ZK consumption proofs.
3. **DAPR** – payout engine that turns usage + prices into creator & collaborator payouts.
4. **IdentityHub** – links DID/VC-based identities to on-chain addresses.
5. **GovCore** – governance kernel (proposals, votes, councils, juries).
6. **EcoTreasury** – ecological reserve management and EcoReserve token.
7. **FeeRouter** – splits incoming funds between creators, infra, and EcoTreasury.

Each module lives in its own contract (or contract set) with well-defined interfaces.

---

## 2. Data Model (Conceptual)

### 2.1 Work identifiers

```text
WorkId = hash(
  media_fingerprint   # perceptual or content hash
  | creator_root_id   # identity root
  | media_type        # enum: audio/video/text/etc
  | version_tag       # optional
)

#### On-chain, WorkRegistry stores:

Work {
  work_id: bytes32
  creator_id: CreatorId
  pricing: PricingProfileId
  splits: SplitProfileId
  status: enum { Active, Deprecated, Removed }
  metadata_ref: bytes32   # e.g. IPFS CID hash
}

### 2.2 Creator and collaborator identity

#### IdentityHub stores:

Creator {
  creator_id: CreatorId   # stable local identifier
  primary_address: Address
  did: string             # W3C DID
  vc_root_hash: bytes32   # hash(commitment) of verifiable credentials bundle
  status: enum { Pending, Active, Suspended, Revoked }
}

### 2.3 Usage commitments

Clients produce per-epoch commitments and proofs.
Conceptually:

UsageCommitment {
  epoch_id: uint64
  user_tier: TierId
  commitment_root: bytes32  # root of a private usage tree
}


UsageProofs stores per-epoch aggregates:

WorkUsage {
  epoch_id: uint64
  work_id: bytes32
  tier_id: TierId
  usage_units: uint128      # e.g. minutes played, normalized
}

These aggregates are derived from ZK proofs, not from raw usage logs.

### 2.4 Payout state

DAPR tracks per-epoch payout calculations:

EpochPayout {
  epoch_id: uint64
  total_fees: uint256
  total_creator_pool: uint256
  total_eco_allocation: uint256
  work_payout_root: bytes32      # Merkle root for (work_id -> payout_amount)
}

Creators and collaborators claim via Merkle proofs.

### 2.5 EcoTreasury reserves

EcoTreasury maintains:

ReserveAsset {
  asset_id: bytes32              # hash of legal+ecological doc set
  asset_type: enum { Forest, CarbonRemoval, WaterRight, Other }
  units: uint256                 # domain-specific units (ha, tCO2, etc)
  valuation: uint256             # value in base currency (with risk haircut)
  jurisdiction: string
  mrv_profile_id: bytes32
}

Plus a mapping from asset_id to off-chain documents (URIs + hashes).

## 3. Contract Interfaces (Sketch)

### 3.1 WorkRegistry (see detailed spec in CHAIN-WORK-REGISTRY.md)

Key functions:

* registerWork(workData, pricingProfile, splitsProfile)

* updatePricing(workId, newPricingProfile)

updateSplits(workId, newSplitsProfile)

deprecateWork(workId)

Events:

WorkRegistered

WorkUpdated

WorkDeprecated
