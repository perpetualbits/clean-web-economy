#!/usr/bin/env bash
#
# Phase 1 end-to-end demo — the exit criterion of the MVP.
#
# Runs steps 1–6 of the dev-spec §11 transaction against a fresh local Anvil node,
# entirely headless (it drives the same contracts and the same Rust settlement job
# the browser extension uses; the extension is the interactive variant):
#
#   1. deploy the four contracts
#   2. register 3 works (each with a payee)
#   3. two users subscribe to a tier (funding the payout pool)
#   4. both users submit this epoch's usage commitments
#   5. the off-chain settlement job runs DAPR, commits the Merkle root
#   6. all three creators withdraw, and balances match the settlement exactly
#
# Requirements: foundry (anvil/forge/cast), cargo, jq. No Docker needed — the
# script starts and stops its own Anvil node.
set -euo pipefail

# Resolve the repo root from this script's location so the demo is path-independent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC="http://127.0.0.1:8545"
WORKDIR="$(mktemp -d)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"

step() { echo; echo "=== $* ==="; }

# --- build the settlement job ---------------------------------------------
step "Building the settlement job"
cargo build --quiet -p cwe-settlement --manifest-path "$ROOT/Cargo.toml"
SETTLE="$ROOT/target/debug/cwe-settlement"

# --- start Anvil (stop only the process we start) --------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
cleanup() { kill -TERM "$ANVIL_PID" 2>/dev/null || true; rm -rf "$WORKDIR"; }
trap cleanup EXIT
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && break; done

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                        # owner + aggregator
U1=${KEYS[1]}; U2=${KEYS[2]}               # two listeners
PAYEE_A=$(cast wallet address ${KEYS[3]})
PAYEE_B=$(cast wallet address ${KEYS[4]})
PAYEE_C=$(cast wallet address ${KEYS[5]})
U1_ADDR=$(cast wallet address $U1)
U2_ADDR=$(cast wallet address $U2)

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }

# --- step 1: deploy --------------------------------------------------------
step "1. Deploying contracts"
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
REG=$(jq -r .registry "$DEP"); TIERS=$(jq -r .tiers "$DEP")
CONS=$(jq -r .consumption "$DEP"); PAY=$(jq -r .payouts "$DEP")
echo "registry=$REG tiers=$TIERS consumption=$CONS payouts=$PAY"

# --- step 2: register 3 works ----------------------------------------------
step "2. Registering 3 works"
LIGHT=$(cast keccak "light"); FEE=1000000000000000000   # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE
send $DEPLOYER $REG "setVerifiedCreator(address,bool)" $(cast wallet address $DEPLOYER) true
WORK_A=$(cast format-bytes32-string "workA")
WORK_B=$(cast format-bytes32-string "workB")
WORK_C=$(cast format-bytes32-string "workC")
CONTENT_A=$(cast keccak "content-workA")
CONTENT_B=$(cast keccak "content-workB")
CONTENT_C=$(cast keccak "content-workC")
# Build a payee's EIP-191 consent signature over the registry's consentDigest:
# read the digest on-chain, then `cast wallet sign` (which applies the
# "\x19Ethereum Signed Message:\n32" prefix and hashes) so the contract's
# ecrecover recovers exactly the signing payee.
consent() {
  local work=$1 content=$2 payee=$3 share=$4 key=$5
  local digest
  digest=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
    "$work" "$content" "$payee" "$share")
  cast wallet sign --private-key "$key" "$digest"
}
SIG_A=$(consent $WORK_A $CONTENT_A $PAYEE_A $PPM ${KEYS[3]})
SIG_B=$(consent $WORK_B $CONTENT_B $PAYEE_B $PPM ${KEYS[4]})
SIG_C=$(consent $WORK_C $CONTENT_C $PAYEE_C $PPM ${KEYS[5]})
WORKS=("$WORK_A" "$WORK_B" "$WORK_C")
CONTENTS=("$CONTENT_A" "$CONTENT_B" "$CONTENT_C")
PAYEES=("$PAYEE_A" "$PAYEE_B" "$PAYEE_C")
SIGS=("$SIG_A" "$SIG_B" "$SIG_C")
for i in 0 1 2; do
  send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
    "${WORKS[$i]}" "${CONTENTS[$i]}" "[${PAYEES[$i]}]" "[1000000]" "[${SIGS[$i]}]" $PPM $EU
done

# --- step 3: subscribe (funds the payout pool) -----------------------------
step "3. Two users subscribe"
send $U1 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U2 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
echo "payout pool: $(cast to-unit $(cast balance --rpc-url $RPC $PAY) ether) ETH"

# --- step 4: submit consumption --------------------------------------------
step "4. Users submit usage commitments"
commit() { cast keccak $(cast concat-hex "$1" $(cast to-uint256 "$2") "$3"); }
SALT1A=0x$(printf '11%.0s' {1..32}); SALT1B=0x$(printf '12%.0s' {1..32})
SALT2A=0x$(printf '21%.0s' {1..32}); SALT2C=0x$(printf '23%.0s' {1..32})
# user1 listens workA 60min, workB 20min; user2 listens workA 30min, workC 90min.
C1A=$(commit $WORK_A 60 $SALT1A); C1B=$(commit $WORK_B 20 $SALT1B)
C2A=$(commit $WORK_A 30 $SALT2A); C2C=$(commit $WORK_C 90 $SALT2C)
send $U1 $CONS "submitConsumption(bytes32,bytes32[],bytes)" $LIGHT "[$C1A,$C1B]" 0x
send $U2 $CONS "submitConsumption(bytes32,bytes32[],bytes)" $LIGHT "[$C2A,$C2C]" 0x
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
echo "epoch = $EPOCH"

# The disclosure file: each user's openings (Phase 1 stand-in for ZK aggregates).
DISC="$WORKDIR/disclosure.json"
cat > "$DISC" <<JSON
{ "users": {
  "${U1_ADDR,,}": [
    { "work_id": "$WORK_A", "minutes": 60, "salt": "$SALT1A" },
    { "work_id": "$WORK_B", "minutes": 20, "salt": "$SALT1B" }
  ],
  "${U2_ADDR,,}": [
    { "work_id": "$WORK_A", "minutes": 30, "salt": "$SALT2A" },
    { "work_id": "$WORK_C", "minutes": 90, "salt": "$SALT2C" }
  ]
} }
JSON

# --- step 5: settle --------------------------------------------------------
step "5. Running the settlement job"
OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DISCLOSURE=$DISC \
  DEPLOYMENTS=$DEP OUT=$OUT "$SETTLE"

# --- step 6: withdraw & verify ---------------------------------------------
step "6. Creators withdraw"
declare -A EXPECT=( [$WORK_A]=1000000000000000000 [$WORK_B]=250000000000000000 [$WORK_C]=750000000000000000 )
declare -A PAYEE=( [$WORK_A]=$PAYEE_A [$WORK_B]=$PAYEE_B [$WORK_C]=$PAYEE_C )
n=$(jq '.entries | length' "$OUT")
fail=0
for i in $(seq 0 $((n-1))); do
  WID=$(jq -r ".entries[$i].work_id" "$OUT")
  AMT=$(jq -r ".entries[$i].amount" "$OUT")
  PROOF=$(jq -r ".entries[$i].proof | join(\",\")" "$OUT")
  before=$(cast balance --rpc-url $RPC ${PAYEE[$WID]})
  send $DEPLOYER $PAY "withdraw(uint256,bytes32,uint256,bytes32[])" $EPOCH $WID $AMT "[$PROOF]"
  after=$(cast balance --rpc-url $RPC ${PAYEE[$WID]})
  gained=$((after - before))
  name=$(cast parse-bytes32-string $WID)
  echo "  $name -> payee gained $(cast to-unit $gained ether) ETH (expected $(cast to-unit ${EXPECT[$WID]} ether))"
  [ "$gained" = "${EXPECT[$WID]}" ] || { echo "  MISMATCH for $name"; fail=1; }
done

echo
if [ "$fail" = "0" ]; then
  echo "✅ DEMO PASSED — every creator's balance matches the settlement exactly."
else
  echo "❌ DEMO FAILED — a balance did not match."; exit 1
fi
