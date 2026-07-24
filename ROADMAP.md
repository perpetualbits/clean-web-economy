# Roadmap (High-Level)

> **Detailed, status-annotated roadmap:** [`docs/roadmap.md`](docs/roadmap.md) —
> forward phases, the stub-hardening track, dependencies, and next steps.

## Phase 1 — MVP (Music) ✅ complete
- [x] Browser extension: local accounting + FP lookup stub
- [x] Contracts: tiers, registry, consumption submit, payout ledger
- [x] DAPR simulator end‑to‑end
- [x] Off-chain settlement job + one-command demo (`make -C ops demo`)

See `docs/plans/phase1_mvp_music_implementation_plan.md` and
`docs/plans/phase1_demo.md`.

## Phase 2 — Video & News ✅ complete
- [x] Discovery Hub MVP + OpenAPI (`make -C ops hub-demo`) — see
  `docs/superpowers/specs/2026-07-21-discovery-hub-mvp-design.md`
- [x] Player agent MVP — native Rust `cwe-player` (decode → two-tier recognition →
  price cap → accrual → on-chain settle), `make -C ops player-demo`; the VLC/FFmpeg
  C module is a deferred FFI shim — see
  `docs/superpowers/specs/2026-07-21-player-plugin-mvp-design.md`
- [x] Arbitration jury flow — `CWEJury` committee (file → vote → finalize) whose
  verdict moves the escrow, with earliest-registration fallback; `CWEEscrow` reworked
  to an async dispute; `make -C ops arbitration-demo` — see
  `docs/superpowers/specs/2026-07-22-arbitration-jury-design.md`

## Hardening track (graduate the MVP stubs)
Runs alongside the feature phases.
- [x] **H1 — Recognition & Ownership** ✅ — real Haitsma-Kalker perceptual
  fingerprint, signing-first two-tier recognition (signed content vs. fingerprint
  fallback), multi-party consent provenance, and a `CWEEscrow` + arbiter anti-fraud
  spine (`make -C ops ownership-demo`) — see
  `docs/superpowers/specs/2026-07-21-recognition-and-ownership-design.md`
- [x] **H3 — Full DAPR + anti-fraud** ✅ — `cwe-dapr` user-centric model: diminishing
  returns (play count bound in the commitment), a bandwidth-credibility discount
  (neutral default), and a reputation signal; deterministic + fee-conserving; fraud
  capped and a strict loss under low bandwidth (`make -C ops antifraud-demo`) — see
  `docs/superpowers/specs/2026-07-22-full-dapr-antifraud-design.md`

- [x] **H6 — Verifiable credentials / identity** ✅ — `CWEIdentity`: a rotatable issuer
  set grants revocable, expiring credentials; the registry and jury gate on `isValid`
  instead of owner allowlists (both removed); removing an issuer invalidates their
  credentials (`make -C ops identity-demo`) — see
  `docs/superpowers/specs/2026-07-24-verifiable-credentials-design.md`

Still to come: ZK usage proofs, decentralised settlement, a storage layer (real
bandwidth receipts), SSI/VC identity, tier capability tokens, an epoch
beacon, discovery v2, and security/legal hardening. Details and spec mapping in
[`docs/roadmap.md`](docs/roadmap.md).

## Phase 3 — DMF
- Creator shop template (split‑pay, escrow)
- Service registry & OIDC

## Phase 4 — Governance
- Member registry + voting contracts
- Council elections and proposal lifecycle
