# Clean Web Economy (CWE)

**A fair, ad-free internet economy.** Users pay a simple monthly tier; creators get paid directly by usage via privacy‑preserving proofs. No tracking. No middlemen. Open protocols.

## Why
- Consumers are overpaying (multiple subscriptions) and being tracked.
- Creators are underpaid by opaque platforms.
- Ads distort culture and attention.

**CWE fixes this:** flat‑fee tiers → smart contracts split revenue automatically based on what you actually enjoyed. Consumers stay anonymous; creators are verified. Storage and services are decentralized.

## Key Ideas
- **DAPR payouts:** usage × creator price, reconciled to user’s flat fee.
- **ZK privacy:** clients submit zero‑knowledge proofs of consumption.
- **Distributed microservices:** creator shops, ticketing, commissions.
- **Open governance:** 1‑person‑1‑vote DAO; juries for disputes; public audits.

## Get Involved
- Read the [Architecture Blueprint](docs/architecture_blueprint_v0.1.md).
- Run the **Phase 1 demo**: `make -C ops demo` — deploys, subscribes, submits
  usage, settles, and pays creators end-to-end on a local Anvil node. See
  [the walkthrough](docs/plans/phase1_demo.md).
- Join an issue labeled _good first issue_ or propose an [RFC](rfcs/README.md).

## Project Status
Phase 1 (Music MVP) is complete — see the [ROADMAP](ROADMAP.md):
- Browser extension: local accounting + fingerprint-lookup stub (Rust/WASM core)
- Smart contracts: tiers, registry, consumption submit, payout ledger (Foundry)
- DAPR payout simulator and the off-chain settlement job (Rust)
- One-command end-to-end demo

The stack is Rust throughout, except the Solidity contracts and the browser
extension's JS shell.

## Contributing
See [CONTRIBUTING.md](CONTRIBUTING.md). Be kind: [Code of Conduct](CODE_OF_CONDUCT.md).

## License
Code is dual‑licensed AGPL‑3.0 / MPL‑2.0 with a defensive patent pledge. See `LICENSE-AGPL-3.0.txt`, `LICENSE-MPL-2.0.txt`, and `PATENT-PLEDGE.md`.
