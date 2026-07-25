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
- [x] **H2 — ZK usage proofs (cycle 1)** ✅ — a real Groth16/BN254 usage-proof circuit
  (`libs/zk-circuits`) verified on-chain by a real `Groth16Verifier`, a
  `CWEEpochBeacon` publishing the per-epoch key, and Poseidon commitments; settlement
  is dual-mode, paying from proven event weights (event mode) while the disclosure
  file remains the legacy path for the pre-ZK demos (`make -C ops zk-demo`) — see
  `docs/superpowers/specs/2026-07-24-zk-usage-proofs-design.md`
- [x] **H5 — Storage layer + real bandwidth receipts (cycle 1)** ✅ — a minimal
  credentialed `cwe-storage` node serves real content bytes and co-signs bandwidth
  receipts (`cwe-receipt`) with the consumer; settlement verifies both signatures
  and the storage-node credential to compute a real per-(user, work) bandwidth
  credibility that feeds `cwe-dapr`'s discount, turning H3's neutral input live
  (`make -C ops bandwidth-demo`) — a zero-byte usage claim is now a strict loss.
  *Known gap:* a very small claimed weight can still reach full credibility off one
  verified byte, so a dust-weight puppet claim isn't deterred the way an honest claim
  is. *Also known:* receipts attest bytes read, not delivered, and bind no byte range,
  so a modified client can accumulate verified bytes via repeated fragment
  fetch-and-discard requests — strictly cheaper than the dust-weight gap, and
  likewise a deterrent gap rather than a money leak (full accounting in
  `docs/roadmap.md`) — see
  `docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md`

Still to come: H2 cycle 2 (work-identity blinding, manifest-signature and
tier-eligibility circuits, cross-epoch unlinkability, a real randomness beacon,
migrating the legacy demos onto real proofs), H5 cycle 2 (ZK bandwidth proof,
peer-diversity proof, full P2P storage swarm, node compliance/staking, ephemeral-key
unlinkability), decentralised settlement, tier capability tokens, an epoch beacon
upgrade, discovery v2, and security/legal hardening. Details and spec mapping in
[`docs/roadmap.md`](docs/roadmap.md).

## Phase 3 — DMF
- Creator shop template (split‑pay, escrow)
- Service registry & OIDC

## Phase 4 — Governance
- Member registry + voting contracts
- Council elections and proposal lifecycle
