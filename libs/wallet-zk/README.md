# cwe-wallet-zk

Portable, chain-agnostic client primitives shared by the browser extension (WP6)
and the off-chain settlement job (WP5). No network or heavy-crypto dependencies —
it compiles cleanly to `wasm32-unknown-unknown`.

## Modules

- **`commit`** — usage commitments (plan decision D2). `keccak256(work_id ‖
  minutes ‖ salt)`. The extension makes a `Commitment` from an `Opening`; the
  settlement job recomputes and `verify`s each opening from the disclosure file.
- **`zk`** — the proof seam. `generate_proof`/`verify_proof` with a Phase 1
  `none-v0` placeholder. Real circuits replace only this module later.
- **`session`** — epoch-aware `SessionStore`: `start`/`add_time`/`stop`/`flush`.
  Its whole state is the serialisable `SessionState`, so the extension persists a
  snapshot to `chrome.storage`. Epochs are 30-day windows, matching
  `CWEConsumption.EPOCH_LENGTH` on-chain.

## Scope note

The `Wallet` signer and `ChainClient` from dev-spec §4.1 are **not** here — they
need a secp256k1/RPC stack that would weigh down this wasm-targeted crate. They
live with the concrete provider integration in the settlement job (WP5) and the
extension's chain layer (WP6).

## Test

```sh
cargo test -p cwe-wallet-zk
cargo build -p cwe-wallet-zk --target wasm32-unknown-unknown  # portability check
```
