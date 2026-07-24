#!/usr/bin/env bash
#
# Ownership & recognition end-to-end demo (H1 exit criterion).
#
# Proves the whole two-tier ownership model against a fresh local Anvil node,
# entirely headless — including a *multi-collaborator* work whose several payees
# each cryptographically consent to their exact share. It exercises both
# recognition tiers and the anti-fraud escrow lifecycle end to end:
#
#   Tier 1 — signed content (authoritative, direct payout):
#     A band records a song. Three collaborators take a cut — a band member
#     (70%), a session musician (20%), and the cover designer (10%). Each signs
#     a consent digest binding them to their share; the registrant assembles the
#     signatures and registers the work with a real content id. A listener plays
#     the *signed* content, and settlement pays the three payees directly, split
#     exactly by their consented shares.
#
#   Tier 2 — unsigned copy (fingerprint match, escrow + dispute + challenge):
#     A fraudster registers a *second* work claiming the SAME content, later than
#     the real owner. A listener plays an unsigned copy; the client recognizes it
#     only by perceptual fingerprint (Tier 2), so its credit is *escrowed*, not
#     paid. The real owner — registered earlier for the same content — challenges
#     the escrow; this OPENS an asynchronous jury dispute rather than deciding
#     instantly. No jurors are appointed in this demo, so once the voting window
#     closes, the jury's finalize falls back to the earliest-registration rule
#     and the escrow reassigns to the real owner. After the challenge window
#     elapses, the escrow releases to the real owner's payees. The fraudster is
#     never paid a cent.
#
# Recognition here is modeled by the settlement disclosure exactly as the
# extension emits it: a work the client matched only by fingerprint is listed in
# the disclosure's `escrow_works`, which routes its credit to CWEEscrow instead
# of a direct payout. Live hub resolution (content id vs fingerprint) is covered
# by `run_hub_demo.sh`; this demo proves the on-chain provenance, consent, and
# escrow lifecycle that resolution feeds into.
#
# Concretely:
#   1. deploy the contracts (registry + payouts + escrow + arbiter)
#   2. register the real owner's song (3 consenting payees) — the EARLIER work
#   3. register the fraudster's claim on the SAME content — the LATER work
#   4. two listeners subscribe (funding the pool); one plays signed, one unsigned
#   5. settle: the signed work pays direct, the fingerprint-matched work escrows
#   6. the three payees withdraw the direct payout — split 70/20/10 exactly
#   7. the real owner challenges the escrow -> opens a jury dispute (no instant
#      reassignment); warp past the voting window and finalize -> the silent
#      committee falls back to earliest-registration -> resolveDispute reassigns
#      the escrow off the fraudster
#   8. warp past the challenge window and release -> the real owner's payees paid
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
DEPLOYER=${KEYS[0]}                        # owner + aggregator + verified creator + registrant
U1=${KEYS[1]}; U2=${KEYS[2]}               # two listeners (signed / unsigned)
BAND=$(cast wallet address ${KEYS[3]})     # band member (70%)
MUSICIAN=$(cast wallet address ${KEYS[4]}) # session musician (20%)
DESIGNER=$(cast wallet address ${KEYS[5]}) # cover designer (10%)
FRAUD=$(cast wallet address ${KEYS[6]})    # fraudster's sole payee (100%)
U1_ADDR=$(cast wallet address $U1)
U2_ADDR=$(cast wallet address $U2)

# Split shares in ppm (must sum to 1_000_000 per work).
S_BAND=700000; S_MUSICIAN=200000; S_DESIGNER=100000

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
bal()  { cast balance --rpc-url $RPC "$1"; }
# A uint256 `cast call` return, stripped of cast's " [1e18]" pretty annotation so
# the bare decimal can be compared and echoed.
callnum() { cast call --rpc-url $RPC "$@" | sed 's/ .*//'; }
# Advance chain time by N seconds and mine a block so the new timestamp takes effect.
warp() { cast rpc --rpc-url $RPC evm_increaseTime "$1" >/dev/null; cast rpc --rpc-url $RPC evm_mine >/dev/null; }

# --- step 1: deploy --------------------------------------------------------
step "1. Deploying contracts"
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
REG=$(jq -r .registry "$DEP"); TIERS=$(jq -r .tiers "$DEP")
CONS=$(jq -r .consumption "$DEP"); PAY=$(jq -r .payouts "$DEP")
ESCROW=$(jq -r .escrow "$DEP"); JURY=$(jq -r .jury "$DEP")
IDENTITY=$(jq -r .identity "$DEP")
echo "registry=$REG payouts=$PAY escrow=$ESCROW jury=$JURY identity=$IDENTITY"

# --- step 2: register the real owner's multi-collaborator song -------------
# The registrant reads each payee's consent digest on-chain and has that payee
# EIP-191 personal-sign it (`cast wallet sign` applies the
# "\x19Ethereum Signed Message:\n32" prefix and hashes), so the registry's
# ecrecover recovers exactly the consenting payee. A work registers only if
# every payee signed their exact share — that is the provenance guarantee.
step "2. Registering the real owner's song (band + musician + designer)"
LIGHT=$(cast keccak "light"); FEE=1000000000000000000   # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE
# Make the deployer a trusted issuer, then attest it its own verified-creator
# credential (far-future expiry) — the H6 replacement for the old
# `setVerifiedCreator` allowlist call.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
FAR=18446744073709551615   # type(uint64).max — effectively non-expiring
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR

# Both works share ONE content id: the real recording and the fraudster's copy
# are the same bytes. The registry keys challenge eligibility on this content id.
CONTENT=$(cast keccak "content-the-song")
WORK_REAL=$(cast format-bytes32-string "realSong")
WORK_FRAUD=$(cast format-bytes32-string "fraudSong")

# Build one payee's consent signature over the registry's consentDigest.
consent() {
  local work=$1 content=$2 payee=$3 share=$4 key=$5
  local digest
  digest=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
    "$work" "$content" "$payee" "$share")
  cast wallet sign --private-key "$key" "$digest"
}

# Each collaborator signs consent to their exact share of the real work.
SIG_BAND=$(consent $WORK_REAL $CONTENT $BAND $S_BAND ${KEYS[3]})
SIG_MUSICIAN=$(consent $WORK_REAL $CONTENT $MUSICIAN $S_MUSICIAN ${KEYS[4]})
SIG_DESIGNER=$(consent $WORK_REAL $CONTENT $DESIGNER $S_DESIGNER ${KEYS[5]})
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK_REAL $CONTENT "[$BAND,$MUSICIAN,$DESIGNER]" "[$S_BAND,$S_MUSICIAN,$S_DESIGNER]" \
  "[$SIG_BAND,$SIG_MUSICIAN,$SIG_DESIGNER]" $PPM $EU
echo "  real song registered at t=$(cast call --rpc-url $RPC $REG "registeredAtOf(bytes32)(uint256)" $WORK_REAL)"

# --- step 3: register the fraudster's later claim on the SAME content ------
# The fraudster registers a competing work over the identical content, but LATER.
# Advance time first so the real owner's registration is strictly earlier — the
# priority signal the earliest-registration arbiter rewards.
step "3. Registering the fraudster's later claim (same content)"
warp 100
SIG_FRAUD=$(consent $WORK_FRAUD $CONTENT $FRAUD 1000000 ${KEYS[6]})
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK_FRAUD $CONTENT "[$FRAUD]" "[1000000]" "[$SIG_FRAUD]" $PPM $EU
echo "  fraud song registered at t=$(cast call --rpc-url $RPC $REG "registeredAtOf(bytes32)(uint256)" $WORK_FRAUD)"

# --- step 4: subscribe + submit usage --------------------------------------
# U1 plays the SIGNED content (recognized Tier 1 -> direct). U2 plays an UNSIGNED
# copy the client recognizes only by fingerprint (Tier 2 -> escrow, marked below
# in the disclosure's escrow_works). Both subscriptions fund the payout pool.
step "4. Two listeners subscribe; one plays signed, one plays an unsigned copy"
send $U1 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
send $U2 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
echo "  payout pool: $(cast to-unit $(bal $PAY) ether) ETH"

# A usage commitment is keccak256(workId || minutes_be32 || plays_be32 || salt)
# — the opening the disclosure reveals below. Each listener plays their work
# for 60 minutes, a single play.
commit() { cast keccak $(cast concat-hex "$1" $(cast to-uint256 "$2") $(cast to-uint256 "$3") "$4"); }
SALT1=0x$(printf '11%.0s' {1..32}); SALT2=0x$(printf '22%.0s' {1..32})
C1=$(commit $WORK_REAL 60 1 $SALT1)     # U1 -> real (signed)
C2=$(commit $WORK_FRAUD 60 1 $SALT2)    # U2 -> fraud (unsigned/fingerprint)
send $U1 $CONS "submitConsumption(bytes32,bytes32[],bytes)" $LIGHT "[$C1]" 0x
send $U2 $CONS "submitConsumption(bytes32,bytes32[],bytes)" $LIGHT "[$C2]" 0x
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
echo "  epoch = $EPOCH"

# The disclosure: each user's openings, plus escrow_works marking the works the
# client recognized only by fingerprint (Tier 2). WORK_FRAUD is fingerprint-
# matched, so its credit is escrowed rather than paid directly.
DISC="$WORKDIR/disclosure.json"
cat > "$DISC" <<JSON
{ "users": {
  "${U1_ADDR,,}": [ { "work_id": "$WORK_REAL", "minutes": 60, "plays": 1, "salt": "$SALT1" } ],
  "${U2_ADDR,,}": [ { "work_id": "$WORK_FRAUD", "minutes": 60, "plays": 1, "salt": "$SALT2" } ]
},
  "escrow_works": [ "$WORK_FRAUD" ]
}
JSON

# --- step 5: settle --------------------------------------------------------
step "5. Settling — signed pays direct, fingerprint-matched escrows"
OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DISCLOSURE=$DISC \
  DEPLOYMENTS=$DEP OUT=$OUT "$SETTLE"

fail=0
# The direct entry for the real work and the escrow entry for the fraud work.
REAL_AMT=$(jq -r --arg w "$WORK_REAL" '.entries[] | select(.work_id==$w) | .amount' "$OUT")
ESC_AMT=$(jq -r --arg w "$WORK_FRAUD" '.escrow[] | select(.work_id==$w) | .amount' "$OUT")
REAL_PROOF=$(jq -r --arg w "$WORK_REAL" '.entries[] | select(.work_id==$w) | .proof | join(",")' "$OUT")
echo "  real (direct) credit = $(cast to-unit ${REAL_AMT:-0} ether) ETH; fraud (escrow) credit = $(cast to-unit ${ESC_AMT:-0} ether) ETH"

# The fingerprint-matched credit must be escrowed, not paid: it appears in the
# escrow bucket, is absent from the direct entries, and CWEEscrow holds it.
[ -n "$ESC_AMT" ] && [ "$ESC_AMT" != "null" ] || { echo "  FAIL: fraud work not escrowed"; fail=1; }
DIRECT_FRAUD=$(jq -r --arg w "$WORK_FRAUD" '.entries[] | select(.work_id==$w) | .amount' "$OUT")
[ -z "$DIRECT_FRAUD" ] || { echo "  FAIL: fraud work paid directly (should be escrowed)"; fail=1; }
ONCHAIN_ESC=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
[ "$ONCHAIN_ESC" = "$ESC_AMT" ] || { echo "  FAIL: on-chain escrow $ONCHAIN_ESC != settled $ESC_AMT"; fail=1; }
echo "  ✓ fingerprint-matched credit escrowed on-chain, not paid"

# --- step 6: the three payees withdraw the direct (signed) payout ----------
# Withdraw splits the credit among the work's payees per their consented shares.
step "6. The real owner's payees withdraw the direct payout (70/20/10)"
# Expected shares: earlier payees floored, the last absorbs the rounding dust —
# exactly how CWEPayouts.withdraw disperses the amount. The credit is a whole
# multiple of the ppm denominator, so dividing first (avoiding 64-bit overflow
# of amount*share in bash) is exact and matches the contract's floored share.
EXP_BAND=$((REAL_AMT / 1000000 * S_BAND))
EXP_MUSICIAN=$((REAL_AMT / 1000000 * S_MUSICIAN))
EXP_DESIGNER=$((REAL_AMT - EXP_BAND - EXP_MUSICIAN))
B0=$(bal $BAND); M0=$(bal $MUSICIAN); D0=$(bal $DESIGNER)
send $DEPLOYER $PAY "withdraw(uint256,bytes32,uint256,bytes32[])" $EPOCH $WORK_REAL $REAL_AMT "[$REAL_PROOF]"
G_BAND=$(( $(bal $BAND) - B0 )); G_MUSICIAN=$(( $(bal $MUSICIAN) - M0 )); G_DESIGNER=$(( $(bal $DESIGNER) - D0 ))
echo "  band $(cast to-unit $G_BAND ether) / musician $(cast to-unit $G_MUSICIAN ether) / designer $(cast to-unit $G_DESIGNER ether) ETH"
[ "$G_BAND" = "$EXP_BAND" ] || { echo "  FAIL: band share $G_BAND != $EXP_BAND"; fail=1; }
[ "$G_MUSICIAN" = "$EXP_MUSICIAN" ] || { echo "  FAIL: musician share $G_MUSICIAN != $EXP_MUSICIAN"; fail=1; }
[ "$G_DESIGNER" = "$EXP_DESIGNER" ] || { echo "  FAIL: designer share $G_DESIGNER != $EXP_DESIGNER"; fail=1; }
echo "  ✓ direct payout split by consented shares"

# --- step 7: the real owner challenges the fraudster's escrow --------------
# `challenge` no longer decides on the spot -- it OPENS an asynchronous jury
# dispute and leaves the escrow exactly where it was until `resolveDispute`
# applies the finalized verdict. Confirm that: a nonzero dispute id exists, and
# neither work's escrowed balance has moved yet.
step "7. Real owner challenges the escrow -> opens a jury dispute"
send $DEPLOYER $ESCROW "challenge(uint256,bytes32,bytes32)" $EPOCH $WORK_FRAUD $WORK_REAL
DISPUTE=$(callnum $ESCROW "disputeIdOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
[ "$DISPUTE" != "0" ] || { echo "  FAIL: challenge did not open a dispute"; fail=1; }
ESC_FRAUD_MID=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
ESC_REAL_MID=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_REAL)
[ "$ESC_FRAUD_MID" = "$ESC_AMT" ] || { echo "  FAIL: fraud escrow moved before resolution ($ESC_FRAUD_MID != $ESC_AMT)"; fail=1; }
[ "$ESC_REAL_MID" = "0" ] || { echo "  FAIL: real escrow funded before resolution ($ESC_REAL_MID != 0)"; fail=1; }
echo "  ✓ dispute #$DISPUTE opened; no instant reassignment"

# No jurors are appointed in this demo, so once the voting window closes,
# `finalize` falls back to the earliest-registration rule (real owner wins).
# `resolveDispute` then applies that verdict to the escrow.
step "7b. Voting window closes -> jury falls back to earliest registration"
warp $((21 * 24 * 3600 + 60))
send $DEPLOYER $JURY "finalize(uint256)" $DISPUTE
send $DEPLOYER $ESCROW "resolveDispute(uint256,bytes32)" $EPOCH $WORK_FRAUD
ESC_FRAUD_AFTER=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_FRAUD)
ESC_REAL_AFTER=$(callnum $ESCROW "escrowOf(uint256,bytes32)(uint256)" $EPOCH $WORK_REAL)
[ "$ESC_FRAUD_AFTER" = "0" ] || { echo "  FAIL: fraud escrow not cleared ($ESC_FRAUD_AFTER)"; fail=1; }
[ "$ESC_REAL_AFTER" = "$ESC_AMT" ] || { echo "  FAIL: escrow not reassigned to real work ($ESC_REAL_AFTER != $ESC_AMT)"; fail=1; }
echo "  ✓ escrow reassigned from fraud -> real work"

# --- step 8: warp past the window and release ------------------------------
# The challenge window is one epoch; warp two more to clear it unconditionally
# (stacked on top of the 21-day voting warp above, `currentEpoch()` is already
# well past the release epoch, so this just keeps the same safety margin), then
# release. The reassigned escrow pays the real owner's payees per their shares.
step "8. Warp past the challenge window and release"
FRAUD0=$(bal $FRAUD)
warp $((2 * EPOCH_LENGTH))
B1=$(bal $BAND); M1=$(bal $MUSICIAN); D1=$(bal $DESIGNER)
send $DEPLOYER $ESCROW "release(uint256,bytes32)" $EPOCH $WORK_REAL
# The reassigned escrow (ESC_AMT) splits by the SAME consented shares (divide
# first, as above, to stay within bash's 64-bit arithmetic).
E_BAND=$((ESC_AMT / 1000000 * S_BAND))
E_MUSICIAN=$((ESC_AMT / 1000000 * S_MUSICIAN))
E_DESIGNER=$((ESC_AMT - E_BAND - E_MUSICIAN))
R_BAND=$(( $(bal $BAND) - B1 )); R_MUSICIAN=$(( $(bal $MUSICIAN) - M1 )); R_DESIGNER=$(( $(bal $DESIGNER) - D1 ))
echo "  released band $(cast to-unit $R_BAND ether) / musician $(cast to-unit $R_MUSICIAN ether) / designer $(cast to-unit $R_DESIGNER ether) ETH"
[ "$R_BAND" = "$E_BAND" ] || { echo "  FAIL: released band share $R_BAND != $E_BAND"; fail=1; }
[ "$R_MUSICIAN" = "$E_MUSICIAN" ] || { echo "  FAIL: released musician share $R_MUSICIAN != $E_MUSICIAN"; fail=1; }
[ "$R_DESIGNER" = "$E_DESIGNER" ] || { echo "  FAIL: released designer share $R_DESIGNER != $E_DESIGNER"; fail=1; }
# The fraudster must never have been paid.
FRAUD_GAIN=$(( $(bal $FRAUD) - FRAUD0 ))
[ "$FRAUD_GAIN" = "0" ] || { echo "  FAIL: fraudster was paid $FRAUD_GAIN wei"; fail=1; }
echo "  ✓ escrow released to the real owner; fraudster paid nothing"

echo
if [ "$fail" = "0" ]; then
  echo "✅ OWNERSHIP DEMO PASSED — consented split payout, and a fingerprint-matched"
  echo "   escrow challenged by the earlier real owner and released to them, not the fraudster."
else
  echo "❌ OWNERSHIP DEMO FAILED — an assertion did not hold."; exit 1
fi
