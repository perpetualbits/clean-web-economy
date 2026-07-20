# CWE Player (browser extension)

Manifest V3 browser extension for the Clean Web Economy. It recognises works as
they play, accounts listening time locally, enforces a price cap, and submits
per-epoch usage commitments to the devnet.

## Architecture

The logic is Rust; the browser shell is the thin JS layer the platform requires.

```
core/            Rust -> WASM (wasm-pack): fingerprint, commitment, session accrual
src/background.js service worker: wires the WASM core + hub + policy + chain submit
src/content-script.js  observes <audio>/<video>, reports play/progress/stop
src/policy.js    price-cap decision (pure)
src/hub.js       static works.json resolver (pure) — Phase 1 stand-in for Discovery Hub
src/popup.*      price cap + "Settle epoch" + export openings
src/options.*    RPC URL, contract address, tier id, signer key
assets/works.json  fingerprint -> {work_id, price_per_min, region_factor}
```

The WASM core reuses the workspace libraries (`cwe-fingerprint`, `cwe-wallet-zk`),
so the extension, the settlement job, and the simulator share one implementation.

## Build

```sh
npm install
npm run build        # wasm-pack build core + esbuild bundle -> dist/
```

Load `dist/` as an unpacked extension (chrome://extensions → Load unpacked).

## Test

```sh
npm test             # unit tests (policy, hub) via node --test
npm run test:e2e     # Playwright: loads the built extension, verifies the
                     # resolve -> price-cap -> block path on a real page
```

The Playwright e2e requires the full Chromium (`npx playwright install chromium`);
it loads the extension in new-headless mode.

## Phase 1 notes

- A work is identified by hashing its media **source URL** (deterministic, demo-
  friendly). Tapping decoded audio via WebAudio is the richer path but CORS-limited;
  it is deferred with the real perceptual fingerprint (Phase 2).
- Settlement submits commitments on-chain and exports the **openings** for the
  aggregator (the Phase 1 disclosure file — the manual stand-in for a ZK proof).
- The signer key in the options page is for a **devnet only**.
