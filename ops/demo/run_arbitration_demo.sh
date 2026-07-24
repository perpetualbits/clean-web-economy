#!/usr/bin/env bash
#
# Arbitration-jury end-to-end demo (Phase 2.3 exit criterion).
#
# Proves the trusted committee (`CWEJury`) can OVERTURN the earliest-
# registration default when it has a considered reason to: a fraudster who
# registered first and a real owner who registered second, disputing the
# SAME content. Left to the timestamp rule alone, the fraudster would win
# every time (first registration wins ties/silence). This demo shows the
# committee majority-voting the real owner into the escrow instead, and the
# money following that verdict rather than the clock.
#
# Concretely:
#   1. deploy the contracts (registry + payouts + escrow + jury)
#   2. appoint a 3-juror committee on the jury
#   3. the FRAUDSTER registers first (earliest timestamp) on some content
#   4. the REAL owner registers second, claiming the SAME content
#   5. a listener plays the fraudster's (fingerprint-matched) copy; settlement
#      escrows the credit under the fraudster's work id
#   6. the real owner challenges the escrow -> opens a jury dispute
#   7. two of three jurors vote for the real owner (majority, NOT unanimous)
#   8. the voting window closes; finalize tallies the REAL owner as the
#      verdict -- overturning what earliest-registration alone would decide
#   9. resolveDispute reassigns the escrowed credit off the fraudster's work
#      and onto the real owner's
#  10. after the challenge window, release pays the real owner; the
#      fraudster never receives a cent
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

# One settlement epoch, matching CWEConsumption/CWEEscrow (30 days in seconds).
EPOCH_LENGTH=2592000
# CWEJury's voting window (matches the contract's VOTING_WINDOW constant).
VOTING_WINDOW=$((21 * 24 * 3600))

# --- build the settlement job ----------------------------------------------
step "Building the settlement job"
cargo build --quiet -p cwe-settlement --manifest-path "$ROOT/Cargo.toml"
SETTLE="$ROOT/target/debug/cwe-settlement"

# --- start Anvil (stop only the process we start) --------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
cleanup() { kill -TERM "$ANVIL_PID" 2>/dev/null || true; rm -rf "$WORKDIR"; }
trap cleanup EXIT
# Wait for Anvil to accept RPC; the 0.25s delay bounds the wait by wall-clock.
anvil_ready=0
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && { anvil_ready=1; break; }; sleep 0.25; done
[ "$anvil_ready" = "1" ] || { echo "FAIL: Anvil never became ready"; exit 1; }

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                          # owner + aggregator + verified creator + registrant
U2=${KEYS[1]}                                # the listener who plays the fraudster's copy
FRAUD_PAYEE=$(cast wallet address ${KEYS[3]})  # the fraudster's sole payee
REAL_PAYEE=$(cast wallet address ${KEYS[4]})   # the real owner's sole payee
J1KEY=${KEYS[5]}; J2KEY=${KEYS[6]}; J3KEY=${KEYS[7]}   # three jurors, voting for themselves
J1=$(cast wallet address $J1KEY)
J2=$(cast wallet address $J2KEY)
J3=$(cast wallet address $J3KEY)
U2_ADDR=$(cast wallet address $U2)

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
bal()  { cast balance --rpc-url $RPC "$1"; }
# A `cast call` return, stripped of cast's " [1e18]" pretty annotation (or any
# other trailing annotation) so the bare value can be compared and echoed.
# Works for both numeric (uint256) and raw (bytes32) single-value returns.
callnum() { cast call --rpc-url $RPC "$@" | sed 's/ .*//'; }
# Advance chain time by N seconds and mine a block so the new timestamp takes effect.
warp() { cast rpc --rpc-url $RPC evm_increaseTime "$1" >/dev/null; cast rpc --rpc-url $RPC evm_mine >/dev/null; }

fail=0

# --- step 1: deploy ----------------------------------------------------------
step "1. Deploying contracts"
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
REG=$(jq -r .registry "$DEP"); TIERS=$(jq -r .tiers "$DEP")
CONS=$(jq -r .consumption "$DEP"); PAY=$(jq -r .payouts "$DEP")
ESCROW=$(jq -r .escrow "$DEP"); JURY=$(jq -r .jury "$DEP")
IDENTITY=$(jq -r .identity "$DEP")
echo "registry=$REG payouts=$PAY escrow=$ESCROW jury=$JURY identity=$IDENTITY"

LIGHT=$(cast keccak "light"); FEE=1000000000000000000   # 1 ether tier fee
PRICE=1000000; EU=$(cast format-bytes32-string "EU")
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE
# Make the deployer a trusted issuer, then attest it its own verified-creator
# credential (far-future expiry) — the H6 replacement for the old
# `setVerifiedCreator` allowlist call.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
FAR=18446744073709551615   # type(uint64).max — effectively non-expiring
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR

# --- step 2: appoint the 3-juror committee ----------------------------------
# `setEscrow` was already called by the deploy script; here the deployer
# attests each juror a JUROR credential — the H6 replacement for the old
# `setJuror` allowlist call on CWEJury itself.
step "2. Appointing a 3-juror committee"
JUROR=$(cast keccak "cwe.credential.juror")
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $J1 $JUROR $FAR
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $J2 $JUROR $FAR
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $J3 $JUROR $FAR
echo "  jurors: $J1 / $J2 / $J3"

# --- step 3: the fraudster registers FIRST ----------------------------------
# Both works claim the SAME content; the fraudster registers earlier, which is
# exactly the signal the timestamp-only fallback rewards. If the committee did
# nothing, the fraudster's earlier registration would win by default.
step "3. Fraudster registers FIRST (earliest timestamp wins the old rule)"
CONTENT=$(cast keccak "content-in-dispute")
WORK_FRAUD=$(cast format-bytes32-string "fraudWork")
WORK_REAL=$(cast format-bytes32-string "realWork")

# Build one payee's consent signature over the registry's consentDigest.
consent() {
  local work=$1 content=$2 payee=$3 share=$4 key=$5
  local digest
  digest=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
    "$work" "$content" "$payee" "$share")
  cast wallet sign --private-key "$key" "$digest"
}

SIG_FRAUD=$(consent $WORK_FRAUD $CONTENT $FRAUD_PAYEE 1000000 ${KEYS[3]})
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK_FRAUD $CONTENT "[$FRAUD_PAYEE]" "[1000000]" "[$SIG_FRAUD]" $PRICE $EU
echo "  fraud work registered at t=$(cast call --rpc-url $RPC $REG "registeredAtOf(bytes32)(uint256)" $WORK_FRAUD)"

# --- step 4: the real owner registers SECOND, same content -----------------
step "4. Real owner registers SECOND, claiming the same content"
warp 100
SIG_REAL=$(consent $WORK_REAL $CONTENT $REAL_PAYEE 1000000 ${KEYS[4]})
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK_REAL $CONTENT "[$REAL_PAYEE]" "[1000000]" "[$SIG_REAL]" $PRICE $EU
echo "  real work registered at t=$(cast call --rpc-url $RPC $REG "registeredAtOf(bytes32)(uint256)" $WORK_REAL)"
echo "  (earliest-registration alone would keep the fraudster's claim from here on)"

# --- step 5: a listener plays the fraudster's copy; settle to escrow -------
# The listener's client recognizes the copy only by fingerprint (Tier 2), so
# its credit is escrowed under WORK_FRAUD rather than paid directly.
step "5. Listener subscribes and plays the fraudster's copy; settlement escrows it"
send $U2 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
echo "  payout pool: $(cast to-unit $(bal $PAY) ether) ETH"

# A usage commitment is keccak256(workId || minutes_be32 || plays_be32 || salt)
# — the opening the disclosure reveals below.
commit() { cast keccak $(cast concat-hex "$1" $(cast to-uint256 "$2") $(cast to-uint256 "$3") "$4"); }
SALT=0x$(printf '33%.0s' {1..32})
C=$(commit $WORK_FRAUD 60 1 $SALT)
send $U2 $CONS "submitConsumption(bytes32,bytes32[],bytes)" $LIGHT "[$C]" 0x
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
echo "  epoch = $EPOCH"

# The disclosure marks WORK_FRAUD as fingerprint-matched (escrow_works), so
# settlement routes its credit to CWEEscrow instead of a direct payout.
DISC="$WORKDIR/disclosure.json"
cat > "$DISC" <<JSON
{ "users": {
  "${U2_ADDR,,}": [ { "work_id": "$WORK_FRAUD", "minutes": 60, "plays": 1, "salt": "$SALT" } ]
},
  "escrow_works": [ "$WORK_FRAUD" ]
}
JSON

OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DISCLOSURE=$DISC \
  DEPLOYMENTS=$DEP OUT=$OUT "$SETTLE"

ESC_AMT=$(jq -r --arg w "$WORK_FRAUD" '.escrow[] | select(.work_id==$w) | .amount' "$OUT")
echo "  fraud (escrow) credit = $(cast to-unit ${ESC_AMT:-0} ether) ETH"
[ -n "$ESC_AMT" ] && [ "$ESC_AMT" != "null" ] && [ "$ESC_AMT" != "0" ] || { echo "  FAIL: fraud work was not escrowed"; fail=1; }
ONCHAIN_ESC=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
[ "$ONCHAIN_ESC" = "$ESC_AMT" ] && [ "$ONCHAIN_ESC" != "0" ] || { echo "  FAIL: escrowOf(EPOCH, WORK_FRAUD) = $ONCHAIN_ESC, expected $ESC_AMT > 0"; fail=1; }
echo "  ✓ fraudster's fingerprint-matched credit escrowed on-chain"

# --- step 6: the real owner challenges the escrow ---------------------------
step "6. Real owner challenges the escrow -> opens a jury dispute"
send $DEPLOYER $ESCROW "challenge(uint256,bytes32,bytes32)" $EPOCH $WORK_FRAUD $WORK_REAL
DISPUTE=$(callnum $ESCROW "disputeIdOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
[ "$DISPUTE" != "0" ] || { echo "  FAIL: challenge did not open a dispute"; fail=1; }
echo "  ✓ dispute #$DISPUTE opened"

# --- step 7: the committee votes -- majority for the REAL owner -------------
# Two of three jurors (a real majority, not unanimity) vote for the real
# owner; the third dissents for the fraudster, so the split is genuinely
# contested rather than a formality.
step "7. Committee votes: J1 and J2 for the real owner, J3 for the fraudster"
send $J1KEY $JURY "vote(uint256,bytes32)" $DISPUTE $WORK_REAL
send $J2KEY $JURY "vote(uint256,bytes32)" $DISPUTE $WORK_REAL
send $J3KEY $JURY "vote(uint256,bytes32)" $DISPUTE $WORK_FRAUD
TALLY=$(cast call --rpc-url $RPC $JURY "tallyOf(uint256)(uint256,uint256)" $DISPUTE)
echo "  tally (fraud-incumbent, real-challenger) = $TALLY"

# --- step 8: voting window closes -> finalize tallies the committee's choice
# Without a considered committee vote, the silent/tied fallback would default
# to earliest registration -- the fraudster. Here the 2-1 majority overturns
# that default and the verdict is the REAL owner instead.
step "8. Voting window closes -> finalize tallies the committee's verdict"
warp $((VOTING_WINDOW + 60))
send $DEPLOYER $JURY "finalize(uint256)" $DISPUTE
VERDICT=$(callnum $JURY "verdictOf(uint256)(bytes32)" $DISPUTE)
[ "$VERDICT" = "$WORK_REAL" ] || { echo "  FAIL: verdict $VERDICT != real work $WORK_REAL"; fail=1; }
echo "  ✓ verdict = the real owner's work (the committee's majority, NOT the timestamp default)"

# --- step 9: apply the verdict -- resolveDispute is keyed on the ESCROWED work
step "9. Applying the verdict: resolveDispute reassigns the escrow"
send $DEPLOYER $ESCROW "resolveDispute(uint256,bytes32)" $EPOCH $WORK_FRAUD
ESC_FRAUD_AFTER=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
ESC_REAL_AFTER=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_REAL)
[ "$ESC_FRAUD_AFTER" = "0" ] || { echo "  FAIL: fraud escrow not cleared ($ESC_FRAUD_AFTER)"; fail=1; }
[ "$ESC_REAL_AFTER" = "$ESC_AMT" ] || { echo "  FAIL: escrow not reassigned to real work ($ESC_REAL_AFTER != $ESC_AMT)"; fail=1; }
echo "  ✓ escrow reassigned from the first-registered fraudster to the real owner"

# --- step 10: warp past the release epoch and release -----------------------
step "10. Warp past the release epoch and release"
FRAUD0=$(bal $FRAUD_PAYEE); REAL0=$(bal $REAL_PAYEE)
warp $((2 * EPOCH_LENGTH))
send $DEPLOYER $ESCROW "release(uint256,bytes32)" $EPOCH $WORK_REAL
REAL_GAIN=$(( $(bal $REAL_PAYEE) - REAL0 ))
FRAUD_GAIN=$(( $(bal $FRAUD_PAYEE) - FRAUD0 ))
echo "  real owner gained $(cast to-unit $REAL_GAIN ether) ETH; fraudster gained $(cast to-unit $FRAUD_GAIN ether) ETH"
[ "$REAL_GAIN" = "$ESC_AMT" ] || { echo "  FAIL: real owner gain $REAL_GAIN != escrowed amount $ESC_AMT"; fail=1; }
[ "$FRAUD_GAIN" = "0" ] || { echo "  FAIL: fraudster was paid $FRAUD_GAIN wei"; fail=1; }
echo "  ✓ the real owner was paid the full escrowed amount; the fraudster paid nothing"

echo
if [ "$fail" = "0" ]; then
  echo "✅ ARBITRATION DEMO PASSED — the committee overturned a first-registered fraudster."
else
  echo "FAIL: an assertion did not hold."; exit 1
fi
