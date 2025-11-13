<!-- File: specs/chain/CHAIN-ARCHITECTURE.md -->

# CWE Chain Architecture (Ethereum L2 + zk-Rollup)

## 0. Status

- **Version:** 0.1 (draft)
- **Scope:** High-level architecture and data/flow model for the CWE chain layer.
- **Target:** Ethereum Layer-2 using a zk-rollup execution environment.
- **Audience:** Engineers, architects, cryptographers, governance designers.

This document describes the *logical* architecture. It is intentionally implementation-agnostic about specific zk-rollup frameworks (e.g., StarkNet, zkSync, Scroll), but assumes:

- An Ethereum-compatible smart contract environment.
- A zk-rollup proving system.
- Low-cost, high-throughput transaction processing.

---

## 1. Design Goals (Recap)

The CWE chain layer must:

1. **Keep users private**
   - No on-chain per-user consumption logs.
   - Usage proofs must not leak viewing/listening patterns.

2. **Be fast and cheap**
   - Confirmation time: ideally under 1–3 seconds.
   - Fees: small and predictable (microtransactions feasible).

3. **Support programmable logic (smart contracts)**
   - DAPR payout logic.
   - Work registry and pricing.
   - Governance and councils.
   - Node rewards (storage/compute/validators).
   - Eco-reserve treasury logic.

4. **Avoid speculation**
   - Internal “money token” must be stable and boring.
   - Governance power must not be purchasable.
   - No built-in DeFi yield or casino-style incentives.

5. **Enable earning by contribution**
   - Tokens earned by:
     - Running storage nodes.
     - Running compute/microservice nodes.
     - Helping secure/run the rollup and related infra.
     - Producing creative works and tools.

6. **Be environmentally responsible**
   - No proof-of-work mining.
   - Proof-of-stake or equivalent low-energy consensus.

7. **Be evolvable**
   - Start as smart contracts on an Ethereum L2 rollup.
   - Allow future migration (optionally) to a dedicated appchain.
   - Allow gradual introduction of eco-backed treasury components.

---

## 2. High-Level Architecture

CWE uses a layered model:

- **Client Layer**
  - Browser extensions, media players, mobile apps.
  - Track usage locally, generate zk proofs, sign transactions.
- **Rollup Execution Layer (L2)**
  - CWE-specific smart contracts and state.
  - zk-rollup proving system (off-chain prover).
  - Sequencer(s) and full nodes.
- **Base Layer (Ethereum L1)**
  - Verifies zk proofs.
  - Stores rollup state commitments and critical treasury balances.
  - Serves as final dispute/arbitration layer.
- **Off-Chain Services**
  - Distributed storage (IPFS, DMF storage nodes).
  - DMF compute/microservice nodes.
  - Oracles and auditors (especially eco-asset oracles).
  - Legal and audit infrastructure for eco-reserve assets.

A simplified ASCII diagram:

    [Client Apps]
        |
        v
    [CWE zk-Rollup (L2)]
        |            \
        |             \
        v              v
    [Ethereum (L1)]   [Off-chain: Storage, DMF, Oracles]

---

## 3. Core On-Chain Modules (Logical)

All modules below live in the **rollup execution layer** (L2). Some have thin anchors / mirrors on Ethereum L1.

### 3.1 Work Registry

Purpose:
- Canonical record of creative works recognized by CWE.
- Minimal: store identifiers, pricing profiles, split profiles, and metadata references.

Responsibilities:
- Register a new work (by authorized creators).
- Update pricing and splits (for future epochs only).
- Deprecate or remove works under governance rules.
- Expose interfaces for DAPR and discovery services.

Key properties:
- Uses robust identifiers (WorkId) derived from fingerprints + creator identity.
- Off-chain metadata (IPFS references) to keep the chain minimal.

### 3.2 Usage Proofs Layer

Purpose:
- Accept privacy-preserving usage proofs from clients.
- Aggregate usage over epochs in a way that:
  - Is consistent with reported tier payments.
  - Cannot be linked back to individual users on-chain.

Responsibilities:
- Receive per-epoch zk proofs that the client’s local usage data:
  - Follows format rules.
  - Respects rate limits and tier rules.
- Aggregate contributions into per-work usage totals.
- Produce data structures consumed by DAPR.

Key properties:
- On-chain data is at the level of:
  - (epoch, work, tier, usage_units)
- No per-user IDs appear on-chain.

### 3.3 DAPR Payout Engine

Purpose:
- Implement the revenue distribution algorithm:
  - Payouts proportional to usage × creator price.
- Convert aggregated usage into concrete payouts to creators/collaborators.

Responsibilities:
- For each epoch:
  - Read tier revenue and protocol parameters.
  - Read aggregated usage per work.
  - Apply pricing profiles and collaborator splits.
  - Compute payout amounts.
- Allow creators/collaborators to claim from epoch payout pools.

Key properties:
- Every epoch has:
  - A clear and deterministic formula.
  - A verifiable record of:
    - total revenue,
    - total fees,
    - total creator payout,
    - share allocated to Eco-Reserve treasury and infra funds.
- Claims are typically facilitated via:
  - A Merkle root or similar commitment to per-work payouts, so individual claiming is efficient.

### 3.4 Identity & Eligibility Layer

Purpose:
- Represent and enforce:
  - Verified creator identities.
  - Collaborator mappings.
  - One-human-one-vote governance identities.

Responsibilities:
- Map DID/VC-based identities to on-chain addresses.
- Manage status: Pending, Active, Suspended, Revoked.
- Provide hooks for:
  - WorkRegistry (only legitimate creators can register works).
  - Governance (only legitimate persons can vote).

Key properties:
- Does not store sensitive personal data on-chain.
- Stores commitments/hashes to off-chain credentials.
- Integrates with external SSI/VC systems.

### 3.5 Governance Core

Purpose:
- Provide a programmable framework for:
  - Proposals,
  - Voting,
  - Councils,
  - Jury-based arbitration.

Responsibilities:
- Manage governance tokens (non-transferable, one-human-one-vote).
- Manage proposal lifecycle:
  - creation, discussion, voting, enactment.
- Support:
  - Constitutional changes,
  - Parameter updates (e.g., DAPR coefficients, fee rates),
  - Governance of Eco-Reserve and node reward rules.

Key properties:
- Governance tokens are **not** transferable.
- Voting is logged and auditable.
- Some decisions may require multi-stage processes or council/jury involvement.

### 3.6 Treasury & Stable Token

Purpose:
- Implement a **stable internal currency** (CWE credits) used for:
  - Creator payouts,
  - Node rewards,
  - Eco-Reserve contributions,
  - Protocol fees.

Responsibilities:
- Mint/burn CWE credits according to:
  - Fiat on/off-ramps,
  - Protocol revenue flows,
  - Governance-approved treasury management.
- Interface with:
  - Fiat-backed custodial reserves (off-chain),
  - Eco-Reserve treasury (see below).

Key properties:
- Designed to be boring for speculators:
  - Fixed or tightly controlled value relative to a reference (e.g. EUR).
  - No built-in yield.
  - No incentivized farming mechanisms.

### 3.7 Eco-Reserve Treasury

Purpose:
- Accumulate and manage ecological assets over time.

Responsibilities:
- Receive a fixed or configurable share of protocol revenue (in CWE credits).
- Convert some share of treasury value into:
  - Land and conservation rights,
  - Carbon removal contracts,
  - Water/biodiversity rights,
  - Other vetted ecological assets.
- Expose:
  - Transparent on-chain accounting of allocations and valuations.
  - Hooks for MRV (monitoring, reporting, verification) oracles.
- Provide governance-controlled policies for:
  - Asset acquisition and disposal,
  - Risk haircuts and valuations,
  - Legal entity relationships.

Key properties:
- On-chain state focuses on:
  - commitments to legal documents,
  - asset identifiers and valuations.
- Legal entities (trusts/NGOs) are described and linked off-chain but controlled by governance.

### 3.8 Node Reward Engine

Purpose:
- Reward distributed nodes that:
  - Store content,
  - Execute microservices,
  - Help run the rollup infrastructure (where applicable),
  - Provide other measurable contributions.

Responsibilities:
- Define contribution metrics:
  - storage proofs,
  - bandwidth served,
  - uptime,
  - microservice job completion,
  - moderation or curation tasks (if applicable).
- Calculate and distribute rewards in CWE credits, within budgets set by governance.
- Coordinate with DMF (Distributed Microservices Fabric) for service manifests and accounting.

Key properties:
- Separated from chain security:
  - Node rewards do not define consensus.
- Anti-gaming design:
  - Limits and checks on self-dealing,
  - Robustness against fake service usage.

---

## 4. zk-Rollup Layer: Roles and Components

The CWE rollup includes:

- **Sequencer(s)**
  - Receive transactions from users and nodes.
  - Order and include them into L2 blocks.
  - Provide fast “soft” confirmation.

- **Rollup State Machine**
  - Applies transactions to the L2 state:
    - Work registry changes,
    - Usage proof acceptances,
    - DAPR calculations,
    - Governance actions,
    - Treasury movements.

- **Prover**
  - Periodically generates zk proofs that:
    - “All L2 transitions over this span of blocks are valid.”
  - Posts these proofs and compressed data to Ethereum L1.

- **Full Nodes**
  - Maintain complete L2 state.
  - Verify rollup blocks (and optionally proofs).
  - Serve data to clients and auxiliary services.

- **Bridge Contracts (on L1)**
  - Hold the root of the L2 state.
  - Verify zk proofs.
  - Handle asset bridging between L1 and L2.
  - Provide an emergency recovery path if sequencers misbehave.

---

## 5. Data & State Model (Conceptual)

This section is intentionally conceptual; detailed type definitions belong in module-specific specs.

Key state groupings:

- Work Registry State:
  - Mapping from WorkId to:
    - creator/root identity,
    - pricing profile,
    - split profile,
    - status,
    - metadata references.

- Usage Aggregates:
  - Per (epoch, work, tier):
    - usage_units (e.g. normalized playtime).

- Payout State:
  - Per epoch:
    - total revenue,
    - total fees,
    - creator pool amount,
    - Eco-Reserve allocation,
    - per-work payout commitments.

- Identity & Governance State:
  - Verified creator/listener identities (via commitments).
  - Governance tokens (one-human-one-vote).
  - Proposal and vote records.

- Treasury State:
  - Balances of:
    - CWE credits,
    - Eco-Reserve shares,
    - Infrastructure funds.

- Node Rewards State:
  - Registered node descriptors.
  - Metrics commitments (for storage, compute, etc.).
  - Pending and claimed rewards.

---

## 6. Key Flows

### 6.1 Work Registration Flow

1. Creator authenticates using an identity solution (SSI/VC).
2. Client tool:
   - Computes media fingerprint(s).
   - Prepares metadata (off-chain).
3. Client sends transaction to WorkRegistry:
   - work_id,
   - media_type,
   - pricing_profile,
   - splits_profile,
   - metadata_ref.
4. Rollup:
   - Validates creator authorization via Identity layer.
   - Writes new Work record in L2 state.
5. zk-rollup proof later attests that:
   - Only authorized creators registered works.
   - No duplicate work_ids were accepted.

### 6.2 Usage Reporting & zk-Proof Flow

1. Client locally logs usage events:
   - Which works were consumed,
   - For how long,
   - Under what tier.

2. At the end of a usage window (e.g., daily or weekly):
   - Client aggregates usage into a private data structure.
   - Client generates a zero-knowledge proof that:
     - Usage is consistent with:
       - tier limits,
       - local rules,
       - syntax constraints.
     - It is within plausible global bounds.

3. Client submits:
   - zk proof,
   - a commitment (hash) to their local usage structure,
   - minimal public data needed for aggregation.

4. UsageProofs module:
   - Verifies the zk proof.
   - Updates aggregate usage totals per (epoch, work, tier).

5. Rollup periodically:
   - Proves (to L1 via zk proof) that:
     - All accepted usage proofs were valid.
     - Aggregated totals are correct.

### 6.3 DAPR Epoch Payout Flow

1. At epoch end (e.g., weekly):
   - DAPR reads:
     - Tier revenue for the epoch,
     - Work usage aggregates,
     - Pricing profiles.

2. DAPR computes:
   - Per-work payout amounts.
   - Shares for:
     - creators,
     - collaborators,
     - Eco-Reserve treasury,
     - infrastructure.

3. DAPR publishes:
   - An epoch payout record:
     - total counts,
     - per-work payout commitments.

4. Creators and collaborators:
   - Use their client to:
     - View expected payouts,
     - Submit claim transactions referencing the epoch and work(s).

5. zk-rollup:
   - Executes payout claims as L2 transactions.
   - Posts zk proof to L1 that:
     - All payouts respect the DAPR rules,
     - No double-claims occurred.

### 6.4 Node Reward Flow (Storage/Compute)

1. Nodes register as providers:
   - Storage node, compute node, or both.
   - Provide proof of identity/economic bonding if required.

2. Periodically:
   - Storage nodes generate proofs that they store specific content.
   - Compute nodes provide signed receipts for completed jobs.
   - Metrics may be verified partially by:
     - client attestations,
     - random audits,
     - cross-checks.

3. Node Reward Engine:
   - Aggregates contributions,
   - Applies reward schedules and caps,
   - Creates reward records for nodes.

4. Nodes claim rewards via L2 transactions.
5. zk-rollup proofs ensure:
   - Reward allocations follow rules,
   - No double-counting.

### 6.5 Eco-Reserve Treasury Flow

1. Protocol parameters allocate a fixed share of revenue per epoch to the Eco-Reserve treasury.
2. Treasury contracts:
   - Track balances earmarked for eco investments.
   - Reference off-chain eco-asset entities:
     - land titles,
     - conservation contracts,
     - carbon removal agreements, etc.
3. Oracles and auditors:
   - Periodically push updated valuations and MRV data:
     - total hectares protected,
     - tonnes CO2 removed,
     - risk factors and haircuts.
4. Governance:
   - Decides on large acquisitions or policy changes.
   - Approves or rejects oracle updates.
5. L2 and L1 store:
   - Transparent accounting of:
     - how much CWE credit is held for eco-assets,
     - which types of assets are held,
     - how they evolved over time.

---

## 7. Security Model (Overview)

Security goals:

- Integrity:
  - All state transitions follow the rules.
  - No one can alter past history unnoticed.
- Availability:
  - Users can exit funds from L2 to L1 in emergency conditions.
- Privacy:
  - Usage patterns remain off-chain and private.
- Governance safety:
  - No token-based capture of protocol decisions.

### 7.1 zk-Rollup Security

- zk proofs provide:
  - Mathematical guarantees that all L2 transactions are valid.
- Ethereum L1:
  - Verifies proofs,
  - Maintains canonical state commitments,
  - Acts as the ultimate dispute resolver.

Even if sequencers fail or misbehave:

- Users and full nodes can reconstruct state from data posted to L1.
- A forced exit mechanism allows withdrawal of funds.

### 7.2 Governance Safety

- Governance tokens:
  - Non-transferable (cannot be bought).
  - Issued on a one-human-one-vote basis.
- High-impact changes:
  - Require multi-step approval (e.g., council + vote).
  - May use jury-like panels for disputes.

### 7.3 Economic Safety

- Stable token:
  - Designed to neither reward idle holding nor encourage speculation.
- Node rewards and payouts:
  - Capped and parameterized by governance.
  - Backed by protocol revenue and treasury rules.

---

## 8. Evolvability and Migration

CWE is designed to:

1. Start as a **zk-rollup on Ethereum**:
   - Leverage global security and existing infrastructure.
   - Focus on building the CWE-specific logic and ecosystem.

2. Reserve the option to:
   - Launch a dedicated CWE appchain in the future.
   - Bridge state and assets from the rollup to the appchain.
   - Preserve governance and eco-reserve commitments.

Key requirements for evolvability:

- Clear modular boundaries (WorkRegistry, DAPR, etc.).
- On-chain versioning of contracts and configuration.
- Governance-controlled upgrade mechanisms, with:
  - notice periods,
  - veto/callback options if necessary.

---

## 9. Open Design Questions

To be refined in future versions:

1. Precise epoch length:
   - Weekly vs monthly vs hybrid (usage aggregation vs payout schedule).
2. Exact structure of zk circuits:
   - Which parts of usage and DAPR are in-circuit vs off-circuit.
3. Specific rollup framework:
   - Which zk-rollup stack to adopt (based on maturity, tooling, licensing).
4. Details of eco-reserve valuation:
   - Asset classes,
   - Risk models,
   - Oracle governance.
5. Node reward metrics:
   - Hardening against abuse (fake workloads, collusion).
6. Fiat on/off-ramp strategy:
   - Partner selection,
   - Jurisdictional compliance,
   - UX for non-technical users.

These will be addressed in dedicated module specs and threat models.

---

## 10. Related Documents

- `CHAIN-SYSTEM-REQUIREMENTS.md` – overall chain requirements.
- `CHAIN-WORK-REGISTRY.md` – detailed WorkRegistry spec.
- `CHAIN-USAGE-PROOFS.md` – (to be written) zk usage proof design.
- `CHAIN-GOVERNANCE.md` – (to be written) governance contracts and processes.
- `CHAIN-ECO-TREASURY.md` – (to be written) eco-reserve treasury design.
- `DMF-ARCHITECTURE.md` – (to be written) distributed microservices fabric.

This architecture document is the top-level reference for the CWE chain layer and should be kept in sync with all module-specific specifications.

