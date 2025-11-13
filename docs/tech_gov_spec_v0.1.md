# Clean Web Economy — Technical Governance Specification v0.1 (Systems Engineering)

**Author:** Roland Nagtegaal & ChatGPT (GPT-5)

**Status:** Draft for architecture review

**Purpose:** Translate the Governance Charter into an implementable, modular, standards-aligned system architecture. Define components, interfaces, data models, processes, states, and compliance requirements for the CWE DAO.

---

## 1. Scope & Non-Goals

### 1.1 Scope

* Governance modules for proposal, voting, delegation, councils, arbitration, treasury, audits, and upgrades.
* Identity and membership registry based on SSI/VC.
* Interoperability with ledger (CLN token), DAPR payouts, and DMF registry.

### 1.2 Non‑Goals

* Media fingerprinting algorithms (separate spec).
* End-user client UX (covered in Access Layer spec).
* Resource-backed token economics (Treasury policy doc).

---

## 2. Architecture Overview

### 2.1 High-Level Components

1. **Identity Service (IDS)** — Verifies humans/orgs via SSI/VC; issues Governance IDs (GIDs).
2. **Membership Registry (MEMREG)** — Maintains member classes, status, capabilities.
3. **Proposal Service (PROP)** — Lifecycle management for proposals.
4. **Voting Service (VOTE)** — Voting logic: person-based, quadratic, supermajority.
5. **Delegation Service (DELEG)** — Graph of vote delegations with revocation.
6. **Council Service (COUNCIL)** — Elections, terms, composition constraints.
7. **Arbitration Service (ARB)** — Jury selection, case handling, appeals.
8. **Treasury Service (TREAS)** — CLN treasury control, grants, disbursements, emission hooks.
9. **Audit & Transparency (AUDIT)** — On-chain/off-chain logs, proofs, dashboards.
10. **Upgrade & Emergency (UPG)** — Controlled contract upgrades, emergency committees.

All state-mutating operations are anchored on-chain; privacy-sensitive pre-processing can occur off-chain with verifiable proofs.

### 2.2 Deployment Topology

* **On-chain contracts**: core registries, proposal/vote state machines, treasury vaults.
* **Off-chain services**: REST/GraphQL gateways, SSI resolvers, analytics, juror selection oracles.
* **Event bus**: publish/subscribe (NATS/Kafka) for proposal/vote/treasury events.

---

## 3. Identity & Membership

### 3.1 Identity Service (IDS)

* **Inputs**: Verifiable Credentials (DID/VC), eIDAS-compatible attestations, org credentials.
* **Outputs**: Governance ID (GID), signed membership claim, revocation list entries.
* **APIs**:

  * `POST /ids/verify`: submit VC bundle → `GID` issuance.
  * `GET /ids/gid/{id}`: resolve current status; returns SSI proofs.
  * `POST /ids/revoke`: revoke VC or binding.

### 3.2 Membership Registry (MEMREG)

* Classes: `consumer`, `creator`, `node_operator`, `auditor`, `observer`.
* States: `active`, `suspended`, `revoked`, `lapsed`.
* **APIs**:

  * `POST /members`: create membership from `GID` + class claims.
  * `PATCH /members/{gid}`: update class, status.
  * `GET /members/{gid}`: read; redacted PII.
* **Constraints**: 1 GID ⇔ 1 human (proof-of-personhood); orgs can have multiple GIDs with org-VC link.

---

## 4. Proposal Lifecycle (PROP)

### 4.1 Types

* `policy`, `technical_upgrade`, `funding_request`, `emergency_action`.

### 4.2 State Machine

`draft → open → voting → passed/failed → executed → archived`

Transitions require:

* **Open**: min 5 co-signers; review docs attached; risk analysis stub.
* **Voting**: min discussion period (7–30 days) satisfied; checks passed.
* **Executed**: on-chain action performed; receipts stored.

### 4.3 APIs

* `POST /proposals`: create draft.
* `POST /proposals/{id}/open`: move to open.
* `POST /proposals/{id}/vote`: cast vote.
* `POST /proposals/{id}/execute`: trigger execution (guarded).
* `GET /proposals/{id}`: full status & artifacts; content-addressed blobs.

---

## 5. Voting (VOTE)

### 5.1 Modes

* **Person-based majority** (default)
* **Supermajority** (2/3) for `technical_upgrade`
* **Quadratic voting** for `funding_request`
* **Council consensus** for `emergency_action` (≥80% of councils)

### 5.2 Ballot Model

```
Ballot {
  proposal_id: CID,
  voter_gid: GID,
  choice: { yes|no|abstain },
  weight: u32,               // derived by mode
  proof_personhood: zkProof?,
  timestamp,
}
```

### 5.3 Delegation (DELEG)

* Delegation graph with cycle detection; last-write-wins.
* `POST /delegate` {from_gid, to_gid, scope: [all|council|proposal_id], ttl}
* `DELETE /delegate/{from_gid}`

### 5.4 Anti‑Sybil

* GID issued only after SSI proof-of-personhood.
* Periodic liveness checks; inactivity decay.

---

## 6. Councils (COUNCIL)

### 6.1 Bodies

* `creators`, `consumers`, `auditor_ethics`.

### 6.2 Elections

* Open-list PR (proportional representation) with term limits.
* Diversity constraints (region, gender, domain) via soft quotas.

### 6.3 APIs

* `POST /councils/{name}/nominate`
* `POST /councils/{name}/vote`
* `GET /councils/{name}/members`

---

## 7. Arbitration (ARB)

### 7.1 Case Types

* Content legality, impersonation, payout disputes, moderation appeals.

### 7.2 Flow

`file → intake triage → juror lottery → hearing → verdict → enforcement → appeal (optional)`

### 7.3 Juror Selection

* Random sample from verified volunteers with stake-weighted reputation floor.
* Conflict-of-interest screening (creator ties, region, orgs).

### 7.4 APIs

* `POST /arb/cases`
* `POST /arb/cases/{id}/vote`
* `GET /arb/cases/{id}`

---

## 8. Treasury (TREAS)

### 8.1 Vaults

* **Core Treasury**: DAO operational budget.
* **Grants Pool**: ecosystem/public goods.
* **Reserve**: stability and emergency fund.

### 8.2 Controls

* Multi-sig guardians; time-locked disbursements.
* Spending classes: caps, rate limits, and audit hooks.

### 8.3 APIs

* `POST /treas/transfers`
* `GET /treas/ledger` (append-only, on-chain mirrored)

---

## 9. Data Models

### 9.1 Proposal (CID-referenced)

```
Proposal {
  id: UUID,
  type: enum,
  title: string,
  abstract: string,
  body_cid: CID,
  cosigners: [GID],
  open_at, close_at,
  risk_summary: string,
  attachments: [CID],
}
```

### 9.2 Member

```
Member {
  gid: GID,
  classes: [enum],
  status: enum,
  created_at,
}
```

### 9.3 Council Seat

```
Seat {
  council: enum,
  gid: GID,
  term_start, term_end,
}
```

---

## 10. Processes & Sequence Flows

### 10.1 Standard Proposal

1. Author drafts → uploads body to content-addressed store.
2. Collect co-signers → open.
3. Discussion window; risk bot posts checklist.
4. Voting opens; DELEG applies.
5. Tally by mode; verifiable transcript published.
6. Execute; receipts logged.

### 10.2 Emergency Action

1. Council consensus e-vote (≥80%).
2. Time-locked execution with 24–72h community veto window (if safe).

### 10.3 Contract Upgrade

1. Formal spec + audit → proposal → supermajority.
2. Proxy upgrade with rollback plan; canary deployment.

---

## 11. Security & Compliance

* **Zero-knowledge proofs** for personhood and vote validity without exposing PII.
* **mTLS** for all service-to-service traffic.
* **SBOM** + reproducible builds; signed artifacts.
* **Data minimization**: PII remains in SSI wallets; registries store hashes/claims only.
* **Regulatory hooks**: optional regulator observer keys for read-only oversight.

---

## 12. Telemetry & Transparency

* On-chain event indexer → public dashboards (proposal status, turnout, treasury flows).
* Integrity checks: Merkle proofs for logs; daily snapshots.

---

## 13. Performance & Sizing

* Target 1M active members initial; 10 TPS governance actions peak.
* Off-chain batching for ballots; on-chain finality within 1–5 minutes.

---

## 14. Operations

* **Runbooks** for incident response (key compromise, stuck proposal, fork protocol).
* **Disaster recovery**: multi-region nodes, state snapshots, cold keys.

---

## 15. Testing & Verification

* Unit tests for state machines.
* Property-based tests for voting/Delegation graphs.
* Chaos tests for partial outages.
* Formal verification for contract critical paths (TREAS, VOTE, UPG).

---

## 16. Interfaces & Standards

* **DID/VC** (W3C), **OIDC** for session auth.
* **EIP‑2535** (Diamond proxy) or Substrate pallets for modular contracts.
* **OpenAPI/GraphQL** specs for all services.

---

## 17. Repo & Module Layout (Reference)

```
/governance
  /contracts
    vote/        # Voting & delegation
    prop/        # Proposals
    treas/       # Treasury vaults
    upg/         # Upgrades & time-locks
  /services
    ids/         # SSI resolver & GID issue
    memreg/      # Membership registry API
    council/     # Elections
    arb/         # Arbitration
    audit/       # Indexer & dashboards
  /specs
    openapi/     # API definitions
    schemas/     # JSON schemas
  /ops
    runbooks/
    helm/
    terraform/
```

---

## 18. Open Issues

* Personhood proof provider neutrality; avoid centralization.
* Council diversity constraints without hard quotas.
* Privacy budget for analytics (DP parameters).

---

## 19. Appendix — State Machines (UML-like)

* **Proposal:** `draft → open → voting → {passed|failed} → {executed|archived}`
* **Member:** `pending → active → {suspended|revoked|lapsed}`
* **Arbitration Case:** `filed → intake → jury → verdict → enforcement → appeal?`

---

**End of v0.1 (Systems Engineering Spec)**

> Recommended next: Developer-friendly spec with pseudocode, contract interfaces, and example transactions (v0.2).

