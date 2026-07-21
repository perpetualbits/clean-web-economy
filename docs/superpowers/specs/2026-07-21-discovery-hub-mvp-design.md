<!-- File: docs/superpowers/specs/2026-07-21-discovery-hub-mvp-design.md -->

# Discovery Hub MVP — Design

**Status:** Approved (design phase)
**Phase:** 2 (Video & News), sub-project 1 of 3
**Scope source:** `docs/specs/discovery_layer_privacy_and_ranking_specification.md`,
cut down to an MVP per this design.
**Builds on:** Phase 1 (`docs/plans/phase1_mvp_music_implementation_plan.md`) — the
`CWERegistry` contract, the `cwe-fingerprint` and `cwe-wallet-zk` crates, the alloy
chain pattern from `cwe-settlement`, and the browser extension's `HubClient` seam.

---

## 1. Objective and exit criterion

The Discovery Hub is the networked service that turns a fingerprint into "which
work, and who to pay", and lets users find content — **without tracking anyone**.
It replaces the Phase 1 extension's static `works.json` with a live resolver.

**Exit criterion:** one command (`make hub-demo`) starts a local Anvil node and the
hub, registers a work on-chain, publishes a creator-signed manifest to the hub,
then:

1. `GET /resolve/:fingerprint` returns the work's `{work_id, price_per_min, region}`,
2. `GET /search?q=<title term>` returns that work,

and the Phase 1 extension, pointed at the hub, resolves the same fingerprint
through `GET /resolve/:fingerprint` instead of `works.json`.

### In scope
- A Rust `cwe-discovery-hub` service (axum) with an in-memory index.
- Chain-anchored, creator-signed **manifest ingest**.
- **Resolution** (`GET /resolve/:fp`), **search**, **trending**, **manifest** and
  **creator** reads, `healthz`, and an **OpenAPI** document.
- Simple, honest ranking (text relevance; recency-based trending).
- Baseline privacy: stateless, no per-user logging, identical results for all.
- A `cwe-manifest` signing CLI.
- Extension integration: a `NetworkedHubClient` that resolves via the hub with a
  `works.json` fallback.

### Explicitly out of scope (later cycles)
Federation / mirrored indices · differential privacy / k-anonymity thresholds ·
IPFS/decentralized metadata storage · feeding DAPR usage totals into ranking
(trending is recency-only for now, with a documented hook) · creator reputation
modeling · DMF anomaly detection · duplicate **delisting** (only a cheap duplicate
**guard** at ingest is included).

---

## 2. Architecture

A single binary crate, `services/discovery-hub` (crate `cwe-discovery-hub`), added
to the Cargo workspace. It is built from small, independently testable modules:

| Module | Responsibility | Depends on |
|---|---|---|
| `manifest` | `WorkManifest` type, canonical encoding, sign/verify | cwe-wallet-zk, alloy signer |
| `index` | in-memory store + resolve/search/trending + JSON snapshot | (pure) |
| `chain` | alloy client: registry cross-checks | alloy, cwe-wallet-zk |
| `api` | axum routes/handlers; OpenAPI via `utoipa` | axum, index, chain, manifest |
| `config` | bind address, RPC URL, registry address, snapshot path | (std) |
| `main` | wire the above together, run the server | tokio, axum |

The `manifest` and `index` modules carry **no network dependency** and hold the
correctness-critical logic (signing, search), so they are unit-tested in isolation.
`chain` is exercised by the `make hub-demo` end-to-end test, mirroring how Phase 1
split `cwe-settlement`.

Reuse: `cwe-fingerprint` for the `fp:` fingerprint type, `cwe-wallet-zk` for
`Bytes32` and keccak256, and **alloy** for signing/recovery and the registry RPC —
the same stack the settlement job already uses.

---

## 3. Data model

### 3.1 `WorkManifest`

```jsonc
{
  "work_id":      "0x<64 hex>",        // the on-chain CWERegistry work id
  "fingerprint":  "fp:<64 hex>",       // from cwe-fingerprint
  "title":        "string",
  "description":  "string",
  "tags":         ["string", ...],
  "work_type":    "audio" | "video" | "text",
  "price_per_min": 1000000,            // ppm, MUST match the on-chain price
  "region":       "0x<64 hex>",        // the on-chain regionRule (bytes32)
  "creator_id":   "0x<20-byte address>", // MUST equal the on-chain registrant
  "created_at":   1721500000           // client Unix seconds (for recency)
}
```

`price_per_min` and `region` duplicate on-chain fields so the hub can verify they
were not tampered with; they are rejected on mismatch (§4).

Note: `region` is the opaque on-chain `regionRule` **tag** (a `bytes32`), which is
distinct from the numeric "region factor" the settlement/DAPR math uses. The MVP
hub surfaces only the tag; it does not compute or return a region factor.

### 3.2 Canonical encoding and signature

To make a manifest signable and verifiable identically on both sides, the
canonical byte string is the manifest serialized as **JSON with lexicographically
sorted keys and no insignificant whitespace** (UTF-8) — RFC 8785 (JCS).

The signature is an **EIP-191 personal-sign over the canonical bytes directly**:
the signer applies the `"\x19Ethereum Signed Message:\n<len>" ‖ canonical_bytes`
prefix and signs (alloy's `sign_message`), producing a 65-byte `r ‖ s ‖ v`. The
hub recovers the signer address from the same canonical bytes (alloy's
`recover_address_from_msg`) and compares it to the on-chain registrant. (There is
no separate `keccak256(canonical_bytes)` pre-hash step — EIP-191 personal-sign
already hashes the prefixed message.)

Because the signer CLI and the hub both call the **same** `manifest` module for
canonicalization and hashing, the encodings cannot drift.

---

## 4. Ingest flow (chain-anchored + signed)

`POST /manifests` with body `{ "manifest": <WorkManifest>, "signature": "0x<130 hex>" }`.

The hub validates, in order, rejecting with a specific error otherwise:

1. **Well-formed** — the manifest parses and `work_type` is one of the allowed values.
2. **Signature** — recover the signer from the signature over the manifest digest.
3. **Registrant match** — the recovered signer equals `CWERegistry.registrant(work_id)`
   (read via alloy) and `creator_id` equals that address.
4. **On-chain agreement** — `work_id` is registered, and the manifest's
   `price_per_min` and `region` equal the on-chain `pricePerMinOf` / `regionRule`.
5. **Duplicate guard** — if the `fingerprint` already maps to a *different*
   `work_id`, reject (a creator re-publishing their own work updates it in place).

On success the manifest is inserted/updated in the index and the snapshot is
written.

> Note: this requires the `CWERegistry` to expose the registrant and region.
> `registrant` is currently private and `regionRule` has no getter; the plan will
> add `registrantOf(workId)` and `regionRuleOf(workId)` view functions (small,
> backward-compatible additions).

---

## 5. API surface

All responses are JSON. Errors use a consistent `{ "error": "<message>" }` body
with an appropriate status code. An OpenAPI 3 document is generated from the
handlers with `utoipa` and served at `/openapi.json`.

| Method & path | Purpose | Response |
|---|---|---|
| `POST /manifests` | Ingest a signed manifest (§4) | `201` `{work_id}` or `4xx` `{error}` |
| `GET /resolve/:fingerprint` | The extension seam | `200` `{work_id, price_per_min, region, work_type}` or `404` |
| `GET /search?q=&type=&page=` | Ranked text search | `200` `{results: [summary], page, total}` |
| `GET /trending?type=` | Recency-ranked list | `200` `{results: [summary]}` |
| `GET /manifest/:work_id` | Full manifest | `200` `<WorkManifest>` or `404` |
| `GET /creator/:address` | A creator's works + count | `200` `{creator_id, works: [summary], count}` |
| `GET /healthz` | Liveness | `200` `{status:"ok", indexed:<n>}` |
| `GET /openapi.json` | OpenAPI document | `200` |

A `summary` is `{work_id, fingerprint, title, work_type, tags, price_per_min}`.

`:fingerprint` is the `fp:<hex>` string (URL-encoded). `page` is 1-based; page size
is a fixed constant (e.g. 20) for the MVP.

---

## 6. Ranking (MVP)

Kept deliberately simple and honest:

- **Search relevance** — a token-match score: the query is lowercased and tokenized;
  a work scores by how many query tokens appear in its `title` (weighted higher) and
  `tags`/`description`. Results below a zero score are omitted. `type` filters by
  `work_type` first.
- **Trending** — `score = usage_total · 0  +  recency_boost`, where
  `recency_boost = exp(-age_days / τ)` (τ a config constant). `usage_total` is a
  field on each indexed work, **0 in the MVP**, wired so a later cycle can feed it
  from DAPR without changing the formula's shape (matching spec §5.2).

No personalization: the same query yields the same order for everyone (spec §4.3).

---

## 7. Privacy (baseline)

Cheap guarantees honored now (spec §2.1, §4.1, §7.2):

- Endpoints are **stateless**: no cookies, sessions, or device/user ids.
- **No per-user logging**: query terms and client IPs are never persisted or logged;
  only aggregate counters (e.g. total requests) may be kept.
- **No personalization**: identical inputs give identical outputs for all callers.

Deferred (documented as future work): k-anonymity minimums, differential-privacy
noise on aggregates, and refusing to surface usage counts below a governance floor.

---

## 8. Extension integration

Add a `NetworkedHubClient` to `clients/browser-ext` that resolves a fingerprint via
`GET {hubUrl}/resolve/:fp`. The background worker chooses it when a `hubUrl` is set
in options, and otherwise falls back to the existing `StaticHubClient`
(`works.json`). The `resolveFingerprint` interface is unchanged, so the content
script and policy code are untouched — this is exactly the seam the Phase 1
extension was built around. Its unit test mirrors the existing `hub.test.mjs`
(a fetch stub standing in for the hub).

---

## 9. Tooling and testing

- **`cwe-manifest` signing CLI** (a `bin` in the hub crate): given a private key and
  the manifest fields, it prints the canonical manifest JSON and its signature,
  ready to `POST`. Reuses the shared `manifest` module and the alloy signer.
- **Unit tests:**
  - `manifest`: canonical encoding is stable; sign → recover round-trips; a tampered
    field or signature fails verification.
  - `index`: resolve hit/miss; search relevance ordering and `type` filtering;
    trending recency ordering; duplicate-fingerprint guard; snapshot save/load
    round-trip.
- **Contract tests (Foundry):** the new `registrantOf` / `regionRuleOf` getters.
- **End-to-end (`make hub-demo`):** start Anvil + hub, register a work on-chain, sign
  a manifest with the registrant key, `POST /manifests`, then assert `GET /resolve`
  and `GET /search` return it, and that a manifest signed by a non-registrant is
  rejected. Runnable in CI.

---

## 10. Risks

| Risk | Mitigation |
|---|---|
| Canonical-JSON encoding drift between signer and verifier | Both call the one shared `manifest` module; a round-trip unit test guards it |
| Registry lacks registrant/region getters | Add small backward-compatible view functions (WP in the plan) |
| In-memory index lost on crash | JSON snapshot on write, load on boot; acceptable for a devnet MVP node |
| Hub coupled to a running chain for ingest | Reads are chain-independent; only ingest needs RPC. Documented; matches the trust model chosen |

---

## 11. Milestones (for the plan to detail)

1. Contract getters (`registrantOf`, `regionRuleOf`) + tests.
2. `manifest` module + signing CLI (pure + alloy signer).
3. `index` module (resolve/search/trending/snapshot).
4. `chain` module (registry cross-checks).
5. `api` + OpenAPI + `main`.
6. Extension `NetworkedHubClient`.
7. `make hub-demo` end-to-end + CI + docs.
