# `cwe-player` — the desktop player agent

`cwe-player` is a native Rust command-line agent that gives a desktop media
player the same "listen and pay" capability the browser extension gives a
browser. It decodes a local audio file, recognises the work it belongs to
against the Discovery Hub, applies a price cap, accrues listening time locally
across invocations, and — when asked — settles that usage on-chain and
discloses it to the aggregator. It is the desktop analogue of
`clients/browser-ext`, reusing the same Rust core (`cwe-fingerprint`,
`cwe-wallet-zk`) natively instead of compiling it to WASM.

The crate also builds `gen-wav`, a small dev-only tool that writes a
deterministic sine-wave WAV file — used by tests and the end-to-end demo so a
work's content id and fingerprint are stable across runs.

## Subcommands

| Command | What it does |
|---|---|
| `cwe-player play <file>` | Decode `<file>`, recognise it via the hub (signed content id first, perceptual fingerprint fallback), enforce the price cap, and accrue its playback time to the persisted session. Prints which recognition tier matched, or that the file was unrecognised/blocked. |
| `cwe-player status` | Print the current epoch, accrued minutes per work, and which works are escrow-bound — without changing anything. |
| `cwe-player settle` | Flush the epoch's accrued usage into commitments, submit them to `CWEConsumption` on-chain, and write a disclosure file (the openings plus the fingerprint-recognised `escrow_works`) for the aggregator to settle against. |
| `cwe-player fingerprint <file>` | Decode `<file>` and print its `fp:<hex>` perceptual fingerprint. Reuses the exact code path `play` uses for Tier 2 recognition, so it can never drift — handy for building a hub manifest that will actually match what the agent computes at play time. |

## Environment variables

| Variable | Used by | Required | Meaning |
|---|---|---|---|
| `HUB_URL` | all | yes | Discovery Hub base URL, e.g. `http://127.0.0.1:18081`. |
| `RPC_URL` | `settle` | no (defaults to `http://127.0.0.1:8545`) | JSON-RPC endpoint of the target chain. |
| `THRESHOLD` | `play` | no | Price-per-minute cap in ppm; unset allows any price. |
| `STATE` | `play`, `status`, `settle` | no (defaults under the system temp dir) | Path to the persisted session-state JSON file. |
| `DISCLOSURE` | `settle` | no (defaults under the system temp dir) | Where `settle` writes the disclosure JSON. |
| `PRIVATE_KEY` | `settle` | yes (for `settle`) | The agent's own wallet key — it is the listener/user submitting usage. |
| `CONSUMPTION` | `settle` | yes (for `settle`) | The deployed `CWEConsumption` contract address. |
| `TIER_ID` | `settle` | yes (for `settle`) | The `bytes32` subscription tier id the agent submits under. |

`play` and `status` need only `HUB_URL` (plus the optional `THRESHOLD`/`STATE`);
`settle` additionally requires the three chain fields, validated up front so a
half-configured environment fails with a clear message before any transaction
is attempted.

## Recognition tiers and escrow

Recognition mirrors the browser extension exactly:

* **Tier 1 — signed content.** The agent hashes the exact file bytes
  (`keccak256`) and asks the hub for that content id. A hit is authoritative —
  someone registered precisely these bytes as the work — so its usage pays out
  **directly** from `CWEPayouts` once the epoch settles.
* **Tier 2 — perceptual fingerprint.** When the content id misses, the agent
  computes the file's acoustic fingerprint (`cwe-fingerprint`) and asks the hub
  for the nearest registered fingerprint above a similarity bar. A hit here is
  a cautious fallback — the exact bytes were never signed as the work, only
  recognised as sounding like it — so its usage is marked **escrow-bound**:
  `settle` lists the work in the disclosure's `escrow_works`, and the
  aggregator routes its credit to `CWEEscrow` instead of paying it directly.
  It is released (or reassigned, on a successful challenge) only after the
  escrow's challenge window.

An unrecognised file accrues nothing; a recognised file priced above
`THRESHOLD` is blocked before anything accrues. Session state (accrued
per-work seconds and the escrow-bound work set) persists to `STATE` between
invocations, the desktop equivalent of the extension's `chrome.storage`.

## Running the demo

```
make -C ops player-demo
```

This is a self-contained, headless end-to-end run: it starts its own Anvil
node and Discovery Hub, deploys the contracts, registers a signed work and a
fingerprint-matched work, has the agent subscribe and play both files, settles
the epoch as the aggregator, and asserts the signed work's payee is paid
directly while the fingerprint-matched credit sits in `CWEEscrow`. See
`ops/demo/run_player_demo.sh` for the full recipe.

## Deferred: VLC/FFmpeg C module

This crate is the whole agent minus one seam: a real desktop deployment needs
it wired into an actual media player rather than invoked as a one-shot CLI
against pre-existing files. That integration — a thin FFI shim exposing
`decode`/`recognize`/`policy`/`session`/`settle` to a VLC or FFmpeg C module
(feeding it live decoded PCM frames as playback happens, instead of reading a
whole file up front) — is out of scope for this slice and tracked for a later
one. Everything on the Rust side it would call into already exists and is
exercised end to end by this crate's tests and the demo above.
