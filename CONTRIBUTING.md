# Contributing to CWE

Thanks for helping build a fair web.

## Ways to contribute
- Tackle issues labeled `good first issue`, `help wanted`.
- Draft or review an [RFC](rfcs/README.md).
- Improve docs and tests.

## Development quickstart
```bash
make devnet            # local chain + services
pnpm -C clients/browser-ext install && pnpm -C clients/browser-ext dev
pnpm -C libs/fingerprint build
pnpm -C libs/wallet-zk build

## Commit & PR
- Conventional commits (feat:, fix:, docs:)
- Small PRs, high tests -> faster reviews

## Security

Please report vulnerabilities via SECURITY.md
