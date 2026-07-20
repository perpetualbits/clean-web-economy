<!-- File: docs/plans/phase1_demo.md -->

# Phase 1 — MVP (Music): End-to-End Demo

This walkthrough runs the smallest complete loop of the Clean Web Economy for
music, entirely on a local devnet. It is the **exit criterion** of Phase 1
(`docs/plans/phase1_mvp_music_implementation_plan.md` §1).

## One command

```sh
make -C ops demo
```

Requirements: [foundry](https://getfoundry.sh) (anvil/forge/cast), a Rust
toolchain (cargo), and `jq`. The demo starts and stops its own Anvil node — no
Docker required.

It runs steps 1–6 of the dev-spec §11 transaction and asserts that every creator
is paid exactly what the settlement computed:

1. **Deploy** the four contracts (`CWETiers`, `CWERegistry`, `CWEConsumption`,
   `CWEPayouts`) plus the accept-all proof verifier.
2. **Register 3 works**, each with a payee and a 100%% split.
3. **Two users subscribe** to the `light` tier, which forwards their fees into the
   `CWEPayouts` pool.
4. **Both users submit** this epoch's usage commitments
   (`keccak256(work ‖ minutes ‖ salt)`).
5. **The settlement job** (`cwe-settlement`) reads the submissions, opens the
   commitments from a disclosure file, runs the shared DAPR payout math, builds
   the credit Merkle tree, and commits its root on-chain.
6. **All three creators withdraw** against their Merkle proofs; the script checks
   each payee's balance matches the settlement.

Expected payouts for the scripted usage (each user pays a 1 ETH tier fee):

| Work | Usage | Credit |
|---|---|---|
| workA | user1 60 min + user2 30 min | 1.00 ETH |
| workB | user1 20 min | 0.25 ETH |
| workC | user2 90 min | 0.75 ETH |

A successful run ends with:

```
✅ DEMO PASSED — every creator's balance matches the settlement exactly.
```

## The interactive (extension) variant

The demo above drives the contracts headlessly through the same Rust code the
extension uses. To exercise the **browser extension** instead:

```sh
cd clients/browser-ext && npm install && npm run build   # -> dist/
```

Load `dist/` as an unpacked MV3 extension, configure the RPC URL, `CWEConsumption`
address, tier id, and a devnet signer key in the options page, play a track from a
page whose fingerprint is in `assets/works.json`, then press **Settle epoch** in
the popup and **Export openings**. Feed the exported openings to the settlement
job as the disclosure file, then withdraw as above.

## What is still a stand-in (Phase 1)

- **Fingerprinting** is a deterministic SHA-256 stub, not perceptual (WP3).
- **Proofs** are hash commitments, not ZK circuits; the disclosure file stands in
  for a zero-knowledge proof of the openings (decision D2).
- **Discovery** is a static `works.json`, not a networked hub (decision D4).
- **Settlement** trusts a single aggregator that commits the Merkle root
  (decision D5).
