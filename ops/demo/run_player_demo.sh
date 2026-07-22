#!/usr/bin/env bash
#
# Desktop player-agent end-to-end demo (Phase 2.2 exit criterion).
#
# Proves the full desktop "listen and pay" cycle for `cwe-player`, the native
# Rust player agent, against a fresh local Anvil node — entirely headless. It
# exercises both recognition tiers exactly as the agent meets them in the field:
#
#   Tier 1 — signed content (authoritative, direct payout):
#     A creator registers a track; its content id is the keccak256 of its EXACT
#     file bytes. The agent decodes the same file, recognises it by an exact
#     content-id match against the Discovery Hub, and its usage pays out
#     directly from `CWEPayouts`.
#
#   Tier 2 — an unrecognised copy (perceptual fingerprint, escrow):
#     A second work is registered under a content id of its own — deliberately
#     NOT the byte hash of the file the agent is about to play, because nobody
#     signed those exact bytes as the work. The agent plays that different
#     file: its bytes miss Tier 1, but its acoustic fingerprint matches what
#     the hub indexed for the work, so recognition falls back to Tier 2 and the
#     credit is escrowed rather than paid directly.
#
# Concretely:
#   1. deploy the contracts (registry + tiers + consumption + payouts + escrow)
#   2. register a signed work (content id = keccak of its exact file bytes)
#   3. register a second work under its own distinct content id, playable only
#      by fingerprint — the "unsigned copy" scenario
#   4. start the Discovery Hub against the freshly deployed registry
#   5. ingest both works' manifests, signed by the registrant
#   6. the agent subscribes, plays the signed file (Tier 1) and the
#      unrecognised copy (Tier 2), then settles: submits usage commitments
#      on-chain and writes a disclosure marking the fingerprint-matched work
#      escrow-bound
#   7. the settlement job runs as the aggregator, committing the direct root to
#      CWEPayouts and the fingerprint-matched credit to CWEEscrow
#   8. the signed work's payee withdraws — their balance must rise by exactly
#      the settled amount; the fingerprint-matched work's credit must sit in
#      CWEEscrow, unpaid
#
# Requirements: foundry (anvil/forge/cast), cargo, jq, curl. No Docker needed —
# the script starts and stops its own Anvil node and its own hub server.
set -euo pipefail

# Resolve the repo root from this script's location so the demo is path-independent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC="http://127.0.0.1:8545"
# A high, uncommon port: distinct from the hub demo's so both can run without
# colliding if ever invoked back to back on the same machine.
HUB_BIND=127.0.0.1:18081
HUB="http://$HUB_BIND"
WORKDIR="$(mktemp -d)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"

step() { echo; echo "=== $* ==="; }

# --- build the player agent, gen-wav, the hub/signing tools, and settlement -
step "Building cwe-player, gen-wav, the Discovery Hub, and cwe-settlement"
cargo build --quiet -p cwe-player -p cwe-discovery-hub -p cwe-settlement \
  --manifest-path "$ROOT/Cargo.toml"
PLAYER="$ROOT/target/debug/cwe-player"
GENWAV="$ROOT/target/debug/gen-wav"
SIGN="$ROOT/target/debug/sign-manifest"
SETTLE="$ROOT/target/debug/cwe-settlement"

# --- start Anvil (stop only the processes we start) --------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 & ANVIL=$!
# Kill only the PIDs this script itself launched (never by name/pattern), and
# always clean up the scratch workdir, whether the demo passes or fails.
trap 'kill -TERM "$ANVIL" "${HUBPID:-}" 2>/dev/null || true; rm -rf "$WORKDIR"' EXIT
# Wait for Anvil to accept RPC; the 0.25s delay bounds the wait by wall-clock
# time rather than burning 80 tries in a blink of failed connections.
anvil_ready=0
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && { anvil_ready=1; break; }; sleep 0.25; done
[ "$anvil_ready" = "1" ] || { echo "FAIL: Anvil never became ready"; exit 1; }

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                              # owner + aggregator + verified creator + registrant
AGENT=${KEYS[1]}                                 # the desktop player agent's own key (the listener/user)
CREATOR_PAYEE=$(cast wallet address "${KEYS[3]}") # the signed work's sole payee
FP_PAYEE=$(cast wallet address "${KEYS[4]}")      # the fingerprint work's sole payee
DEPLOYER_ADDR=$(cast wallet address "$DEPLOYER")

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
bal()  { cast balance --rpc-url $RPC "$1"; }
# A uint256 `cast call` return, stripped of cast's " [1e18]" pretty annotation so
# the bare decimal can be compared and echoed.
callnum() { cast call --rpc-url $RPC "$@" | sed 's/ .*//'; }

# --- step 1: deploy ----------------------------------------------------------
step "1. Deploying contracts"
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
REG=$(jq -r .registry "$DEP"); TIERS=$(jq -r .tiers "$DEP")
CONS=$(jq -r .consumption "$DEP"); PAY=$(jq -r .payouts "$DEP")
ESCROW=$(jq -r .escrow "$DEP")
echo "registry=$REG payouts=$PAY escrow=$ESCROW"

LIGHT=$(cast keccak "light"); FEE=1000000000000000000  # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE
send $DEPLOYER $REG "setVerifiedCreator(address,bool)" "$DEPLOYER_ADDR" true

# --- step 2: generate the WAV fixtures ---------------------------------------
# 65 seconds each — just over a full minute — because the session store floors
# accrued time to whole minutes and drops anything sub-minute entirely (see
# `cwe-wallet-zk::session::SessionStore::flush`); a shorter clip would settle
# to nothing. The two tones differ, so the files' bytes (and Tier 1 content
# ids) and their acoustic fingerprints are both audibly distinct.
step "2. Generating WAV fixtures (signed.wav @440Hz, copy.wav @550Hz)"
"$GENWAV" "$WORKDIR/signed.wav" 65 440
"$GENWAV" "$WORKDIR/copy.wav" 65 550

# The Tier 1 content id the agent computes at play time is keccak256 of the
# exact file bytes (see `recognize::content_id_of`). Piping the file through
# stdin (rather than passing its hex as a `cast` argument) sidesteps the
# kernel's per-argument length limit, which a multi-megabyte WAV blows past.
CONTENT_SIGNED=$(cast keccak < "$WORKDIR/signed.wav")

# Build one payee's consent signature over the registry's consentDigest —
# copied from `run_ownership_demo.sh`.
consent() {
  local work=$1 content=$2 payee=$3 share=$4 key=$5
  local digest
  digest=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
    "$work" "$content" "$payee" "$share")
  cast wallet sign --private-key "$key" "$digest"
}

# --- step 3: register the signed work (Tier 1) -------------------------------
step "3. Registering the signed work (content id = keccak(signed.wav))"
WORK_SIGNED=$(cast format-bytes32-string "playerSigned")
SIG_SIGNED=$(consent "$WORK_SIGNED" "$CONTENT_SIGNED" "$CREATOR_PAYEE" $PPM "${KEYS[3]}")
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  "$WORK_SIGNED" "$CONTENT_SIGNED" "[$CREATOR_PAYEE]" "[$PPM]" "[$SIG_SIGNED]" $PPM "$EU"

# --- step 4: register the fingerprint-matched work (Tier 2) ------------------
# Its content id is its own, deliberately NOT copy.wav's byte hash: were it
# registered under that hash, the agent would resolve it via Tier 1 (exact
# content match) the instant it played the file, defeating the fixture. This
# mirrors the real "unsigned copy" case — nobody signed these exact bytes as
# the work, so the agent can only ever find it by acoustic fingerprint, and
# that recognition tier is what routes its credit to escrow instead of a
# direct payout.
CONTENT_FP=$(cast keccak "content-revision-of-the-fingerprint-work")
step "4. Registering the fingerprint-matched work (its own distinct content id)"
WORK_FP=$(cast format-bytes32-string "playerFp")
SIG_FP=$(consent "$WORK_FP" "$CONTENT_FP" "$FP_PAYEE" $PPM "${KEYS[4]}")
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  "$WORK_FP" "$CONTENT_FP" "[$FP_PAYEE]" "[$PPM]" "[$SIG_FP]" $PPM "$EU"

# --- step 5: start the Discovery Hub -----------------------------------------
step "5. Starting the Discovery Hub"
REGISTRY=$REG RPC_URL=$RPC BIND=$HUB_BIND SNAPSHOT="$WORKDIR/index.json" \
  "$ROOT/target/debug/cwe-hub" > "$WORKDIR/hub.log" 2>&1 & HUBPID=$!
# Wait for the hub's health endpoint, with a bounded, delayed retry and an
# explicit failure so a hub that never starts is a clear error.
hub_ready=0
for _ in $(seq 1 40); do curl -sf $HUB/healthz >/dev/null 2>&1 && { hub_ready=1; break; }; sleep 0.25; done
[ "$hub_ready" = "1" ] || { echo "FAIL: hub never became ready"; cat "$WORKDIR/hub.log"; exit 1; }

# --- step 6: ingest both manifests, signed by the registrant -----------------
# The fingerprint values ingested here MUST be exactly what the agent computes
# at play time (the same `cwe-fingerprint` code path via `cwe-player
# fingerprint`), or recognition would miss what was indexed.
step "6. Computing fingerprints and ingesting both manifests"
FP_SIGNED=$("$PLAYER" fingerprint "$WORKDIR/signed.wav")
FP_COPY=$("$PLAYER" fingerprint "$WORKDIR/copy.wav")

# $1=work_id $2=content_id $3=fingerprint $4=title $5=payee
manifest() {
  cat <<JSON
{"work_id":"$1","content_id":"$2","fingerprint":"$3","title":"$4","description":"demo","tags":["demo"],"work_type":"audio","price_per_min":$PPM,"region":"$EU","creator_id":"$DEPLOYER_ADDR","created_at":1,"payees":[["$5",$PPM]]}
JSON
}

ENV_SIGNED=$(manifest "$WORK_SIGNED" "$CONTENT_SIGNED" "$FP_SIGNED" "Signed Track" "$CREATOR_PAYEE" \
  | PRIVATE_KEY=$DEPLOYER "$SIGN")
CODE=$(curl -s -o "$WORKDIR/post_signed.out" -w '%{http_code}' -X POST $HUB/manifests \
  -H 'content-type: application/json' -d "$ENV_SIGNED")
[ "$CODE" = "201" ] || { echo "FAIL: signed manifest ingest expected 201, got $CODE"; cat "$WORKDIR/post_signed.out"; exit 1; }

ENV_FP=$(manifest "$WORK_FP" "$CONTENT_FP" "$FP_COPY" "Unsigned Copy" "$FP_PAYEE" \
  | PRIVATE_KEY=$DEPLOYER "$SIGN")
CODE=$(curl -s -o "$WORKDIR/post_fp.out" -w '%{http_code}' -X POST $HUB/manifests \
  -H 'content-type: application/json' -d "$ENV_FP")
[ "$CODE" = "201" ] || { echo "FAIL: fingerprint manifest ingest expected 201, got $CODE"; cat "$WORKDIR/post_fp.out"; exit 1; }

# --- step 7: the agent subscribes and plays both files -----------------------
step "7. Agent subscribes and plays the signed file and the unrecognised copy"
send $AGENT $TIERS "subscribe(bytes32)" $LIGHT --value $FEE

# State persists across the two `play` invocations exactly as it would across
# separate playback sessions on a real desktop.
STATE="$WORKDIR/state.json"
OUT_SIGNED=$(HUB_URL=$HUB STATE=$STATE "$PLAYER" play "$WORKDIR/signed.wav")
echo "  $OUT_SIGNED"
echo "$OUT_SIGNED" | grep -q '\[signed\]' \
  || { echo "FAIL: signed.wav did not recognise as Tier 1 (signed)"; exit 1; }

OUT_COPY=$(HUB_URL=$HUB STATE=$STATE "$PLAYER" play "$WORKDIR/copy.wav")
echo "  $OUT_COPY"
echo "$OUT_COPY" | grep -q 'fingerprint (escrow)' \
  || { echo "FAIL: copy.wav did not recognise as Tier 2 (fingerprint escrow)"; exit 1; }

# --- step 8: the agent settles -----------------------------------------------
step "8. Agent settles: submits usage commitments and writes the disclosure"
DISC="$WORKDIR/disclosure.json"
HUB_URL=$HUB RPC_URL=$RPC STATE=$STATE DISCLOSURE=$DISC PRIVATE_KEY=$AGENT \
  CONSUMPTION=$CONS TIER_ID=$LIGHT "$PLAYER" settle
jq -e --arg w "$WORK_FP" '.escrow_works | index($w) != null' "$DISC" >/dev/null \
  || { echo "FAIL: disclosure escrow_works does not list the fingerprint-matched work"; cat "$DISC"; exit 1; }
echo "  ✓ disclosure marks the fingerprint-matched work escrow-bound"

# --- step 9: settle the epoch as the aggregator ------------------------------
step "9. Running the settlement job as the aggregator"
EPOCH=$(callnum $CONS "currentEpoch()(uint256)")
OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DISCLOSURE=$DISC \
  DEPLOYMENTS=$DEP OUT=$OUT "$SETTLE"

SIGNED_AMT=$(jq -r --arg w "$WORK_SIGNED" '.entries[] | select(.work_id==$w) | .amount' "$OUT")
SIGNED_PROOF=$(jq -r --arg w "$WORK_SIGNED" '.entries[] | select(.work_id==$w) | .proof | join(",")' "$OUT")
FP_ESCROW_AMT=$(jq -r --arg w "$WORK_FP" '.escrow[] | select(.work_id==$w) | .amount' "$OUT")
echo "  signed (direct) credit = $(cast to-unit "${SIGNED_AMT:-0}" ether) ETH; fingerprint (escrow) credit = $(cast to-unit "${FP_ESCROW_AMT:-0}" ether) ETH"
[ -n "$SIGNED_AMT" ] && [ "$SIGNED_AMT" != "null" ] || { echo "FAIL: signed work was not credited directly"; exit 1; }
[ -n "$FP_ESCROW_AMT" ] && [ "$FP_ESCROW_AMT" != "null" ] || { echo "FAIL: fingerprint-matched work was not escrowed"; exit 1; }
DIRECT_FP=$(jq -r --arg w "$WORK_FP" '.entries[] | select(.work_id==$w) | .amount' "$OUT")
[ -z "$DIRECT_FP" ] || { echo "FAIL: fingerprint-matched work paid directly (should be escrowed)"; exit 1; }

# --- step 10: withdraw the direct payout and check the escrow ---------------
step "10. Creator payee withdraws; escrow holds the fingerprint-matched credit"
B0=$(bal "$CREATOR_PAYEE")
send $DEPLOYER $PAY "withdraw(uint256,bytes32,uint256,bytes32[])" "$EPOCH" "$WORK_SIGNED" "$SIGNED_AMT" "[$SIGNED_PROOF]"
GAIN=$(( $(bal "$CREATOR_PAYEE") - B0 ))
[ "$GAIN" = "$SIGNED_AMT" ] || { echo "FAIL: creator payee balance rose by $GAIN, expected $SIGNED_AMT"; exit 1; }
echo "  ✓ creator payee balance rose by exactly the settled amount ($(cast to-unit "$GAIN" ether) ETH)"

ONCHAIN_ESCROW=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" "$EPOCH" "$WORK_FP")
[ "$ONCHAIN_ESCROW" -gt 0 ] || { echo "FAIL: fingerprint-matched work's on-chain escrow balance is not positive"; exit 1; }
[ "$ONCHAIN_ESCROW" = "$FP_ESCROW_AMT" ] || { echo "FAIL: on-chain escrow $ONCHAIN_ESCROW != settled $FP_ESCROW_AMT"; exit 1; }
echo "  ✓ fingerprint-matched credit sits in CWEEscrow, unpaid"

echo
echo "✅ PLAYER DEMO PASSED — signed content paid the creator payee directly;"
echo "   the unrecognised copy's credit was recognised only by fingerprint and escrowed, not paid."
