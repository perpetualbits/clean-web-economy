#!/usr/bin/env bash
#
# Verifiable-credential lifecycle end-to-end demo (H6 exit criterion).
#
# Proves what the old per-contract owner allowlists (`setVerifiedCreator`,
# `setJuror`) could not: a credential that is portable across contracts,
# revocable by its issuer, expires on its own, and is invalidated the moment
# its issuer is no longer trusted. Everything here runs against `CWEIdentity`
# and `CWERegistry`, entirely headless, against a fresh local Anvil node.
#
# Concretely:
#   1. deploy the contracts; make the deployer a trusted issuer
#   2. attest a verified-creator credential to CREATOR -> CREATOR registers a
#      work -> succeeds
#   3. an address that was never attested anything tries to register -> reverts
#   4. revoke CREATOR's credential -> a further registration attempt reverts
#   5. re-attest with a short expiry, warp past it -> isValid flips false and
#      registration reverts
#   6. re-attest afresh (valid again, registration succeeds); then remove the
#      deployer as a trusted issuer -> isValid flips false even though the
#      credential itself was never touched (issuer-set revocation)
#   7. attest a juror credential and revoke it, checking isValid's transitions
#      directly -- the on-chain `vote` gating over these same credentials is
#      exercised end to end by `CWEJuryTest` in the Foundry suite, so this demo
#      does not duplicate a full dispute here
#
# Requirements: foundry (anvil/forge/cast), jq. No Docker needed -- the script
# starts and stops its own Anvil node.
set -euo pipefail

# Resolve the repo root from this script's location so the demo is path-independent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RPC="http://127.0.0.1:8545"
WORKDIR="$(mktemp -d)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"

step() { echo; echo "=== $* ==="; }

# --- start Anvil (stop only the process we start) --------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
# Kill only the PID this script itself launched (never by name/pattern), and
# always clean up the scratch workdir, whether the demo passes or fails.
cleanup() { kill -TERM "$ANVIL_PID" 2>/dev/null || true; rm -rf "$WORKDIR"; }
trap cleanup EXIT
# Wait for Anvil to accept RPC; the 0.25s delay bounds the wait by wall-clock.
anvil_ready=0
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && { anvil_ready=1; break; }; sleep 0.25; done
[ "$anvil_ready" = "1" ] || { echo "FAIL: Anvil never became ready"; exit 1; }

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                       # owner + issuer
CREATOR_KEY=${KEYS[1]}                    # gets the verified-creator credential
OUTSIDER_KEY=${KEYS[2]}                   # never attested anything
JUROR_ADDR=$(cast wallet address ${KEYS[3]}) # gets a juror credential in step 7
CREATOR=$(cast wallet address $CREATOR_KEY)
OUTSIDER=$(cast wallet address $OUTSIDER_KEY)
DEPLOYER_ADDR=$(cast wallet address $DEPLOYER)

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }
# A `cast call` return, stripped of cast's " [1e18]" pretty annotation (or any
# other trailing annotation) so bool/uint/bytes32 single-value returns compare
# and echo as bare values.
callnum() { cast call --rpc-url $RPC "$@" | sed 's/ .*//'; }
# Advance chain time by N seconds and mine a block so the new timestamp takes effect.
warp() { cast rpc --rpc-url $RPC evm_increaseTime "$1" >/dev/null; cast rpc --rpc-url $RPC evm_mine >/dev/null; }
# Build one payee's EIP-191 consent signature over the registry's
# consentDigest -- copied from the sibling demos. Every registerWork call
# below has the registering creator sign consent for their own payout share.
consent() {
  local work=$1 content=$2 payee=$3 share=$4 key=$5
  local digest
  digest=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
    "$work" "$content" "$payee" "$share")
  cast wallet sign --private-key "$key" "$digest"
}
# True (0) iff a registerWork call from `key` for `work` reverts -- used to
# assert the credential gate actually blocks an unauthorised registration,
# without letting `set -e` abort the whole script on the expected failure.
register_reverts() {
  local key=$1 work=$2 payee=$3 sig
  sig=$(consent "$work" "$CONTENT" "$payee" "$PPM" "$key")
  ! cast send --rpc-url $RPC --private-key "$key" $REG \
    "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
    "$work" "$CONTENT" "[$payee]" "[$PPM]" "[$sig]" "$PRICE" "$EU" >/dev/null 2>&1
}

fail=0
PPM=1000000; PRICE=1000000; EU=$(cast format-bytes32-string "EU")
CONTENT=$(cast keccak "content-identity-demo")

# --- step 1: deploy; make the deployer a trusted issuer ---------------------
step "1. Deploying contracts; making the deployer a trusted issuer"
( cd "$ROOT/chain" && PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
REG=$(jq -r .registry "$DEP"); IDENTITY=$(jq -r .identity "$DEP")
echo "registry=$REG identity=$IDENTITY"
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $DEPLOYER_ADDR true

VC=$(cast keccak "cwe.credential.verified-creator")
JUROR=$(cast keccak "cwe.credential.juror")
FAR=18446744073709551615   # type(uint64).max -- effectively non-expiring

# --- step 2: attest -> a credentialed creator registers a work -------------
step "2. Attesting a verified-creator credential; CREATOR registers a work"
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $CREATOR $VC $FAR
WORK1=$(cast format-bytes32-string "identityWork1")
SIG1=$(consent $WORK1 $CONTENT $CREATOR $PPM $CREATOR_KEY)
send $CREATOR_KEY $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK1 $CONTENT "[$CREATOR]" "[$PPM]" "[$SIG1]" $PRICE $EU
REGISTERED1=$(callnum $REG "isRegistered(bytes32)(bool)" $WORK1)
[ "$REGISTERED1" = "true" ] || { echo "  FAIL: credentialed creator's work did not register"; fail=1; }
echo "  ✓ credentialed creator registered a work"

# --- step 3: a non-credentialed address is rejected -------------------------
step "3. A non-credentialed address tries to register -> reverts"
WORK_OUTSIDER=$(cast format-bytes32-string "identityWorkOutsider")
register_reverts $OUTSIDER_KEY $WORK_OUTSIDER $OUTSIDER \
  || { echo "  FAIL: non-credentialed registration unexpectedly succeeded"; fail=1; }
OUTSIDER_REGISTERED=$(callnum $REG "isRegistered(bytes32)(bool)" $WORK_OUTSIDER)
[ "$OUTSIDER_REGISTERED" = "false" ] || { echo "  FAIL: outsider's work is registered"; fail=1; }
echo "  ✓ non-credentialed registration reverted; nothing was recorded"

# --- step 4: revoke -> a further registration attempt reverts --------------
step "4. Revoking CREATOR's credential -> a further registration reverts"
send $DEPLOYER $IDENTITY "revoke(address,bytes32)" $CREATOR $VC
VALID_AFTER_REVOKE=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $CREATOR $VC)
[ "$VALID_AFTER_REVOKE" = "false" ] || { echo "  FAIL: isValid still true after revoke"; fail=1; }
WORK2=$(cast format-bytes32-string "identityWork2")
register_reverts $CREATOR_KEY $WORK2 $CREATOR \
  || { echo "  FAIL: registration after revoke unexpectedly succeeded"; fail=1; }
WORK2_REGISTERED=$(callnum $REG "isRegistered(bytes32)(bool)" $WORK2)
[ "$WORK2_REGISTERED" = "false" ] || { echo "  FAIL: work registered after credential revoke"; fail=1; }
echo "  ✓ revoked credential blocks registration"

# --- step 5: short expiry + warp -> isValid flips false ---------------------
step "5. Re-attesting with a short expiry; warping past it -> isValid flips false"
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $CREATOR $VC \
  $(( $(cast block latest --rpc-url $RPC --field timestamp) + 100 ))
VALID_BEFORE_EXPIRY=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $CREATOR $VC)
[ "$VALID_BEFORE_EXPIRY" = "true" ] || { echo "  FAIL: freshly re-attested credential is not valid"; fail=1; }
warp 200
VALID_AFTER_EXPIRY=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $CREATOR $VC)
[ "$VALID_AFTER_EXPIRY" = "false" ] || { echo "  FAIL: isValid still true after expiry"; fail=1; }
WORK3=$(cast format-bytes32-string "identityWork3")
register_reverts $CREATOR_KEY $WORK3 $CREATOR \
  || { echo "  FAIL: registration after expiry unexpectedly succeeded"; fail=1; }
echo "  ✓ expiry flips isValid false and blocks registration"

# --- step 6: re-attest (valid again); issuer removal flips isValid false ----
step "6. Re-attesting (valid again); then removing the issuer -> isValid flips false"
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $CREATOR $VC $FAR
VALID_RENEWED=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $CREATOR $VC)
[ "$VALID_RENEWED" = "true" ] || { echo "  FAIL: renewed credential is not valid"; fail=1; }
WORK4=$(cast format-bytes32-string "identityWork4")
SIG4=$(consent $WORK4 $CONTENT $CREATOR $PPM $CREATOR_KEY)
send $CREATOR_KEY $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  $WORK4 $CONTENT "[$CREATOR]" "[$PPM]" "[$SIG4]" $PRICE $EU
REGISTERED4=$(callnum $REG "isRegistered(bytes32)(bool)" $WORK4)
[ "$REGISTERED4" = "true" ] || { echo "  FAIL: registration after renewal did not succeed"; fail=1; }
echo "  ✓ renewed credential is valid again and registration succeeds"

# The credential itself is untouched -- only the issuer that granted it is no
# longer trusted. `isValid` must still flip false: it checks the issuer is
# CURRENTLY trusted, not merely that it was trusted at attestation time.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $DEPLOYER_ADDR false
VALID_AFTER_ISSUER_REMOVED=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $CREATOR $VC)
[ "$VALID_AFTER_ISSUER_REMOVED" = "false" ] || { echo "  FAIL: isValid still true after issuer removal"; fail=1; }
WORK5=$(cast format-bytes32-string "identityWork5")
register_reverts $CREATOR_KEY $WORK5 $CREATOR \
  || { echo "  FAIL: registration with an untrusted issuer's credential unexpectedly succeeded"; fail=1; }
echo "  ✓ removing the issuer invalidates every credential it granted, without touching the credential"

# Restore the issuer so step 7's juror attestation is itself valid.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $DEPLOYER_ADDR true

# --- step 7: juror credential lifecycle -------------------------------------
# A full dispute (open -> vote -> finalize) exercises exactly this same
# isValid gate inside CWEJury.vote -- that on-chain path is covered end to end
# by CWEJuryTest. Here the demo confirms the credential half of that gate
# directly: attest flips a juror valid, revoke flips it back.
step "7. Juror credential: attest -> valid, revoke -> invalid"
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $JUROR_ADDR $JUROR $FAR
JUROR_VALID=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $JUROR_ADDR $JUROR)
[ "$JUROR_VALID" = "true" ] || { echo "  FAIL: attested juror credential is not valid"; fail=1; }
send $DEPLOYER $IDENTITY "revoke(address,bytes32)" $JUROR_ADDR $JUROR
JUROR_REVOKED=$(callnum $IDENTITY "isValid(address,bytes32)(bool)" $JUROR_ADDR $JUROR)
[ "$JUROR_REVOKED" = "false" ] || { echo "  FAIL: revoked juror credential is still valid"; fail=1; }
echo "  ✓ juror credential attest/revoke transitions isValid correctly"
echo "  (CWEJuryTest exercises the same gate inside CWEJury.vote on-chain)"

echo
if [ "$fail" = "0" ]; then
  echo "✅ IDENTITY DEMO PASSED"
else
  echo "FAIL: an assertion did not hold."; exit 1
fi
