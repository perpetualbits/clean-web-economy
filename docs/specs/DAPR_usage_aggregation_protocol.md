<!-- File: docs/specs/dapr_usage_aggregation_protocol.md -->

# Clean Web Economy

## DAPR Usage Aggregation Protocol

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This document specifies the **Distributed Aggregate Payout and Ranking (DAPR) Usage Aggregation Protocol**, which converts:

* Anonymous, zero-knowledge usage proofs, and
* Anonymous bandwidth receipts

into **per-work aggregated usage signals** used for:

* Creator payouts
* Discovery-layer ranking
* Reputation metrics
* Ecosystem health monitoring

DAPR is the **economic core** of the Clean Web Economy (CWE), enabling fair rewards to creators while preserving complete user privacy.

---

## 2. Design Goals

DAPR MUST satisfy:

### 2.1 Privacy

* No user-specific information appears in aggregated data.
* No per-user event logs exist.
* No actor can link usage events to users.

### 2.2 Integrity

* All aggregated signals MUST be backed by valid ZK proofs.
* No creator may inflate their own usage beyond the protocol’s constraints.

### 2.3 Scalability

* Designed for millions of users and millions of works.
* Rollup-friendly, batch-verifiable proofs.

### 2.4 Minimal Trust

* Aggregators MUST NOT need to trust clients.
* ZK proofs guarantee correctness.

### 2.5 Economic Fairness

* Rewards scale with real usage.
* Replays are discounted.
* Bandwidth-backed events receive full credit.

---

## 3. High-Level Architecture

DAPR combines:

1. **Client-side ZK Usage Proofs**
2. **Anonymous Bandwidth Receipts**
3. **Epoch-based aggregation services** (DMF microservices)
4. **Chain Layer settlement**

```
Client  ->  DMF Aggregators  ->  Rollup  ->  Chain Settlement  ->  Creator Payouts
```

---

## 4. Definitions

### 4.1 Epoch

Discrete reporting period, e.g. 24h.
Each epoch has a unique `K_epoch` beacon key.

### 4.2 Usage Event

Locally recorded, signed, and committed event representing real content consumption.

### 4.3 Pseudonymized Event

```
P_w,j = H(K_epoch || C_w,j)
```

These are deduplicated by aggregators.

### 4.4 Work Aggregate

For each work `w`, DAPR computes:

* Total weighted usage
* Total bandwidth-backed usage
* Creator price multiplier
* Optional discovery weights

---

## 5. Inputs to DAPR

### 5.1 Validated Proof Bundles

Each client submits a bundle:

```
{
  "event_pseudonyms": [P_1, P_2, ...],
  "proof_usage": "ZK_USAGE",
  "proof_bandwidth": "ZK_BW"    // optional
}
```

### 5.2 ZK Usage Proof Requirements

The bundle MUST prove:

* All events are valid
* No event is double-counted
* All events correspond to signed manifests
* All events belong to this epoch
* No information about works is leaked

### 5.3 Bandwidth Receipts (Optional)

Bandwidth receipts add credibility to usage events.
DAPR MUST accept:

* Usage-only proofs
* Usage + bandwidth proofs

A work with high bandwidth diversity receives **higher credibility weighting**.

---

## 6. Aggregator Responsibilities

Aggregators operate inside the **Distributed Microservice Fabric (DMF)**.

They MUST:

* Verify ZK proofs
* Deduplicate pseudonyms
* Aggregate counts per `manifest_id`
* Apply DAPR weighting rules
* Produce aggregate proofs suitable for rollup submission

They MUST NOT:

* Store or reconstruct per-user events
* Attempt to correlate pseudonyms
* Log client IPs linked to pseudonyms

---

## 7. Aggregation Algorithm

### 7.1 Step 1 — Proof Verification

For each proof bundle:

* Verify `proof_usage`
* Verify `proof_bandwidth` (if present)
* Reject bundle if invalid

### 7.2 Step 2 — Pseudonym Deduplication

Aggregators deduplicate:

```
unique_events = unique(P_w,j)
```

No identity or work ID is revealed.

### 7.3 Step 3 — Work Attribution via MPC / ZK

Using secure multi-party computation or additional ZK proofs:

1. Aggregators hold an **encrypted mapping** of pseudonyms → work indices
2. They compute, without learning the mapping:

```
usage[w] += weight(event)
```

3. MPC ensures no single aggregator learns the mapping

This yields **per-work totals without per-user detail**.

### 7.4 Step 4 — DAPR Weighting

Each work `w` receives weight:

```
W_w = (U_w)^α  *  (B_w)^β  *  P_w
```

Where:

* `U_w` = usage count
* `B_w` = bandwidth credibility score
* `P_w` = creator-defined price multiplier
* α, β = governance-defined exponents

Typical default:

* α = 1.0
* β = 0.35

### 7.5 Step 5 — Finalization

Aggregators produce a rollup-ready structure:

```
{
  "work_id": "MID256",
  "weighted_usage": W_w,
  "raw_usage": U_w,
  "bandwidth": B_w,
  "epoch": E
}
```

This is accompanied by a ZK or SNARK proof verifying:

* All aggregates arise from valid inputs
* No double-counting
* MPC integrity
* All rules applied correctly

---

## 8. DAPR Weighting Model

### 8.1 Diminishing Returns Per User

Usage valuation per user MUST follow:

```
value(j-th_view) = 1 / (1 + k*(j-1))
```

Where `k` is a governance parameter.

This prevents:

* Puppet users inflating creator usage
* Automated replay attacks

### 8.2 Diversity BONUS

Works seen by:

* Many unrelated users
* Across wide storage/microservice regions
* With bandwidth-backed views

receive additional reputation weighting.

### 8.3 Price Multiplier

Creators MAY set a price multiplier:

* 1.0 (default)
* > 1.0 for specialized content
* <1.0 for promotional content

Governance MUST ensure fairness across categories.

---

## 9. Chain Layer Settlement

Aggregators submit final rollup:

* Per-work weighted usage
* Proof of correct aggregation
* Epoch number

Chain contracts compute payouts:

```
payout[w] = (W_w / SUM(W_all)) * epoch_fund_pool
```

Collaborator splits are applied automatically by the contract.

Contracts MUST record:

* Work ID
* Weighted usage
* Payout amount
* Epoch number

Contracts MUST NOT record:

* User pseudonyms
* User event counts
* Any traceable user metadata

---

## 10. Security Requirements

### 10.1 Against Malicious Clients

Clients MUST NOT be able to:

* Inflate usage without bandwidth cost
* Forge pseudonyms
* Reuse pseudonyms across epochs
* Submit invalid ZK proofs
* Bypass diminishing returns

### 10.2 Against Malicious Creators

Creators MUST NOT be able to:

* Collude to generate mass puppet usage
* Reverse-engineer usage patterns
* Access raw aggregation inputs

### 10.3 Against Malicious Aggregators

Aggregators MUST NOT:

* Access unencrypted pseudonym → work mappings
* Produce biased weights
* Exclude honest works

Rollup proofs ensure correctness.

### 10.4 Against Governance Capture

Governance MUST:

* Review weighting parameters regularly
* Publish transparent reports
* Ensure no category is systematically disadvantaged

---

## 11. Performance Requirements

### 11.1 Verification Cost

* ZK proof verification MUST be cheap enough for batch verification (< 100 ms per bundle)

### 11.2 Aggregation Throughput

Aggregators MUST handle:

* Millions of pseudonyms per epoch
* Thousands of works
* Heavy MPC workloads

### 11.3 Rollup Size Limits

Final per-epoch rollup MUST be:

* < a few MB in size
* Verifiable by light clients

---

## 12. Summary

The DAPR Usage Aggregation Protocol transforms:

* Anonymous, zero-knowledge usage events
* Anonymous bandwidth receipts
  into **fair, privacy-preserving payouts** for creators.

It ensures:

* No user information is ever leaked
* No double-counting is possible
* No actor can cheat without economic cost
* All payouts are backed by cryptographic guarantees
* Governance parameters can guide fairness and ecosystem stability

DAPR is the beating heart of the Clean Web Economy — the mechanism that makes it possible to fund culture without tracking people.

