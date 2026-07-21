#!/usr/bin/env bash
#
# Discovery Hub end-to-end demo (Phase 2 exit criterion).
#
# Runs the full ingest/discovery loop against a fresh local Anvil node,
# entirely headless: deploy the contracts, register a work on-chain, start the
# hub against that registry, sign a manifest as the registrant and POST it,
# then resolve and search for it, and finally confirm a manifest signed by a
# non-registrant is rejected. Concretely:
#
#   1. deploy the contracts
#   2. register one work (deployer is owner + verified creator + registrant)
#   3. start `cwe-hub` pointed at the freshly deployed registry
#   4. sign a manifest as the registrant and POST it to /manifests -> 201
#   5. GET /resolve/{fingerprint} and /search?q=... and find the work
#   6. sign the same manifest fields as a non-registrant and POST -> 4xx
#
# Requirements: foundry (anvil/forge/cast), cargo, jq, curl. No Docker needed —
# the script starts and stops its own Anvil node and its own hub server.
set -euo pipefail

# Resolve the repo root from this script's location so the demo is path-independent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
RPC=http://127.0.0.1:8545
# A high, uncommon port: 8080 collides with unrelated local services on some
# machines, and this demo must not depend on that port being free.
HUB_BIND=127.0.0.1:18080
HUB=http://$HUB_BIND
WORK="$(mktemp -d)"

# --- build the hub + signing CLI -------------------------------------------
cargo build --quiet -p cwe-discovery-hub --manifest-path "$ROOT/Cargo.toml"

# --- start Anvil (stop only the processes we start) -------------------------
anvil > "$WORK/anvil.log" 2>&1 & ANVIL=$!
trap 'kill -TERM "$ANVIL" "${HUBPID:-}" 2>/dev/null || true; rm -rf "$WORK"' EXIT
# Wait for Anvil to accept RPC. The 0.25s delay bounds the wait by wall-clock time
# (a failed `cast` returns in milliseconds, so without it 80 tries can burn out in a
# blink); fail loudly if it never comes up rather than racing into a deploy.
anvil_ready=0
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && { anvil_ready=1; break; }; sleep 0.25; done
[ "$anvil_ready" = "1" ] || { echo "FAIL: Anvil never became ready"; exit 1; }

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORK/anvil.log" | head -3)
DEPLOYER=${KEYS[0]}                       # owner + verified creator + registrant
OUTSIDER=${KEYS[1]}                       # not registered anywhere
DEPLOYER_ADDR=$(cast wallet address $DEPLOYER)

# --- deploy -------------------------------------------------------------
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol --rpc-url $RPC --broadcast >/dev/null 2>&1 )
REG=$(jq -r .registry "$ROOT/chain/deployments/localhost.json")

# --- register a work on-chain (deployer is owner + verified creator + registrant) ---
send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
send $DEPLOYER $REG "setVerifiedCreator(address,bool)" $DEPLOYER_ADDR true
WORK_ID=$(cast format-bytes32-string "workA"); EU=$(cast format-bytes32-string "EU")
PAYEE=$(cast wallet address ${KEYS[2]})
send $DEPLOYER $REG "registerWork(bytes32,address[],uint96[],uint256,bytes32)" \
  $WORK_ID "[$PAYEE]" "[1000000]" 1000000 $EU

# --- start the hub, pointed at the freshly deployed registry ----------------
REGISTRY=$REG RPC_URL=$RPC BIND=$HUB_BIND SNAPSHOT="$WORK/index.json" "$ROOT/target/debug/cwe-hub" & HUBPID=$!
# Wait for the hub's health endpoint, with a bounded, delayed retry and an explicit
# failure so a hub that never starts is a clear error, not a confusing later POST.
hub_ready=0
for _ in $(seq 1 40); do curl -sf $HUB/healthz >/dev/null 2>&1 && { hub_ready=1; break; }; sleep 0.25; done
[ "$hub_ready" = "1" ] || { echo "FAIL: hub never became ready"; exit 1; }

FP="fp:$(printf 'a%.0s' {1..64})"
manifest() { cat <<JSON
{"work_id":"$WORK_ID","fingerprint":"$FP","title":"Blue Ocean","description":"demo","tags":["calm"],"work_type":"audio","price_per_min":1000000,"region":"$EU","creator_id":"$1","created_at":1}
JSON
}

# --- sign as the registrant and POST -> expect 201 --------------------------
ENVELOPE=$(manifest $DEPLOYER_ADDR | PRIVATE_KEY=$DEPLOYER "$ROOT/target/debug/sign-manifest")
CODE=$(curl -s -o "$WORK/post.out" -w '%{http_code}' -X POST $HUB/manifests -H 'content-type: application/json' -d "$ENVELOPE")
[ "$CODE" = "201" ] || { echo "FAIL: ingest expected 201, got $CODE"; cat "$WORK/post.out"; exit 1; }

# --- resolve + search --------------------------------------------------------
curl -sf "$HUB/resolve/$FP" | jq -e '.work_id' >/dev/null || { echo "FAIL: resolve"; exit 1; }
curl -sf "$HUB/search?q=ocean" | jq -e '.results[0].title == "Blue Ocean"' >/dev/null || { echo "FAIL: search"; exit 1; }

# --- a manifest signed by a non-registrant must be rejected (4xx) -----------
BAD=$(manifest $(cast wallet address $OUTSIDER) | PRIVATE_KEY=$OUTSIDER "$ROOT/target/debug/sign-manifest")
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST $HUB/manifests -H 'content-type: application/json' -d "$BAD")
[ "${CODE:0:1}" = "4" ] || { echo "FAIL: non-registrant expected 4xx, got $CODE"; exit 1; }

echo "✅ HUB DEMO PASSED — ingest, resolve, search, and rejection all correct."
