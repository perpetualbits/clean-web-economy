#!/usr/bin/env bash
#
# ZK usage-proof end-to-end demo — the H2 capstone.
#
# Drives the REAL zero-knowledge path against a fresh local Anvil node, entirely
# headless. Unlike the legacy disclosure demos (which deploy the accept-all
# verifier and submit empty proofs), this one deploys the real Groth16 verifier
# and settles in EVENT mode (no disclosure file): payouts come straight from the
# per-work weights each submission's proof attests to. Three acts:
#
#   Act 1 (honest):    a genuine proof is generated, submitted, accepted on-chain,
#                      and settled — a creator is paid from the proven weight.
#   Act 2 (inflation): a submission with a tampered digest is REJECTED by the
#                      on-chain Groth16 verifier (you cannot claim weights the
#                      proof does not attest to).
#   Act 3 (row-split): splitting one work across two rows to dodge the per-work
#                      diminishing-returns cap is REFUSED by the prover (the
#                      circuit's per-work uniqueness constraint is unsatisfiable).
#
# Requirements: foundry (anvil/forge/cast), cargo, jq. No Docker — the script
# starts and stops its own Anvil node, killing only the exact PID it started.
set -euo pipefail

# Resolve the repo root from this script's location so the demo is path-independent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RPC="http://127.0.0.1:8545"
WORKDIR="$(mktemp -d)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"

step() { echo; echo "=== $* ==="; }
fail() { echo "❌ $*"; exit 1; }

# --- build the Rust binaries (release: the prover must not run in debug) -----
step "Building the settlement job and zk_submit (release)"
cargo build --release --quiet -p cwe-settlement --manifest-path "$ROOT/Cargo.toml"
SETTLE="$ROOT/target/release/cwe-settlement"
ZK_SUBMIT="$ROOT/target/release/zk_submit"

# --- regenerate the devnet proving key if missing (fresh checkout) ---------
# chain/zk/proving_key.bin is gitignored (17MB+); only verifying_key.bin is
# committed. zk_submit needs the proving key to build a real proof, so on a
# fresh checkout we regenerate BOTH keys via the deterministic devnet setup —
# deterministic means the freshly-derived verifying key matches the one baked
# into the Groth16Verifier we're about to deploy below, byte for byte.
if [ ! -f "$ROOT/chain/zk/proving_key.bin" ]; then
  step "Regenerating devnet proving key (missing)"
  echo "regenerating devnet proving key (missing)... (~85s, deterministic)"
  # export_keys writes to chain/zk/* and chain/test/fixtures/* relative to its
  # OWN cwd (not --manifest-path), so it must be run from the repo root or it
  # would scribble those paths under wherever the script happened to be invoked.
  ( cd "$ROOT" && cargo run --release --quiet -p cwe-zk-circuits --bin export_keys )
fi

# --- start Anvil (stop only the process we start) --------------------------
step "Starting Anvil devnet"
anvil > "$WORKDIR/anvil.log" 2>&1 &
ANVIL_PID=$!
cleanup() { kill -TERM "$ANVIL_PID" 2>/dev/null || true; rm -rf "$WORKDIR"; }
trap cleanup EXIT
for _ in $(seq 1 80); do cast block-number --rpc-url $RPC >/dev/null 2>&1 && break; done

# Anvil's deterministic dev keys/addresses.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                         # owner + aggregator
U1=${KEYS[1]}                               # honest submitter (Act 1)
U2=${KEYS[2]}                               # inflation submitter (Act 2) — fresh, so
                                            # it hits the verifier, not "already submitted"
PAYEE=$(cast wallet address ${KEYS[3]})     # the creator paid for the work

send() { cast send --rpc-url $RPC --private-key "$1" "${@:2}" >/dev/null; }

# --- step 1: deploy with the real Groth16 verifier --------------------------
step "1. Deploying contracts (VERIFIER=groth16)"
( cd "$ROOT/chain" && VERIFIER=groth16 PRIVATE_KEY=$DEPLOYER forge script script/Deploy.s.sol \
    --rpc-url $RPC --broadcast >/dev/null 2>&1 )
DEP="$ROOT/chain/deployments/localhost.json"
CONS=$(jq -r .consumption "$DEP"); BEACON=$(jq -r .beacon "$DEP")
TIERS=$(jq -r .tiers "$DEP"); REG=$(jq -r .registry "$DEP")
IDENTITY=$(jq -r .identity "$DEP"); PAY=$(jq -r .payouts "$DEP")
echo "consumption=$CONS beacon=$BEACON tiers=$TIERS"

# --- step 2: publish the epoch beacon key -----------------------------------
step "2. Publishing the epoch beacon key"
EPOCH=$(cast call --rpc-url $RPC $CONS "currentEpoch()(uint256)")
# A fixed, non-zero 32-byte key for the current epoch (MVP beacon: owner-set).
KEY=0x$(printf '5c%.0s' {1..32})
send $DEPLOYER $BEACON "setKey(uint256,bytes32)" $EPOCH $KEY
echo "epoch=$EPOCH key=$KEY"

# --- step 3: register a work + tier fee + fund a subscription ---------------
step "3. Registering a work, setting a tier fee, funding a subscription"
LIGHT=$(cast keccak "light"); FEE=1000000000000000000    # 1 ether tier fee
PPM=1000000; EU=$(cast format-bytes32-string "EU")
send $DEPLOYER $TIERS "setFee(bytes32,uint256)" $LIGHT $FEE
# H6 verified-creator credential: make the deployer a trusted issuer, then attest
# it its own (non-expiring) verified-creator credential so it may register works.
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
FAR=18446744073709551615                                 # type(uint64).max
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR
# The work zk_submit attributes its proven usage to; the SAME id is passed to the
# submitter via WORK_ID so the proven weight pays this registered creator.
WORK=$(cast format-bytes32-string "zkwork")
CONTENT=$(cast keccak "content-zkwork")
# The payee's EIP-191 consent signature over the registry's consent digest.
DIGEST=$(cast call --rpc-url $RPC $REG "consentDigest(bytes32,bytes32,address,uint96)(bytes32)" \
  "$WORK" "$CONTENT" "$PAYEE" "$PPM")
SIG=$(cast wallet sign --private-key ${KEYS[3]} "$DIGEST")
send $DEPLOYER $REG "registerWork(bytes32,bytes32,address[],uint96[],bytes[],uint256,bytes32)" \
  "$WORK" "$CONTENT" "[$PAYEE]" "[1000000]" "[$SIG]" $PPM $EU
# The honest submitter subscribes, funding the payout pool with the tier fee.
send $U1 $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
echo "work=$WORK payee=$PAYEE pool=$(cast to-unit $(cast balance --rpc-url $RPC $PAY) ether) ETH"

# zk_submit and the settlement job resolve the committed proving/verifying keys
# by a repo-root-relative path (chain/zk/*.bin), so run the acts from the root.
cd "$ROOT"

# =========================================================================
# ACT 1 — honest proof pays a creator
# =========================================================================
step "ACT 1 — honest proof, submit, settle, pay"
RPC_URL=$RPC PRIVATE_KEY=$U1 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK \
  "$ZK_SUBMIT" --mode honest || fail "honest submit did not succeed"

# Settle in EVENT mode (DISCLOSURE unset) and check a creator was paid.
OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DEPLOYMENTS=$DEP OUT=$OUT "$SETTLE" \
  || fail "settlement (event mode) failed"

# Assertions: the registered work got credit, and fees conserve.
N=$(jq '.entries | length' "$OUT")
[ "$N" -ge 1 ] || fail "settlement produced no direct payout entries"
CREDIT=$(jq -r --arg w "${WORK,,}" '.entries[] | select(.work_id == $w) | .amount' "$OUT")
[ -n "$CREDIT" ] && [ "$CREDIT" != "0" ] || fail "registered work $WORK received no credit"
# total_credits + unallocated must equal the fee the submitter paid (conservation).
TOTAL=$(jq -r '.total_credits' "$OUT"); UNALLOC=$(jq -r '.unallocated' "$OUT")
# 1 ETH-scale values fit in signed 64-bit, so bash arithmetic is exact here.
SUM=$((TOTAL + UNALLOC))
[ "$SUM" = "$FEE" ] || fail "fees not conserved: total($TOTAL)+unallocated($UNALLOC) != fee($FEE)"
echo "  work ${WORK} credited $(cast to-unit $CREDIT ether) ETH; fees conserved ($SUM wei)"
echo "ACT 1 OK"

# =========================================================================
# ACT 2 — inflated (tampered-digest) submission is rejected on-chain
# =========================================================================
step "ACT 2 — tampered digest rejected by the on-chain verifier"
# A fresh submitter (U2) so the revert is the verifier's ProofRejected, not the
# one-submission-per-epoch guard. zk_submit exits 0 iff it observed the revert.
RPC_URL=$RPC PRIVATE_KEY=$U2 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK \
  "$ZK_SUBMIT" --mode tamper-digest || fail "tampered submit was NOT rejected"
echo "ACT 2 OK (rejected)"

# =========================================================================
# ACT 3 — row-split (duplicate work id) is refused by the prover
# =========================================================================
step "ACT 3 — row-split refused by the prover/circuit"
# Nothing is submitted; zk_submit exits 0 iff prove/verify refused the split.
RPC_URL=$RPC PRIVATE_KEY=$U2 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK \
  "$ZK_SUBMIT" --mode row-split || fail "row-split was NOT refused"
echo "ACT 3 OK (rejected)"

echo
echo "✅ ZK DEMO PASSED — honest pays, inflation rejected, row-split rejected."
