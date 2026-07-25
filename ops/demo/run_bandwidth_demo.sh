#!/usr/bin/env bash
#
# Bandwidth-receipt end-to-end demo — the H5 capstone.
#
# Proves that the bandwidth-credibility knob is LIVE: real bytes move over HTTP,
# consumer and storage node co-sign receipts for them, and the aggregator turns
# verified receipts into a per-(user, work) credibility that decides who gets
# paid. Everything runs against a fresh local Anvil node in EVENT mode, so the
# usage half is backed by real Groth16 proofs, not a disclosure file.
#
#   Act 1 (honest):        downloads 512 KiB of work W from a CREDENTIALED node,
#                          co-signs receipts, and is paid in full.
#   Act 2 (puppet work):   claims heavy usage of work F but downloads nothing.
#                          Credibility 0 → its fee is BURNED, F earns nothing.
#   Act 3 (rogue node):    downloads real bytes of work G, but from a node with
#                          no storage-node credential. Receipts are rejected →
#                          same strict loss.
#
# Requirements: foundry (anvil/forge/cast), cargo, jq, curl. No Docker — the
# script starts and stops its own Anvil and storage nodes, killing only the exact
# PIDs it started.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC="http://127.0.0.1:8545"
WORKDIR="$(mktemp -d)"
CONTENT="$WORKDIR/content"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$CONTENT"

step() { echo; echo "=== $* ==="; }
fail() { echo "❌ $*"; exit 1; }

# --- build (release: the prover must not run in debug) ----------------------
step "Building settlement, zk_submit and the storage node (release)"
cargo build --release --quiet -p cwe-settlement -p cwe-storage --manifest-path "$ROOT/Cargo.toml"
SETTLE="$ROOT/target/release/cwe-settlement"
ZK_SUBMIT="$ROOT/target/release/zk_submit"
STORAGE="$ROOT/target/release/cwe-storage"
CLIENT="$ROOT/target/release/bandwidth-client"

# --- regenerate the devnet proving key if missing (fresh checkout) ----------
if [ ! -f "$ROOT/chain/zk/proving_key.bin" ]; then
  step "Regenerating devnet proving key (missing)"
  echo "regenerating devnet proving key (missing)... (~85s, deterministic)"
  ( cd "$ROOT" && cargo run --release --quiet -p cwe-zk-circuits --bin export_keys )
fi

# --- start Anvil (stop only the processes we start) -------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
GOOD_NODE_PID=""
ROGUE_NODE_PID=""
cleanup() {
  # Kill ONLY the exact PIDs this script started.
  [ -n "$GOOD_NODE_PID" ]  && kill -TERM "$GOOD_NODE_PID"  2>/dev/null || true
  [ -n "$ROGUE_NODE_PID" ] && kill -TERM "$ROGUE_NODE_PID" 2>/dev/null || true
  kill -TERM "$ANVIL_PID" 2>/dev/null || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && break; sleep 0.1; done

# Anvil's deterministic dev keys.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                          # owner + issuer + aggregator
U1=${KEYS[1]}                                # honest consumer      (Act 1)
U2=${KEYS[2]}                                # puppet-work claimant (Act 2)
U3=${KEYS[3]}                                # rogue-node consumer  (Act 3)
GOOD_NODE_KEY=${KEYS[4]}                     # credentialed storage node
ROGUE_NODE_KEY=${KEYS[5]}                    # uncredentialed storage node
PAYEE_W=$(cast wallet address ${KEYS[6]})    # creator of the honest work
PAYEE_F=$(cast wallet address ${KEYS[7]})    # creator of the puppet work
PAYEE_G=$(cast wallet address ${KEYS[8]})    # creator of the rogue-served work

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }

# --- step 1: deploy with the real Groth16 verifier ---------------------------
step "1. Deploying contracts (VERIFIER=groth16)"
( cd "$ROOT/chain" && VERIFIER=groth16 PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
CONS=$(jq -r .consumption "$DEP"); BEACON=$(jq -r .beacon "$DEP")
TIERS=$(jq -r .tiers "$DEP"); REG=$(jq -r .registry "$DEP")
IDENTITY=$(jq -r .identity "$DEP")

# --- step 2: epoch beacon key ------------------------------------------------
step "2. Publishing the epoch beacon key"
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
KEY=0x$(printf '5c%.0s' {1..32})
send $DEPLOYER $BEACON "setKey(uint256,bytes32)" $EPOCH $KEY
echo "epoch=$EPOCH"

# --- step 3: credentials, works, tier fee, subscriptions ---------------------
step "3. Registering works, credentialing the storage node, funding subscriptions"
LIGHT=$(cast keccak "light"); FEE=1000000000000000000     # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
FAR=18446744073709551615                                  # type(uint64).max
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE

# The deployer is a trusted issuer and attests itself a verified-creator
# credential so it may register works.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR

# The GOOD node gets a storage-node credential; the ROGUE node deliberately does not.
SN=$(cast keccak "cwe.credential.storage-node")
GOOD_NODE_ADDR=$(cast wallet address $GOOD_NODE_KEY)
ROGUE_NODE_ADDR=$(cast wallet address $ROGUE_NODE_KEY)
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $GOOD_NODE_ADDR $SN $FAR
echo "credentialed node=$GOOD_NODE_ADDR   rogue node=$ROGUE_NODE_ADDR (no credential)"

# Register one work per act, each with its own payee's consent signature.
register_work() {                      # $1 label  $2 payee  $3 payee key
  local work content digest sig
  work=$(cast format-bytes32-string "$1")
  content=$(cast keccak "content-$1")
  digest=$(cast call --rpc-url $RPC $REG \
    "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" "$work" "$content" "$2" "$PPM")
  sig=$(cast wallet sign --private-key "$3" "$digest")
  send $DEPLOYER $REG \
    "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
    "$work" "$content" "[$2]" "[1000000]" "[$sig]" $PPM $EU
  echo "$work"
}
WORK_W=$(register_work bwwork  "$PAYEE_W" "${KEYS[6]}")
WORK_F=$(register_work bwpuppet "$PAYEE_F" "${KEYS[7]}")
WORK_G=$(register_work bwrogue  "$PAYEE_G" "${KEYS[8]}")

# All three users subscribe, each funding the pool with one tier fee.
send $U1 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U2 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U3 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE

# --- step 4: content + rates -------------------------------------------------
# 512 KiB of deterministic content per work. Expected bytes for the demo's
# sample row is weight(45e12) x rate(8192) / 1e12 = 368640, so a full download
# clears expectation and clamps to full credit.
step "4. Publishing content and the aggregator's bandwidth rates"
for W in "$WORK_W" "$WORK_G"; do
  head -c 524288 /dev/zero | tr '\0' 'x' > "$CONTENT/${W,,}.bin"
done
RATES="$WORKDIR/rates.json"
jq -n --arg w "${WORK_W,,}" --arg f "${WORK_F,,}" --arg g "${WORK_G,,}" \
  '{($w): 8192, ($f): 8192, ($g): 8192}' > "$RATES"
echo "content=512KiB per work; rate=8192 bytes per 1e12 weight for all three works"

# --- step 5: start both storage nodes ---------------------------------------
step "5. Starting the credentialed node (8546) and the rogue node (8547)"
CONTENT_DIR=$CONTENT PRIVATE_KEY=$GOOD_NODE_KEY EPOCH=$EPOCH PORT=8546 \
  "$STORAGE" > "$WORKDIR/node-good.log" 2>&1 &
GOOD_NODE_PID=$!
CONTENT_DIR=$CONTENT PRIVATE_KEY=$ROGUE_NODE_KEY EPOCH=$EPOCH PORT=8547 \
  "$STORAGE" > "$WORKDIR/node-rogue.log" 2>&1 &
ROGUE_NODE_PID=$!
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8546/health >/dev/null 2>&1 && break; sleep 0.1; done
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8547/health >/dev/null 2>&1 && break; sleep 0.1; done

cd "$ROOT"   # zk_submit/settlement resolve chain/zk/*.bin from the repo root

# =========================================================================
# ACT 1 — honest: real bytes from a credentialed node
# =========================================================================
step "ACT 1 — honest consumer downloads real bytes and submits proven usage"
RPC_URL=$RPC PRIVATE_KEY=$U1 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_W \
  "$ZK_SUBMIT" --mode honest || fail "honest submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_W,,} PRIVATE_KEY=$U1 EPOCH=$EPOCH \
  CHUNKS=4 CHUNK_LEN=131072 OUT="$WORKDIR/r1.json" "$CLIENT" \
  || fail "honest consumer failed to collect receipts"

# =========================================================================
# ACT 2 — puppet work: usage claimed, no bytes ever moved
# =========================================================================
step "ACT 2 — puppet work claimed with NO downloads"
RPC_URL=$RPC PRIVATE_KEY=$U2 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_F \
  "$ZK_SUBMIT" --mode honest || fail "puppet submit did not succeed"
# Deliberately no receipts for U2.

# =========================================================================
# ACT 3 — rogue node: real bytes, but from an uncredentialed node
# =========================================================================
step "ACT 3 — real bytes served by an UNCREDENTIALED node"
RPC_URL=$RPC PRIVATE_KEY=$U3 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_G \
  "$ZK_SUBMIT" --mode honest || fail "rogue-node submit did not succeed"
STORAGE_URL=http://127.0.0.1:8547 WORK_ID=${WORK_G,,} PRIVATE_KEY=$U3 EPOCH=$EPOCH \
  CHUNKS=4 CHUNK_LEN=131072 OUT="$WORKDIR/r3.json" "$CLIENT" \
  || fail "rogue-node consumer failed to collect receipts"

# --- settle once over all three submissions ----------------------------------
step "Settling the epoch with the combined receipt bundle"
# Merge both consumers' bundles into the single bundle the aggregator verifies.
BUNDLE="$WORKDIR/receipts.json"
jq -s '{epoch: .[0].epoch, receipts: (.[0].receipts + .[1].receipts)}' \
  "$WORKDIR/r1.json" "$WORKDIR/r3.json" > "$BUNDLE"

OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DEPLOYMENTS=$DEP \
  RECEIPTS=$BUNDLE RATES=$RATES OUT=$OUT "$SETTLE" \
  || fail "settlement (event mode + receipts) failed"

# --- assertions ---------------------------------------------------------------
step "Assertions"
credit_of() { jq -r --arg w "$1" '[.entries[] | select(.work_id == $w) | .amount][0] // "0"' "$OUT"; }
CREDIT_W=$(credit_of "${WORK_W,,}")
CREDIT_F=$(credit_of "${WORK_F,,}")
CREDIT_G=$(credit_of "${WORK_G,,}")
TOTAL=$(jq -r '.total_credits' "$OUT"); UNALLOC=$(jq -r '.unallocated' "$OUT")

# 1. The honest work is paid its consumer's whole fee (single row, full credibility).
[ "$CREDIT_W" = "$FEE" ] || fail "honest work earned $CREDIT_W, expected the full fee $FEE"
# 2. The puppet work earned nothing — a strict loss, not a transfer.
[ "$CREDIT_F" = "0" ] || fail "puppet work earned $CREDIT_F, expected 0"
# 3. The rogue-node-backed work earned nothing either.
[ "$CREDIT_G" = "0" ] || fail "rogue-node work earned $CREDIT_G, expected 0"
# 4. The two failed claims' fees were BURNED, not redistributed.
EXPECTED_BURN=$((2 * FEE))
[ "$UNALLOC" = "$EXPECTED_BURN" ] \
  || fail "expected $EXPECTED_BURN wei burned, got $UNALLOC"
# 5. Conservation across all three subscriptions.
SUM=$((TOTAL + UNALLOC)); THREE=$((3 * FEE))
[ "$SUM" = "$THREE" ] || fail "fees not conserved: $TOTAL + $UNALLOC != $THREE"

echo "  honest work paid $(cast to-unit $CREDIT_W ether) ETH"
echo "  puppet work paid 0; rogue-node work paid 0"
echo "  burned $(cast to-unit $UNALLOC ether) ETH; fees conserved ($SUM wei)"

echo
echo "✅ BANDWIDTH DEMO PASSED — real bytes pay, no-bytes and uncredentialed-node claims are a strict loss."
