<!-- File: docs/superpowers/specs/2026-07-21-recognition-and-ownership-design.md -->

# Recognition & Ownership (H1) — Design

**Status:** Approved (design phase)
**Roadmap item:** Hardening H1 — reframed from "perceptual fingerprinting" to a
**signing-first recognition and ownership model** with a cautious fingerprint
fallback.
**Builds on:** Phase 1 (`CWERegistry`, `CWEPayouts`, `cwe-fingerprint`,
`cwe-settlement`, the browser extension) and Phase 2·1 (the Discovery Hub's
chain-anchored **signed manifest** ingest).
**Spec sources:** `docs/specs/content_manifest_and_creator_registration_specification.md`,
`docs/specs/fingerprinting_specification.md` (esp. §9 "Handling Unsigned Content").

---

## 1. Objective and guiding principle

Recognise which work is playing and attribute earnings to its rightful owner —
**primarily by cryptographic proof, never by a fuzzy match alone.**

> **The rule:** signing is authoritative and pays directly; a perceptual
> fingerprint is only a fallback for unsigned content, and its earnings are held
> in escrow behind a challenge window so a misattribution is always recoverable
> before money moves. CWE must never be able to profit by paying a non-owner.

**Exit criterion (`make -C ops ownership-demo`):** on a local devnet, (a) a
**signed** work plays and pays its creator directly; (b) an **unsigned copy** of
that content plays, matches by fingerprint, and its credit goes to **escrow**, not
a payout; (c) a **challenge** with an earlier signed registration reassigns the
escrow to the rightful owner; and (d) after the window with no valid challenge,
escrow releases to the registered owner.

### In scope
- A **real perceptual audio fingerprint** (modest but genuine) in `libs/fingerprint`.
- **Signed recognition:** content manifests bind `content_id = H(content)`; the
  client verifies the creator signature + content hash for authoritative attribution.
- **Escrow + challenge** for fingerprint-matched (unsigned) earnings, with a
  first-signed-registration priority rule and a pluggable **arbitration seam**.
- Hub fingerprint nearest-match; settlement routing (direct vs. escrow); a demo.

### Out of scope (seams / deferred)
- Production-grade fingerprint robustness (Chromaprint/AcoustID quality) — iterative.
- The real **arbitration jury** (Phase 2.3) — this cycle ships a deterministic
  stub behind a seam.
- **SSI/VC** verified-creator identity (H6) — identity stays the registry allowlist.
- **Arbitrary-web content capture** — needs the native **player plugin (Phase 2.2)**;
  this cycle demonstrates on **controlled/served content** (a demo page serving
  content + its signed manifest). CORS limits real-web audio access (a Phase 1 note).

---

## 2. Decisions (locked in brainstorming)

| # | Decision | Choice |
|---|---|---|
| D1 | Cycle scope | Signed path **and** real FP **and** cautious fallback (one larger cycle) |
| D2 | FP-match payout | **Escrow + challenge window** — never auto-pay a fingerprint match |
| D3 | Ownership priority | **Earliest valid signed registration wins**; unresolved → arbitration seam |
| D4 | FP ambition | **Modest but real** Rust acoustic fingerprint (fixed-length, Hamming-compared) |
| D5 | Demo surface | **Served content** (browser real-web capture deferred to the player plugin) |

---

## 3. Recognition model

Two tiers, tried in order, at the moment content plays.

### Tier 1 — Signed (authoritative, pays directly)
A creator publishes a **content manifest** (extends the Phase 2 signed manifest)
binding:
- `content_id = keccak256(content)` for this MVP — a real cryptographic hash of
  the content bytes, **not** the Phase-1 URL stub (a Merkle root over storage
  fragments is a later option once the storage layer, H5, exists);
- `fingerprint` — the perceptual fingerprint (Tier 2 fallback data);
- `creator_id`, splits, price, region — as today;
- a creator **signature** over the canonical manifest.

At playback the client computes `H(content)` from the bytes it has and asks the
hub / chain: is there a registered work whose `content_id` equals this hash, whose
manifest **signature verifies** against the registry, and whose `creator_id` is the
on-chain registrant? If yes → **authoritative recognition**: the usage is credited
on the normal direct-payout path (Phase 1). No fuzzy matching, no fraud surface.

**Key protection:** because an *exact* content hash always resolves to the signed
owner, a creator who signs their content is **immune to fingerprint fraud** on that
content — any play of those exact bytes pays them directly, regardless of who
registered a similar fingerprint. Signing is the owner's best defence, and the
incentive is aligned.

### Tier 2 — Fingerprint (cautious fallback, escrows)
Only reached when Tier 1 fails (no verifiable signature; re-encoded/unsigned bytes).
The client computes a **perceptual fingerprint**; the hub finds the nearest
registered fingerprint within a similarity threshold. A match does **not** pay —
its credit is routed to **escrow** (§5).

---

## 4. Why signing ≠ proof of authorship (stated plainly)

Cryptographic signing proves **possession of the bytes**, **identity** (the
signer's registered key), and **time of registration** — it cannot prove *original
authorship*. A fraudster can sign a manifest over a famous song's bytes with their
own key. The system therefore does **not** treat any single signature as an
authorship oracle. The defence is the combination:

1. **Signed-exact beats fingerprint** — the true owner who signs their content is
   paid directly and cannot be displaced on that exact content (§3, Tier 1).
2. **Fingerprint earnings escrow** — never paid immediately; recoverable (§5).
3. **First-registration priority + arbitration** — disputes resolve by earliest
   valid signed registration; the residual "fraudster registered first" case is
   routed to arbitration with off-chain evidence (a seam this cycle) (§5).

This honest framing is documented in the code and READMEs so CWE is never
represented as guaranteeing authorship it cannot cryptographically prove.

---

## 5. Escrow + challenge (the anti-fraud spine)

Fingerprint-matched credit for `(epoch, work)` enters an **escrow** state instead
of paying out:

- **Commit.** The settlement job marks FP-matched credits as escrowed, attributed
  to the matched `work_id`, with a `release_epoch = current + CHALLENGE_WINDOW`.
- **Challenge.** Within the window, anyone may submit a competing claim: a
  `work_id` they control whose on-chain **registration timestamp is earlier** than
  the escrowed work's. If valid, the escrowed credit **reassigns** to the challenger.
  A dispute that registration time cannot settle is routed to the **arbitration
  seam** — an interface returning a verdict; the Phase 1-style stub returns
  "earliest registration wins", and the Phase 2.3 jury implements it later.
- **Release.** After `release_epoch` with no successful challenge, the (final)
  owner withdraws via the normal split-pay path.

On-chain support:
- `CWERegistry` records a **registration timestamp** per work (priority) and the
  `content_id`.
- A dedicated **`CWEEscrow`** contract (the default; the plan may fold it into
  `CWEPayouts` if that proves simpler) holds FP-attributed funds and implements
  commit / challenge / release, reading registration timestamps from the registry
  and splits for the eventual payout. The arbitration decision is consulted through
  an `IArbiter` interface (accept-earliest stub now).

Signed (Tier 1) credits skip escrow entirely — they pay directly.

---

## 6. The perceptual fingerprint (modest but real)

Replace the SHA-256 stub in `libs/fingerprint` with a genuine acoustic fingerprint,
keeping a compact fixed-length form so it fits in a manifest and supports
nearest-match:

- **Input:** decoded audio samples, downmixed to mono and resampled to a canonical
  rate (e.g. 11025 Hz).
- **Features:** a short-time spectrogram → per-frame sub-band energy (or chroma)
  features → a **fixed-length binary sub-fingerprint sequence** (a compact acoustic
  hash), robust to volume changes and mild re-encoding.
- **API (evolved, same shape):** `compute(samples, opts) -> Fingerprint` where a
  `Fingerprint` now carries the fixed-length bit data; `compare(a, b) -> f64` is a
  **Hamming similarity in `[0.0, 1.0]`** (not the stub's binary); a compact
  `id()`/hex form remains for manifest embedding and exact dedup.
- **Threshold:** a governance-style constant defines "match"; documented and tunable.

Honest scope: this demonstrates robustness to volume and light re-encoding, not the
full adversarial robustness of a production fingerprint — acceptable because Tier 2
is a fallback that never auto-pays.

---

## 7. Component changes

| Component | Change |
|---|---|
| `libs/fingerprint` | real acoustic fingerprint + Hamming `compare`; evolved `Fingerprint` type |
| `chain/CWERegistry` | store `content_id` + registration timestamp; getters |
| `chain` (new) | `CWEEscrow` (or payouts escrow dimension) + `IArbiter` seam + accept-earliest stub |
| `services/discovery-hub` | fingerprint nearest-match index; resolve returns signed-exact vs fp-candidate (+distance) |
| `services/settlement` | route signed → direct payout, fp-match → escrow; process release/reassignment |
| `libs/wallet-zk` / client | content-hash + signature verification (Tier 1); FP compute + escrow-aware status (Tier 2) |
| `ops/` | `make ownership-demo` exercising signed pay, fp→escrow, challenge, release |

---

## 8. Testing

- **Unit:** fingerprint determinism + robustness (same audio under volume/re-encode
  transforms scores above threshold; distinct audio below); Hamming `compare` bounds;
  content-hash match; escrow state machine (commit → challenge-reassign, commit →
  release); the `IArbiter` earliest-registration stub.
- **Contract (Foundry):** registration timestamp + `content_id` getters; escrow
  commit/challenge/release; challenge with an earlier registration reassigns; no
  release before the window; no double-release.
- **Differential:** settlement's routing (signed vs fp) reproduces expected payouts
  and escrow assignments on fixtures.
- **End-to-end (`make ownership-demo`):** the four exit-criterion steps on Anvil.

---

## 9. Risks

| Risk | Mitigation |
|---|---|
| Fingerprint too weak → false matches pay wrong work | Fallback never auto-pays; escrow + challenge + threshold tuning; signed-exact always wins |
| Fraudster registers a famous work first | Signed-exact beats FP for real content; escrow window; arbitration seam for the residual case; documented limitation |
| Content bytes unavailable (CORS) in browser | Demo on served content; real-web capture is the player plugin (Phase 2.2) |
| Escrow/challenge contract complexity/bugs | Small explicit state machine; reentrancy-safe like `CWEPayouts`; thorough Foundry tests |
| `Fingerprint` type change ripples through Phase 1/2 | Keep `compute`/`compare`/`id` shape; update call sites (extension, hub, settlement) deliberately |

---

## 10. Milestones (for the plan to detail)

1. Perceptual fingerprint in `libs/fingerprint` (fixed-length, Hamming `compare`, robustness tests).
2. `CWERegistry`: `content_id` + registration timestamp (+ getters, tests).
3. `CWEEscrow` + `IArbiter` seam + accept-earliest stub (commit/challenge/release, tests).
4. Hub: fingerprint nearest-match index; resolve signed-exact vs fp-candidate.
5. Settlement: route signed → direct, fp → escrow; release/reassignment.
6. Client: content-hash + signature verification (Tier 1); FP + escrow-aware status (Tier 2).
7. `make ownership-demo` end-to-end + docs.
