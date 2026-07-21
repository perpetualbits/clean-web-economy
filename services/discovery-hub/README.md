# Discovery Hub

The Discovery Hub (`cwe-discovery-hub`) is the Phase 2 service that lets a
listener go from "I heard this" to "here is the work, its price, and its
region" without leaking who they are or what they listened to. It keeps no
per-user state: every request is answered purely from the public index of
manifests that creators have published.

It ships two binaries:

- **`cwe-hub`** — the HTTP server described below.
- **`sign-manifest`** — a small CLI that signs a work manifest so it can be
  POSTed to the hub (see [Signing a manifest](#signing-a-manifest-sign-manifest)).

## Ingest trust model

A work only becomes discoverable once its creator publishes a **manifest**: a
JSON document (`WorkManifest`, see `src/manifest.rs`) describing the work
(title, description, tags, modality, fingerprint) plus the three fields the
hub re-verifies against the chain — `price_per_min`, `region`, and
`creator_id`. The manifest is signed by the creator over its RFC 8785
canonical JSON bytes (EIP-191 personal-sign), so the hub can recover the
signer's address without trusting the client at all.

On `POST /manifests` the hub (`src/chain.rs`, `validate_ingest`):

1. Recovers the signer's address from the signature.
2. Looks up `work_id` on the `CWERegistry` contract (`isRegistered`,
   `registrantOf`, `pricePerMinOf`, `regionRuleOf`).
3. Rejects if the work isn't registered on-chain.
4. Rejects unless **both** the recovered signer and the manifest's
   `creator_id` equal the on-chain `registrantOf`.
5. Rejects unless the manifest's `price_per_min` and `region` exactly match
   the on-chain values.

Only then is the manifest indexed (and a duplicate-fingerprint claim by a
different work is rejected with `409`). This means the hub trusts the chain,
not the network: for the fields the registry knows — `work_id`, `price_per_min`,
`region`, and the registrant/`creator_id` — nobody can publish, misprice, or
re-region a work they don't control, and no off-chain party (including the hub
operator) can forge a manifest on a creator's behalf.

**Scope of the guarantee.** The `fingerprint`→work binding is **not**
chain-anchored — the registry has no fingerprint concept. A creator asserts their
work's fingerprint in the manifest, and the hub protects it only on a
first-writer-wins basis (the `409` duplicate guard): whoever registers a given
fingerprint first holds it. Robust duplicate detection and anti-theft (perceptual
similarity, delisting, reputation) are deferred to a later cycle, so this MVP does
not prevent a verified creator from claiming an as-yet-unclaimed fingerprint of
content they did not produce.

## Running the server

```bash
REGISTRY=<CWERegistry address> cargo run -p cwe-discovery-hub --bin cwe-hub
```

Configuration is read from the environment (`src/config.rs`):

| Variable   | Required | Default                   | Meaning                                              |
|------------|----------|----------------------------|-------------------------------------------------------|
| `REGISTRY` | yes      | —                          | `CWERegistry` contract address, checked on every ingest |
| `RPC_URL`  | no       | `http://127.0.0.1:8545`   | JSON-RPC endpoint for the chain `REGISTRY` lives on   |
| `BIND`     | no       | `127.0.0.1:8080`          | Socket address the HTTP listener binds to             |
| `SNAPSHOT` | no       | `hub-index.json`          | Path to the index snapshot, loaded at startup and rewritten on every ingest |

The index snapshot is a local file, not a database: it lets a restarted hub
recover its indexed manifests without re-ingesting them, and is git-ignored
(see `.gitignore`) since it's regenerated per-environment.

## API

All responses are JSON. Errors are `{ "error": "<message>" }` with a `4xx`/`5xx`
status. The full machine-readable contract is served at `GET /openapi.json`.

| Method | Path                     | Description                                                         |
|--------|--------------------------|----------------------------------------------------------------------|
| POST   | `/manifests`             | Ingest a signed manifest (`{ manifest, signature }`). `201` on success, `400` on a bad signature/chain mismatch, `409` if the fingerprint is already claimed by another work. |
| GET    | `/resolve/{fingerprint}` | Resolve a `fp:<hex>` fingerprint to the work's payout-relevant fields (the browser extension's seam into the hub). `404` if unknown. |
| GET    | `/search?q=&type=&page=` | Ranked text search over title/tags/description, optionally filtered by `type` (`audio`/`video`/`text`), paginated (fixed page size). |
| GET    | `/trending?type=`        | Recency-ranked list of works, optionally filtered by `type`.         |
| GET    | `/manifest/{work_id}`    | The full manifest for an on-chain work id. `404` if unknown.         |
| GET    | `/creator/{address}`    | A creator's works and their count.                                   |
| GET    | `/healthz`               | Liveness probe; reports how many works are indexed.                  |
| GET    | `/openapi.json`          | The service's OpenAPI 3 document.                                    |

## Signing a manifest (`sign-manifest`)

`sign-manifest` reads a manifest JSON object on stdin, signs its canonical
bytes with the key in `PRIVATE_KEY`, and prints the ready-to-POST envelope on
stdout:

```bash
echo '{
  "work_id": "0x...", "fingerprint": "fp:...", "title": "Blue Ocean",
  "description": "demo", "tags": ["calm"], "work_type": "audio",
  "price_per_min": 1000000, "region": "0x...", "creator_id": "0x...",
  "created_at": 1
}' | PRIVATE_KEY=0x... cargo run -p cwe-discovery-hub --bin sign-manifest \
  > envelope.json

curl -X POST http://127.0.0.1:8080/manifests \
  -H 'content-type: application/json' -d @envelope.json
```

`price_per_min`, `region`, and `creator_id` must match what is registered for
`work_id` on-chain, and `PRIVATE_KEY` must belong to that work's registrant —
otherwise ingest correctly rejects the manifest (see
[Ingest trust model](#ingest-trust-model) above).

## End-to-end demo

`ops/demo/run_hub_demo.sh` exercises the whole flow against a fresh, local,
self-contained Anvil devnet: deploy the contracts, register a work, start the
hub, sign and ingest a manifest, resolve and search for it, then confirm a
manifest signed by a non-registrant is rejected. Run it with:

```bash
make -C ops hub-demo
```

It prints `✅ HUB DEMO PASSED` on success and exits non-zero on any failure.
It is also run in CI as the `hub-e2e` job (see `.github/workflows/ci.yml`).
