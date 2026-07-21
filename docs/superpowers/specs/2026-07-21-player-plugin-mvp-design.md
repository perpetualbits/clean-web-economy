<!-- File: docs/superpowers/specs/2026-07-21-player-plugin-mvp-design.md -->

# Player Plugin (Phase 2.2) — MVP Design

**Status date:** 2026-07-21
**Cycle:** Phase 2.2 — Player plugin (feature track)
**Depends on:** Phase 1 (contracts, settlement, `cwe-wallet-zk`), Phase 2.1 (Discovery
Hub), H1 (perceptual fingerprint, two-tier recognition, escrow routing).

---

## 1. Objective and guiding principle

Bring the browser extension's "recognise a work, pay its creator" loop to **desktop
media, outside the browser** — as a pure-Rust **player agent** that decodes a local
audio file, recognises the work (signed content first, perceptual fingerprint as a
cautious fallback), applies the user's price cap, accrues listening time, and settles
on-chain. It is the desktop analogue of `clients/browser-ext`, reusing the same Rust
core (`cwe-fingerprint`, `cwe-wallet-zk`) natively rather than through WASM.

**Guiding principle — MVP-first, seam-preserving.** The literal deliverable named in
the roadmap and `dev_friendly_spec_v0.2.md §2` is a VLC/FFmpeg plugin. That plugin is
a *host integration*: a thin C shim that taps a media player's decoder and calls into
recognition + accounting logic. This MVP builds that logic as a standalone,
headlessly-demoable Rust agent first — exactly as Phase 1's `run_demo.sh` stands in
for the interactive extension — and leaves the VLC/FFmpeg C module as an explicit
deferred FFI seam. Proving the mechanism end-to-end is the goal; packaging it into a
specific host is the next slice.

### In scope

- A new workspace crate `clients/player-plugin/` (bin `cwe-player`).
- Audio decode (pure-Rust, `symphonia`) → mono `f32` PCM + sample rate + raw bytes.
- Two-tier recognition via the live Discovery Hub: Tier 1 exact `content_id`
  (authoritative), Tier 2 perceptual fingerprint (escrow-bound).
- Price-cap policy, session time-accrual with persistence between invocations.
- On-chain settle: submit `CWEConsumption.submitConsumption`, export the disclosure
  openings **and** the `escrow_works` set.
- A self-contained headless demo (`make -C ops player-demo`) + CI job, proving a full
  desktop pay-cycle (signed play → creator paid; fingerprint play → escrowed).

### Out of scope (seams / deferred)

- The real **VLC/FFmpeg C module** — a thin FFI shim over this agent, a later slice.
- **Video** fingerprinting (the fingerprint lib and `symphonia` path are audio-only).
- **Real-time / partial-playback** accrual — the MVP decodes a whole track and accrues
  its full duration (`samples / sample_rate`).
- A **static-manifest fallback** for recognition (the extension has one; a trivial
  later add — the agent uses the live hub only).

---

## 2. Decisions (locked in brainstorming)

| # | Decision | Choice |
|---|---|---|
| D1 | MVP shape | **Pure-Rust player agent**; VLC/FFmpeg C module deferred to an FFI shim |
| D2 | Pipeline depth | **Full pay-cycle parity** — recognise → accrue → settle on-chain → creator paid |
| D3 | Media | **Audio-only** (`symphonia`); video is a separate hardening item |
| D4 | Playback model | **Decode-and-accrue** the whole file's duration (deterministic, headless) |
| D5 | Recognition source | **Live Discovery Hub** (Tier 1 content, Tier 2 fingerprint); static fallback deferred |
| D6 | Config | **Environment variables**, mirroring the settlement job and hub |

---

## 3. Architecture

The agent is invoked as discrete one-shot commands (`play`, `settle`), so state that
must survive between them (accrued time, epoch) persists to a snapshot file — the
desktop analogue of the extension's `chrome.storage`. The heavy logic reuses the
workspace's tested crates; the crate itself is glue + a decode + an HTTP hub client +
an alloy signer path.

```
clients/player-plugin/   (crate: cwe-player)
  src/decode.rs     symphonia → { bytes, samples: Vec<f32>, sample_rate }
  src/recognize.rs  hub client (reqwest): content_hash → /resolve/content;
                    else fingerprint → /resolve/fingerprint; -> { work, tier }
  src/policy.rs     allows(price_per_min, threshold) -> bool  (mirror of policy.js)
  src/session.rs    wraps cwe_wallet_zk::session::SessionStore + snapshot file IO
  src/settle.rs     Opening::commit → submitConsumption (alloy) + disclosure export
  src/config.rs     env config (HUB_URL, RPC_URL, PRIVATE_KEY, CONSUMPTION, TIER_ID,
                    THRESHOLD, STATE, DISCLOSURE)
  src/main.rs       CLI: `cwe-player play <file>` | `cwe-player status` | `cwe-player settle`
```

### Reused, not rebuilt

| Need | Source |
|---|---|
| Perceptual fingerprint | `cwe_fingerprint::Fingerprint::compute(&[f32], u32)` → `fp:<hex>` |
| Content id | `Bytes32(cwe_wallet_zk::keccak256(bytes))` (Tier 1 identifier) |
| Usage commitment | `cwe_wallet_zk::commit::Opening::new(work, minutes, salt).commit()` |
| Session accrual | `cwe_wallet_zk::session::SessionStore` (start/add_time/stop/flush/snapshot/from_state) |
| On-chain signer + `sol!` bindings | `alloy` (same stack as `services/settlement`) |
| Disclosure shape | the settlement job's `Disclosure { users, escrow_works }` (JSON contract) |

---

## 4. Data flow (the end-to-end demo)

```
cwe-player play song.wav
  ├─ decode(file)                → bytes, samples, sample_rate
  ├─ content_hash(bytes)         → GET {HUB_URL}/resolve/content/{id}
  │     hit  → work, tier = "signed"        (authoritative)
  │     miss → fingerprint(samples) → GET /resolve/fingerprint/{fp}
  │              hit → work, tier = "fingerprint"  (escrow-bound)
  ├─ policy.allows(work.price_per_min, THRESHOLD)?  → refuse + report if over cap
  ├─ session.start(session_id, work.work_id); accrue duration; persist snapshot
  └─ if tier == "fingerprint": record work_id in the escrow set (persisted)

cwe-player settle
  ├─ session.flush() → [{ work_id, minutes }]
  ├─ for each: fresh 32-byte salt → Opening.commit(); remember the opening
  ├─ CWEConsumption.submitConsumption(TIER_ID, commitments, 0x)    [agent = the user]
  └─ write DISCLOSURE: { users: { <agent>: [openings…] }, escrow_works: [fp works] }

[settlement job, run as the aggregator]
  → routes signed → CWEPayouts (direct), fingerprint → CWEEscrow
  → the signed work's creator withdraws; the fingerprint work's credit is escrowed
```

**Escrow-works parity (a correctness win).** `settle` exports `escrow_works` so a
Tier-2 (fingerprint) play routes to escrow rather than a direct payout. This closes an
open H1 follow-up — the extension's `handleSettle` does not yet export `escrow_works` —
so the player agent is the first client to drive the full two-tier settlement honestly.

---

## 5. Configuration

Read from the environment (`config.rs`), matching the settlement job/hub conventions
so one devnet's variables carry across tools:

| Variable | Required | Default | Meaning |
|---|---|---|---|
| `HUB_URL` | yes | — | Discovery Hub base URL (recognition) |
| `RPC_URL` | no | `http://127.0.0.1:8545` | JSON-RPC endpoint |
| `PRIVATE_KEY` | for `settle` | — | the agent's wallet key (it is the listener/user) |
| `CONSUMPTION` | for `settle` | — | `CWEConsumption` address |
| `TIER_ID` | for `settle` | — | `bytes32` tier the agent submits under |
| `THRESHOLD` | no | unset = allow all | price-per-minute cap (policy) |
| `STATE` | no | `<tmp>/cwe-player-state.json` | session snapshot path |
| `DISCLOSURE` | for `settle` | `<tmp>/disclosure.json` | openings + `escrow_works` output |

`play` needs only `HUB_URL` (+ optional `THRESHOLD`, `STATE`); the chain variables are
required only by `settle`, and their absence is reported before any transaction.

---

## 6. Commands and behaviour

- **`cwe-player play <file>`** — decode, recognise, apply policy, accrue the file's
  duration, persist. Output reports the resolved work, tier, price, and accrued
  minutes; an over-cap work is refused (not accrued) with a clear reason; an
  unrecognised work (no signed match, no fingerprint match, or the hub is unreachable)
  is reported and nothing is accrued.
- **`cwe-player status`** — read-only: print the current session's epoch, the accrued
  usage per work (`work_id`, minutes), and the pending `escrow_works` set, without
  submitting or mutating anything. No chain interaction; needs only `STATE`. It is how
  a user sees "what have I accrued this epoch" before settling.
- **`cwe-player settle`** — flush accrued usage, submit commitments on-chain, and write
  the disclosure. Reports the tx hash and the works settled. With no accrued usage it
  exits with a clear "nothing to settle" message rather than submitting an empty tx.

---

## 7. Testing

**Unit (per module), verifying real behaviour:**
- `decode`: a synthesized WAV round-trips to its expected sample count / duration; a
  corrupt/unsupported input yields a clear error.
- `recognize`: with a mock hub, Tier 1 (content) is preferred over Tier 2
  (fingerprint); a content miss falls back to fingerprint with `tier = "fingerprint"`;
  a total miss returns `None`.
- `policy`: `allows` boundaries (under, equal, over the cap; unset = allow).
- `session`: accrual sums correctly; a snapshot written by one invocation restores in
  the next; flush drains the epoch and clears state.
- `settle`: the built commitment equals `Opening::commit` for the same
  `(work, minutes, salt)`; the emitted disclosure is well-formed and includes
  `escrow_works` for fingerprint-tier plays.

**Integration — `ops/demo/run_player_demo.sh` (`make -C ops player-demo`, CI job
`player-e2e`):** self-contained Anvil + hub, PID-safe cleanup:
1. deploy; start the hub; **synthesize a short WAV fixture** deterministically (a few
   seconds — enough audio to fill the fingerprint window; via a small dev-only
   generator, so no binary blob is committed).
2. register a **signed** work (`content_id = keccak(wav bytes)`, plus its fingerprint);
   sign + ingest its manifest. Also register + ingest a competing **fingerprint-matched**
   work for an unsigned copy (to exercise Tier 2).
3. the agent subscribes (funds the pool); `cwe-player play signed.wav` → Tier 1 hit;
   `cwe-player play copy.wav` → Tier 2 (escrow-bound).
4. `cwe-player settle` → submit both commitments + write the disclosure with
   `escrow_works=[fp work]`.
5. run the settlement job → **assert the signed work's creator is paid directly** and
   the **fingerprint work's credit is escrowed** (not paid). Print `✅ PLAYER DEMO PASSED`.

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| `symphonia` codec coverage / decode variance across formats | MVP demos WAV (lossless, deterministic); FLAC/MP3/OGG supported but the demo pins WAV so the fixture is byte-stable for the Tier-1 `content_id`. |
| Fingerprint needs enough audio to fill its window (H1 note: ~3.16 s) | The demo fixture is several seconds long; short-clip behaviour is the known H1 fingerprint-hardening item, not re-litigated here. |
| Committing a binary media fixture to git | The demo/tests **generate** the WAV deterministically via a dev-only helper; nothing binary is checked in. |
| Config drift vs. the settlement job / hub | Reuse the same env var names and the settlement job's disclosure JSON shape verbatim. |

---

## 9. Component changes

| Area | Change |
|---|---|
| `clients/player-plugin/` (new crate) | `cwe-player` bin: decode, recognize, policy, session, settle, config, CLI |
| `Cargo.toml` (workspace) | add the new member; deps: `symphonia`, `reqwest`, `alloy`, `serde`/`serde_json`, reuse `cwe-fingerprint`/`cwe-wallet-zk` |
| `ops/` | `run_player_demo.sh` + `make player-demo`; CI `player-e2e` job mirroring `ownership-e2e` |
| `clients/player-plugin/` scaffold | the empty `Makefile` + `plugin_stub.c` are removed/replaced by the Rust crate (the C shim returns as the deferred FFI seam) |

---

## 10. Deliverable

`make -C ops player-demo` prints `✅ PLAYER DEMO PASSED`, and the full gate (fmt,
clippy, workspace tests, contracts, existing demos) stays green. The roadmap's Phase
2.2 item moves to ✅, with the VLC/FFmpeg C module recorded as the remaining seam.
