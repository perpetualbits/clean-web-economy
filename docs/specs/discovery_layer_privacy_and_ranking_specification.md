<!-- File: docs/specs/discovery_layer_privacy_ranking_specification.md -->

# Clean Web Economy

## Discovery Layer Privacy & Ranking Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

The Discovery Layer enables users to:

* Find content
* Browse categories
* Navigate creators
* Search metadata
* View trending signals

All **without revealing user identity or consumption behavior**.

This document defines:

* Privacy requirements
* Metadata handling rules
* Ranking & reputation algorithms
* Anti-manipulation safeguards
* Interaction with manifests and DAPR
* Protocol constraints to prevent centralization and surveillance

The Discovery Layer MUST empower creators and users while preserving full privacy and resisting adversarial pressure.

---

## 2. Design Principles

### 2.1 Zero Tracking

The Discovery Layer MUST NOT:

* Track user activity
* Log per-user search terms
* Build user profiles
* Use cookies, device IDs, or session IDs

### 2.2 Decentralized Metadata

Metadata MUST be:

* Stored in decentralized storage (IPFS/swarm)
* Signed by creators via manifests
* Indexed by distributed microservices
* Aggregate-only at the discovery layer

### 2.3 Differential Transparency

Creators choose:

* What metadata to publish
* What tags or categories to include

Discovery surfaces this metadata without adding tracking.

### 2.4 Aggregate-Only Ranking

Ranking uses:

* Anonymous aggregated usage from DAPR
* Anonymous bandwidth credibility metrics
* Fingerprint uniqueness signals
* Creator reputation inputs

But never user-level data.

### 2.5 Censorship Minimization

Discovery MUST:

* Remain open to all lawful content
* Resist centralized blacklisting
* Allow federation across providers
* Support mirrored indices

---

## 3. Metadata Model

Discovery Layer indexes **public metadata** from manifests:

* `title`
* `description`
* `tags[]`
* `work_type`
* `fingerprint`
* `creator_id`
* `collaborators[]`
* Optional extended metadata (creator-supplied)

### 3.1 Optional Search Metadata

Creators MAY include structured fields:

* Genre
* Year
* Language
* Duration
* Source links

Discovery MUST NOT modify these.

### 3.2 Forbidden Metadata

Creators MUST NOT include:

* User identifiers
* Behavioral trackers
* Personalized labels
* Dynamic per-user fields

---

## 4. Search & Query Model

### 4.1 Stateless Search Queries

Search endpoints MUST NOT:

* Store queries tied to IPs
* Use cookies or sessions
* Return personalized results

### 4.2 Query Format

```
GET /search?q=term&page=1
```

OR

```
POST /search
{
  "terms": ["term1", "term2"],
  "filters": { "work_type": "audio" }
}
```

### 4.3 No Personalization

Users must see the **same ranking** for the same query regardless of identity.

---

## 5. Ranking Algorithm

The Discovery Layer MUST integrate only **anonymous, aggregated signals**.

### 5.1 Aggregate Inputs

Ranking inputs include:

* `usage_total[w]` from DAPR aggregation
* `bandwidth_credibility[w]` (AFBRP)
* `fingerprint_uniqueness[w]` (duplicate detection)
* Optional creator reputation (`creator_reputation[c]`)

### 5.2 Core Ranking Score

Default formula:

```
R_w = log(1 + usage_total[w])
      + α * log(1 + bandwidth_credibility[w])
      + β * uniqueness_score[w]
      + γ * creator_reputation[c]
```

Governance defines `α, β, γ`.

### 5.3 Recency Boost

Content MAY gain recency boost:

```
recency_boost = exp(- age_in_days / τ)
```

where τ is governance-configured.

### 5.4 Category-Specific Models

Categories (audio, video, text) MAY have:

* Distinct scoring parameters
* Normalization per content type
* Independent trending lists

### 5.5 Hard Privacy Constraint

Ranking MUST NOT use:

* User IDs
* User browsing history
* Personalized weighting
* Collaborative filtering models

---

## 6. Anti-Manipulation Defenses

Discovery MUST detect and mitigate malicious attempts to:

* Inflate rankings
* Spam metadata
* Submit misleading tags
* Upload duplicate works

### 6.1 Duplicate Fingerprint Detection

Discovery hubs compute:

```
fingerprint_similarity = sim( fingerprint(w1), fingerprint(w2) )
```

High similarity triggers:

* Warning to creators
* Optional delisting of duplicates

### 6.2 Sybil-Resistant Usage Scores

DAPR inherently applies diminishing returns to bots.
Discovery uses only DAPR output, not raw event counts.

### 6.3 Metadata Spam Prevention

Creators who repeatedly:

* Mislabel content
* Abuse tags
* Submit low-quality metadata
  may receive reduced `creator_reputation`.

### 6.4 Manipulation Detection

DMF nodes can apply anomaly detection to:

* Usage/bandwidth mismatches
* Sudden unnatural spikes
* Cross-category inflation patterns

Always aggregate-only; no per-user info.

---

## 7. Privacy Guarantees

Discovery MUST:

* Never reveal pseudonymized events
* Never reveal usage counts smaller than governance minimum
* Avoid long-tail leakage by applying k-anonymity thresholds on small categories

### 7.1 Differential Privacy (Optional)

Governance MAY require noise injection:

```
usage_total'[w] = usage_total[w] + Laplace(ε)
```

Works well for:

* Rare categories
* Sensitive content

### 7.2 No Cross-Merging of Logs

Discovery hubs MUST NOT merge:

* Access logs
* Query logs
* IP-level analytics

---

## 8. Federation Model

Discovery is a **federated service**, not a centralized index.
Nodes MAY:

* Mirror global index
* Maintain specialized indices
* Provide regional filters (without personalization)

### 8.1 Independent Operators

Universities, NGOs, and cooperatives MAY run discovery nodes.

### 8.2 Consensus on Core Index

DAPR provides universal usage signals.
Disagreement only affects ranking weights, not access.

### 8.3 No Central Gatekeeper

No single entity may:

* Remove content globally
* Dictate ranking rules
* Penalize creators without governance approval

---

## 9. API Definitions

### 9.1 `GET /search`

Return matching content sorted by ranking.

### 9.2 `GET /trending`

Returns trending items:

* Based on weighted usage
* No personalization

### 9.3 `GET /creator/:id`

Returns public profile, metadata, and aggregated reputation.

### 9.4 `GET /manifest/:id`

Returns the manifest from decentralized storage.

---

## 10. Integration With Other Layers

### 10.1 Content Manifests

Discovery uses manifest fields to populate index.

### 10.2 DAPR Aggregation

Discovery uses only the **aggregated usage totals** from DAPR.
No user-level detail is ever consumed.

### 10.3 Fingerprinting

Used to:

* Detect duplicates
* Avoid spam
* Improve quality of listings

### 10.4 Access Microservice

No direct interaction.

### 10.5 Client–Storage Handshake

Discovery does not interact with storage nodes.

---

## 11. Security Requirements

Discovery MUST protect against:

* Metadata poisoning
* Ranking manipulation
* Centralized control
* Surveillance risks

### 11.1 Metadata Poisoning

Nodes validate manifest signatures:

* Only creator-signed manifests allowed
* Invalid metadata dropped

### 11.2 Ranking Manipulation

Nodes cross-check usage inputs against DAPR rollup proofs.

### 11.3 Censorship Resistance

Nodes MUST:

* Mirror global indices
* Provide verifiable logs
* Rotate operators
* Provide community-moderation tools (DAO-governed)

---

## 12. Summary

The Discovery Layer connects users and creators **without surveillance, profiling, or personalization**. It provides:

* Privacy-preserving search
* Transparent ranking
* Sybil-resistant trending
* Decentralized metadata indexing
* Strong anti-manipulation defenses
* Full compatibility with ZK usage proofs and DAPR aggregation

It ensures that CWE remains an **open, fair, privacy-first alternative** to current centralized recommendation platforms.

