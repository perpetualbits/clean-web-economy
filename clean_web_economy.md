# Clean Web Economy — Specification v0.2 (with Distributed Microservices)

**Author:** Roland Nagtegaal & ChatGPT (GPT-5 Thinking)

**Status:** Draft for internal review

**Purpose:** Replace ad-funded, siloed subscriptions with a decentralized, user-funded ecosystem that pays creators directly, protects user privacy, and scales across all digital media.

---

## 1. Vision & Principles

**Vision:** A universal access layer for digital culture where users pay a predictable flat fee and creators receive direct, automatic, transparent payouts—without ads or tracking.

**Principles**

1. **User-funded, not ad-funded**: Flat-fee tiers map to all signed content.
2. **Direct-to-creator**: Smart contracts split revenue to contributors; no opaque intermediaries.
3. **Privacy by default**: Anonymous consumers; verified (non-anonymous) earners.
4. **Market discipline**: Creators set a price/min; users set price thresholds; reputation steers discovery.
5. **Open & decentralized**: Open protocols, distributed storage, verifiable identities, DAO governance.
6. **Sustainability**: Resource-backed currency; node operators rewarded for contributing storage/bandwidth.

---

## 2. Layered Architecture

| Layer                    | Role                                         | Core Components                                                                  |
| ------------------------ | -------------------------------------------- | -------------------------------------------------------------------------------- |
| **Identity**             | Verify creators, preserve consumer anonymity | SSI / EU eID (VCs), wallet bindings, revocation lists                            |
| **Ledger**               | Payments, smart contracts, auditability      | Resource-backed chain; DAPR payouts; ZK proofs for private consumption summaries |
| **Content**              | Hosting & integrity                          | IPFS/BitTorrent hybrid; content IDs; robust fingerprints; optional watermarks    |
| **Access**               | Recognition & UX                             | Browser extensions (WebExtensions), player plugins (VLC/FFmpeg), mobile SDKs     |
| **Moderation**           | Reputation & arbitration                     | User tagging, filter rules, jury-based dispute resolution, blacklists            |
| **Governance**           | Human-centric policy                         | 1-person-1-vote DAO (verified humans), upgrade proposals, treasury               |
| **Microservices Fabric** | Social & B2B economy for creators            | VM/container orchestration, service registry, commerce primitives (below)        |

---

## 3. Economic Model

### 3.1 Tiers (User → Ecosystem)

* **Light**: ≤ 1 hr/day → fee **X₁**
* **Medium**: 2–3 hr/day → fee **X₂**
* **Heavy**: 4–8 hr/day → fee **X₃**

Users pay **X** monthly to a public smart contract wallet bound to their identity (pseudonymous) but with **consumption summaries computed locally** and committed via ZK proofs.

### 3.2 Creator Pricing & DAPR

Creators publish **min fee per minute** (P_c), optionally with regional factor (G). Payout from a user’s contribution **X** is allocated by Demand-Adjusted Pro‑Rata (DAPR):

* (D_{total} = \sum_i (T_i · P_{c,i} · G_i))
* (W_i = (T_i · P_{c,i} · G_i) / D_{total})
* (R_i = X · W_i)

Where (T_i) is local consumption time. **User price threshold** prevents playback when (P_c · G) exceeds user’s cap.

### 3.3 Half‑Life Revenue Decay

Older works’ weights decay (e.g., 100% → 50% over 10 years → asymptote at 10%) to encourage fresh creation while preserving back-catalog value.

---

## 4. Identity, Signing & Fingerprints

1. **Creator Identity:** Verifiable Credentials (eID/SSI) bind legal identity to earning addresses. Revocable; supports org hierarchies (labels, studios, newsrooms).
2. **Work Signing:** Hash + metadata (title, contributors, split graph, price/min) signed by creator key.
3. **Robust Recognition:** Perceptual audio/video fingerprints map re-encodes to canonical IDs. Unsigned media can be voluntarily recognized and paid “out of love.”
4. **Anti-impersonation:** Duplicate-sign detection via similarity search; arbitration resolves disputes.

---

## 5. Reputation & Moderation

### 5.1 Soft Moderation (Discoverability)

* User tags: `Music`, `Movie`, `News`, `Porn`, `Satire`, `Violence`, `Educational`, etc.
* Personal filter rules; social-graph weighting optional.
* Reputation score feeds ranking (never payout directly), indirectly shaping (T_i).

### 5.2 Hard Moderation (Illegality)

* Earners are non-anonymous; illegal-content flags trigger **jury arbitration**.
* On quorum: blacklist content hash; disable earning key; notify relevant jurisdictions.
* Content may remain on decentralized storage but becomes unplayable in ecosystem clients.

---

## 6. Distributed Microservices Fabric (DMF)

**Purpose:** Power the *social, commercial, and collaborative* layer for creators and the creative supply chain—websites, merch, ticketing, commissions, hiring, collaboration tools—without re‑centralizing value.

### 6.1 Design Goals

* **Creator-first UX:** One-click deploy templates (portfolio, storefront, fan club, livestream).
* **Composable services:** Identity, payments, catalog, CRM, analytics (privacy-preserving), chat, ticketing, licensing.
* **Permissionless hosting:** Run at home or in data centers; earn fees by hosting DMF nodes.
* **Interoperable commerce:** All DMF services speak the same payout/contract protocol.

### 6.2 Reference Architecture

* **Execution substrate:** VMs (KVM/Firecracker) hosting container orchestration (Nomad/Kubernetes). Lightweight edge flavor for home nodes.
* **Service registry & mesh:** mTLS, service discovery, rate limits, quota, DDoS-friendly defaults.
* **Data plane:** Encrypted object store (S3-compatible) + CDN gateways; event bus (NATS/Kafka) for real-time interactions.
* **Identity & policy:** OIDC against SSI/VC wallets; fine-grained access via signed capabilities.
* **Payments:**

  * **a. Split-pay primitives** (smart contracts) for instant revenue sharing (contributors, session musicians, producers).
  * **b. Subscription & one-off payments:** merch, tickets, paid messages.
  * **c. Escrow & milestones:** commissions (artwork, mixing, editing) with dispute hooks to arbitration juries.
* **Templates:**

  * Creator Site (bio, catalog, tour dates, shop)
  * Studio/Label Portal (roster, session booking, rights clearance)
  * Marketplace (gigs, casting, session musicians, voice actors)
  * Educational Hub (courses, pay‑per‑module, certification)

### 6.3 Developer Platform

* **Open APIs/SDKs:** JavaScript/TypeScript, Python, Rust clients.
* **Service blueprint schema:** declarative manifest (`service.yml`) describing endpoints, required scopes, pricing, and on-chain contracts.
* **Composability:** Any DMF app can call any other with consented scopes and metered billing.

### 6.4 Privacy-Preserving Analytics

* Local-first metrics; opt-in aggregated stats using differential privacy.
* Zero-knowledge attestations for KPI claims (e.g., “10k unique listeners this month”) without exposing raw logs.

### 6.5 Governance of DMF

* **Registry DAO** curates default templates and approved service classes.
* Security review program; reproducible builds; SBOM requirements.

---

## 7. Storage & Distribution

* **Primary:** IPFS CID addressing with availability incentives; hybrid BitTorrent swarms for large media.
* **Edge:** Community CDN nodes receive a share of network fees; auto-evict unpopular content.
* **Integrity:** All payloads content-addressed; clients verify before playback.

---

## 8. Token & Treasury (Resource-Backed)

* Basket of verified ecological assets backs issuance; auditor-signed proofs decay unless renewed.
* Treasury rules codified by DAO; emissions fund public goods (fingerprint DB, reference clients, audits).
* Node operator rewards adjust dynamically based on demand and availability.

---

## 9. Adoption Strategy

1. **Music-first pilot** with 100–1,000 creators; build testimonial flywheel.
2. **Integrate video & news**; launch discovery hub ranked by reputation (no ads).
3. **Hardware/browser partnerships** for preinstalled plugin; education campaigns on privacy.
4. **Coexistence → displacement**: allow dual-posting; payouts reveal the better model.

---

## 10. Risk Register (Pointers; full audit in separate document)

* Copyright collision / impersonation
* Jurisdictional conflicts (KYC/AML, consumer rights, VAT)
* Moderation abuse / brigading
* Token stability & auditor trust
* UX complexity vs mainstream expectations
* Energy footprint of the chain

*(See: “Clean Web Economy — Risk & Compliance Audit v0.1” for full treatment.)*

---

## 11. Roadmap (Engineering)

* **R0:** DAPR simulator; pricing sensitivity model
* **R1:** Fingerprint DB MVP; client-side recognizer (WASM)
* **R2:** Wallet + tier contract; ZK consumption proof prototype
* **R3:** Creator portal; signing & split-pay contracts; dispute flow
* **R4:** DMF minimal stack (Nomad/K8s, registry, 2–3 templates)
* **R5:** Public pilot; onboarding toolkit; docs and SDKs

---

## 12. Open Questions (for review)

1. Optimal half-life function across domains (music vs film vs news)?
2. Geographical pricing fairness vs arbitrage risk.
3. Standard fingerprint format and openness vs abuse resistance.
4. Minimum viable ZK scheme for usability.
5. Auditor selection and periodic re-validation cadence for resource basket.
6. Anti-Sybil (1 person = 1 vote) without compromising privacy.

---

## 13. Glossary

* **DAPR**: Demand-Adjusted Pro‑Rata payout algorithm.
* **DMF**: Distributed Microservices Fabric for creator-centric apps and commerce.
* **SSI/VC**: Self-Sovereign Identity / Verifiable Credentials.
* **CID**: Content Identifier (IPFS).

---

**End of v0.2**

> Next: produce the separate **Risk & Compliance Audit v0.1** document (legal, technical, and moral hazards with mitigations).

