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
- Try the **devnet**: `make devnet` (see `ops/Makefile`).
- Join an issue labeled _good first issue_ or propose an [RFC](rfcs/README.md).

## Project Status
This is a community build. We’re starting with:
- Browser extension skeleton (content recognition + local accounting)
- Minimal smart contracts (tiers, registry, consumption, payouts)
- DAPR simulation

## Contributing
See [CONTRIBUTING.md](CONTRIBUTING.md). Be kind: [Code of Conduct](CODE_OF_CONDUCT.md).

## License
Code is dual‑licensed AGPL‑3.0 / MPL‑2.0 with a defensive patent pledge. See `LICENSE-AGPL-3.0.txt`, `LICENSE-MPL-2.0.txt`, and `PATENT-PLEDGE.md`.
