#!/usr/bin/env bash
#
# Bandwidth-receipt end-to-end demo — the H5 cycle 2 capstone.
#
# Proves that the bandwidth-credibility knob, once hardened against the two
# fraud shapes cycle 2 closed (content-position replay and partial-delivery
# credit) and backed by an on-chain per-work rate, actually behaves end to end
# against a real chain. Real bytes move over HTTP, the storage node and its
# consumer co-sign receipts for them, and the aggregator turns verified
# receipts into a per-(user, work) credibility that decides who gets paid.
# Everything runs in EVENT mode against a fresh local Anvil node, so the usage
# half is backed by real Groth16 proofs, not a disclosure file.
#
# Six users, six works, one act each, settled together in a single epoch:
#
#   Act 1 (honest):       downloads a whole 4 MiB work from a CREDENTIALED
#                         node, co-signs every chunk, and is paid in full.
#   Act 2 (no-download):  asks the node to attest 32 chunks it never fetched.
#                         The node holds no ledger entry for any of them and
#                         refuses every request — zero receipts, zero evidence,
#                         a strict loss.
#   Act 3 (re-fetch):     downloads the SAME chunk (index 0) a hundred times.
#                         Each delivery is genuine and the node signs every one,
#                         but the aggregator's content-position dedup key
#                         collapses all hundred receipts down to one chunk's
#                         worth of evidence — re-fetching cannot amplify credit.
#                         The claim still earns a small, real, non-zero payout
#                         (one chunk against a much larger claimed expectation),
#                         proving the cap has genuine payout consequence rather
#                         than merely leaving a usage claim unattributed.
#   Act 4 (dust weight):  the claimed work is smaller than a single chunk (1
#                         KiB), so even a fully honest download can never clear
#                         the absolute per-epoch evidence floor. The floor guts
#                         the payout — proving the deterrent AND naming, per
#                         spec §3.3, exactly whom it costs: legitimately tiny
#                         works.
#   Act 5 (unset rate):   a fully honest 4 MiB download of a work whose creator
#                         never called `setBandwidthRate`. Real bytes move, but
#                         with no rate to compare them against the row fails
#                         CLOSED, not neutral — a missing rate must never be a
#                         free pass.
#   Act 6 (rogue node):   an honest 4 MiB download, but served by a node with no
#                         storage-node credential. The bytes are as real as
#                         Act 1's; the receipts are rejected outright because
#                         the aggregator only trusts credentialed nodes.
#
# Every act uses its own user AND its own work (Act 6 alone reuses Act 1's
# work, but from a different user against a different, uncredentialed node).
# This is load-bearing, not incidental: the storage node's ledger persists for
# the life of the process, keyed by (consumer, work_id, chunk_index). Two acts
# sharing a user or a work would let a later act inherit an earlier act's
# genuine ledger entries, turning a "proves nothing" result into a false
# positive for a broken trust property.
#
# Requirements: foundry (anvil/forge/cast), cargo, jq, curl. No Docker — the
# script starts and stops its own Anvil and storage nodes, killing only the
# exact PIDs it started.
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

# Anvil's deterministic dev keys fund the deployer and the six subscribers —
# the only parties that ever send a transaction. Storage-node keys and payee
# keys never sign a transaction of their own (a node only co-signs receipts
# off-chain; a payee only signs a consent digest off-chain), so they are fresh
# throwaway keypairs rather than more of Anvil's funded accounts.
mapfile -t KEYS < <(grep -oE '0x[0-9a-f]{64}' "$WORKDIR/anvil.log" | head -10)
DEPLOYER=${KEYS[0]}                          # owner + issuer + aggregator
U1=${KEYS[1]}                                # honest consumer        (Act 1)
U2=${KEYS[2]}                                # never-downloaded claim (Act 2)
U3=${KEYS[3]}                                # re-fetch consumer      (Act 3)
U4=${KEYS[4]}                                # dust-weight consumer   (Act 4)
U5=${KEYS[5]}                                # unset-rate consumer    (Act 5)
U6=${KEYS[6]}                                # rogue-node consumer    (Act 6)

mapfile -t FRESH < <(cast wallet new --json -n 7 | jq -r '.[].private_key')
GOOD_NODE_KEY=${FRESH[0]}                    # credentialed storage node
ROGUE_NODE_KEY=${FRESH[1]}                   # uncredentialed storage node
PAYEE_W_KEY=${FRESH[2]}; PAYEE_G_KEY=${FRESH[3]}; PAYEE_R_KEY=${FRESH[4]}
PAYEE_D_KEY=${FRESH[5]}; PAYEE_N_KEY=${FRESH[6]}
PAYEE_W=$(cast wallet address $PAYEE_W_KEY)  # creator of the honest work
PAYEE_G=$(cast wallet address $PAYEE_G_KEY)  # creator of the no-download work
PAYEE_R=$(cast wallet address $PAYEE_R_KEY)  # creator of the re-fetch work
PAYEE_D=$(cast wallet address $PAYEE_D_KEY)  # creator of the dust-weight work
PAYEE_N=$(cast wallet address $PAYEE_N_KEY)  # creator of the unset-rate work

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
# credential so it may register works (it registers every work below, on
# every creator's behalf, using that creator's consent signature).
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $(cast wallet address $DEPLOYER) $VC $FAR

# The GOOD node gets a storage-node credential; the ROGUE node deliberately does not.
SN=$(cast keccak "cwe.credential.storage-node")
GOOD_NODE_ADDR=$(cast wallet address $GOOD_NODE_KEY)
ROGUE_NODE_ADDR=$(cast wallet address $ROGUE_NODE_KEY)
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $GOOD_NODE_ADDR $SN $FAR
echo "credentialed node=$GOOD_NODE_ADDR   rogue node=$ROGUE_NODE_ADDR (no credential)"

# Register one work per act, each with its own payee's consent signature. The
# deployer is the msg.sender/registrant for all five, which is exactly who
# `setBandwidthRate` (registrant-only) is called as below.
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
WORK_W=$(register_work bwhonest  "$PAYEE_W" "$PAYEE_W_KEY")  # Act 1 + Act 6
WORK_G=$(register_work bwnodl    "$PAYEE_G" "$PAYEE_G_KEY")  # Act 2
WORK_R=$(register_work bwrefetch "$PAYEE_R" "$PAYEE_R_KEY")  # Act 3
WORK_D=$(register_work bwdust    "$PAYEE_D" "$PAYEE_D_KEY")  # Act 4
WORK_N=$(register_work bwnorate  "$PAYEE_N" "$PAYEE_N_KEY")  # Act 5

# All six users subscribe, each funding the pool with one tier fee. A user may
# submit usage only once per epoch, which is exactly why each act needs its
# own user — six independent claimants, not one user claiming six times.
for U in $U1 $U2 $U3 $U4 $U5 $U6; do
  send $U $TIERS "subscribe(bytes32)" $LIGHT --value $FEE
done

# --- step 4: content and bandwidth rates -------------------------------------
step "4. Publishing content and the registry's per-work bandwidth rates"
# 4 MiB (32 chunks of 128 KiB) per work, except Act 4's.
for W in "$WORK_W" "$WORK_G" "$WORK_R" "$WORK_N"; do
  head -c 4194304 /dev/zero | tr '\0' 'x' > "$CONTENT/${W,,}.bin"
done
# Act 4's work is deliberately SMALLER than one chunk, so a claimant whose
# whole epoch is this work cannot reach the absolute evidence floor however
# honestly they download it — the dust-weight deterrent (spec §3.3) working,
# and the accepted cost it imposes on works shorter than a single chunk.
head -c 1024 /dev/zero | tr '\0' 'x' > "$CONTENT/${WORK_D,,}.bin"

# Every work gets a bandwidth rate EXCEPT Act 5's — its absence is the thing
# Act 5 proves. 60000 = MIN_BANDWIDTH_RATE, the contract's clamp floor. The
# demo's proven per-row weight is 45e12, so expectation is 45 * 60000 =
# 2_700_000 bytes: comfortably under the 4 MiB an honest download delivers.
for W in "$WORK_W" "$WORK_G" "$WORK_R" "$WORK_D"; do
  send $DEPLOYER $REG "setBandwidthRate(bytes32,uint256)" "$W" 60000
done
echo "content published; rate=60000 set for W/G/R/D, deliberately UNSET for N"

# --- step 5: start both storage nodes ----------------------------------------
step "5. Starting the credentialed node (8546) and the rogue node (8547)"
CONTENT_DIR=$CONTENT PRIVATE_KEY=$GOOD_NODE_KEY EPOCH=$EPOCH PORT=8546 \
  "$STORAGE" > "$WORKDIR/node-good.log" 2>&1 &
GOOD_NODE_PID=$!
CONTENT_DIR=$CONTENT PRIVATE_KEY=$ROGUE_NODE_KEY EPOCH=$EPOCH PORT=8547 \
  "$STORAGE" > "$WORKDIR/node-rogue.log" 2>&1 &
ROGUE_NODE_PID=$!
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8546/health >/dev/null 2>&1 && break; sleep 0.1; done
for _ in $(seq 1 100); do curl -sf http://127.0.0.1:8547/health >/dev/null 2>&1 && break; sleep 0.1; done

cd "$ROOT"   # zk_submit/settlement resolve chain/zk/*.bin and the deployments file from the repo root

# =========================================================================
# ACT 1 — honest: full download from the credentialed node
# =========================================================================
step "ACT 1 — honest consumer downloads the whole work and submits proven usage"
RPC_URL=$RPC PRIVATE_KEY=$U1 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_W \
  "$ZK_SUBMIT" --mode honest || fail "honest submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_W,,} PRIVATE_KEY=$U1 EPOCH=$EPOCH \
  CHUNKS=32 OUT="$WORKDIR/r1.json" "$CLIENT" --mode honest \
  || fail "honest consumer failed to collect receipts"

# =========================================================================
# ACT 2 — never downloaded: usage claimed, no bytes ever fetched
# =========================================================================
step "ACT 2 — usage claimed with NO chunks ever downloaded"
RPC_URL=$RPC PRIVATE_KEY=$U2 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_G \
  "$ZK_SUBMIT" --mode honest || fail "no-download submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_G,,} PRIVATE_KEY=$U2 EPOCH=$EPOCH \
  CHUNKS=32 OUT="$WORKDIR/r2.json" "$CLIENT" --mode no-download \
  || fail "no-download client did not exit cleanly"

# =========================================================================
# ACT 3 — re-fetch: the SAME chunk downloaded a hundred times
# =========================================================================
step "ACT 3 — the same chunk re-fetched 100 times"
# U3 DOES file a usage claim for WORK_R, same as every other act — an act
# whose payout assertion passes only because no row exists to discount would
# prove "we forgot to submit usage," not "re-fetch amplification is capped."
# With the claim in place the arithmetic is fully deterministic: proven
# weight 45e12, rate 60000 => expected 2_700_000 bytes; 100 re-fetches of
# chunk 0 dedup to ONE chunk's worth of evidence (131_072 bytes, not
# 100 x 131_072); row ratio 131_072 * 1e6 / 2_700_000 ~= 48_545 ppm; U3's
# epoch evidence (131_072 bytes, exactly the floor) saturates the epoch
# factor at neutral, so the row ratio passes through unscaled => credit is
# ~4.85% of the tier fee. That is a real, nonzero, but overwhelmingly
# discounted payout — proving BOTH that dedup caps the evidence at one
# chunk AND that the cap has a genuine payout consequence, not merely an
# absent one.
RPC_URL=$RPC PRIVATE_KEY=$U3 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_R \
  "$ZK_SUBMIT" --mode honest || fail "re-fetch submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_R,,} PRIVATE_KEY=$U3 EPOCH=$EPOCH \
  REPEATS=100 OUT="$WORKDIR/r3.json" "$CLIENT" --mode refetch \
  || fail "re-fetch consumer failed to collect receipts"

# =========================================================================
# ACT 4 — dust weight: a work smaller than one chunk, honestly downloaded whole
# =========================================================================
step "ACT 4 — dust-weight work (1 KiB) downloaded honestly in full"
RPC_URL=$RPC PRIVATE_KEY=$U4 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_D \
  "$ZK_SUBMIT" --mode honest || fail "dust-weight submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_D,,} PRIVATE_KEY=$U4 EPOCH=$EPOCH \
  CHUNKS=1 OUT="$WORKDIR/r4.json" "$CLIENT" --mode honest \
  || fail "dust-weight consumer failed to collect receipts"

# =========================================================================
# ACT 5 — unset rate: a fully honest download of a work with no bandwidth rate
# =========================================================================
step "ACT 5 — honest full download of a work whose bandwidth rate was NEVER SET"
RPC_URL=$RPC PRIVATE_KEY=$U5 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_N \
  "$ZK_SUBMIT" --mode honest || fail "unset-rate submit did not succeed"
STORAGE_URL=http://127.0.0.1:8546 WORK_ID=${WORK_N,,} PRIVATE_KEY=$U5 EPOCH=$EPOCH \
  CHUNKS=32 OUT="$WORKDIR/r5.json" "$CLIENT" --mode honest \
  || fail "unset-rate consumer failed to collect receipts"

# =========================================================================
# ACT 6 — rogue node: real bytes, but from an UNCREDENTIALED node
# =========================================================================
step "ACT 6 — real bytes served by an UNCREDENTIALED node"
RPC_URL=$RPC PRIVATE_KEY=$U6 DEPLOYMENTS=$DEP TIER=$LIGHT WORK_ID=$WORK_W \
  "$ZK_SUBMIT" --mode honest || fail "rogue-node submit did not succeed"
STORAGE_URL=http://127.0.0.1:8547 WORK_ID=${WORK_W,,} PRIVATE_KEY=$U6 EPOCH=$EPOCH \
  CHUNKS=32 OUT="$WORKDIR/r6.json" "$CLIENT" --mode honest \
  || fail "rogue-node consumer failed to collect receipts"

# --- settle once over all six submissions ------------------------------------
step "Settling the epoch with the combined receipt bundle"
# Merge every consumer's bundle into the single bundle the aggregator verifies.
BUNDLE="$WORKDIR/receipts.json"
jq -s '{epoch: .[0].epoch, receipts: (map(.receipts) | add)}' \
  "$WORKDIR/r1.json" "$WORKDIR/r2.json" "$WORKDIR/r3.json" \
  "$WORKDIR/r4.json" "$WORKDIR/r5.json" "$WORKDIR/r6.json" > "$BUNDLE"

OUT="$WORKDIR/proofs.json"
RPC_URL=$RPC PRIVATE_KEY=$DEPLOYER EPOCH=$EPOCH DEPLOYMENTS=$DEP \
  RECEIPTS=$BUNDLE MIN_EPOCH_BYTES=131072 OUT=$OUT "$SETTLE" \
  || fail "settlement (event mode + receipts) failed"
SIDE="${OUT%.json}.bandwidth.json"

# --- assertions ---------------------------------------------------------------
step "Assertions"
credit_of() { jq -r --arg w "$1" '[.entries[] | select(.work_id == $w) | .amount][0] // "0"' "$OUT"; }
# The sidecar's `user` field is lowercased by the aggregator (`normalize_addr`),
# but `cast wallet address` returns an EIP-55 checksummed (mixed-case) address —
# comparing the two as-is would silently never match. Lowercase the address
# argument here, once, rather than at every call site.
bytes_of()  { jq -r --arg u "${1,,}" --arg w "$2" \
  '[.[] | select(.user == $u and .work_id == $w) | .verified_bytes][0] // "0"' "$SIDE"; }

# Act 1 — honest: full fee, and real evidence behind it.
[ "$(credit_of "${WORK_W,,}")" = "$FEE" ] || fail "honest work was not paid in full"
[ "$(bytes_of "$(cast wallet address $U1)" "${WORK_W,,}")" -ge 4000000 ] \
  || fail "honest act did not register its downloaded bytes"

# Act 2 — never downloaded: ZERO evidence (the mechanism), and a burned fee.
B2=$(bytes_of "$(cast wallet address $U2)" "${WORK_G,,}")
[ "$B2" = "0" ] || fail "undownloaded chunks were credited $B2 bytes, expected 0"
[ "$(credit_of "${WORK_G,,}")" = "0" ] || fail "work with no downloads earned credit"

# Act 3 — refetch: 100 receipts for one chunk collapse to a single chunk's
# bytes, and that cap has a real (not merely absent) payout consequence.
B3=$(bytes_of "$(cast wallet address $U3)" "${WORK_R,,}")
# (1) The load-bearing assertion: without dedup this would be 100 x 131072.
[ "$B3" -le 131072 ] || fail "re-fetching amplified evidence to $B3 bytes"
C3=$(credit_of "${WORK_R,,}")
# (2) Strict and nonzero: the row exists and was discounted, not absent —
# an absent row would pass this act for the wrong reason (no usage claim to
# discount at all, rather than dedup capping a real one).
[ "$C3" -gt 0 ] || fail "re-fetch work earned no credit at all; is its usage claim missing?"
# (3) A band, not an exact figure, so the assertion isn't brittle to
# apportionment rounding: more than 90% of the fee must still be burned.
[ "$C3" -lt $((FEE / 10)) ] \
  || fail "re-fetch work earned $C3, expected the chunk cap to burn over 90% of $FEE"

# Act 4 — dust: real but tiny evidence, so the absolute floor guts the payout.
B4=$(bytes_of "$(cast wallet address $U4)" "${WORK_D,,}")
[ "$B4" = "1024" ] || fail "dust act moved $B4 bytes, expected its whole 1 KiB work"
C4=$(credit_of "${WORK_D,,}")
[ "$C4" -lt $((FEE / 1000)) ] \
  || fail "dust claim earned $C4, expected the epoch floor to burn nearly all of $FEE"

# Act 5 — unset rate: fails closed regardless of genuine evidence.
B5=$(bytes_of "$(cast wallet address $U5)" "${WORK_N,,}")
[ "$B5" -ge 4000000 ] || fail "unset-rate act should still have moved real bytes"
[ "$(credit_of "${WORK_N,,}")" = "0" ] || fail "work with no bandwidth rate earned credit"

# Act 6 — uncredentialed node: receipts rejected outright, so no evidence at all.
[ "$(bytes_of "$(cast wallet address $U6)" "${WORK_W,,}")" = "0" ] \
  || fail "rogue node's receipts were counted"

echo "  Act 1 honest:      paid $(cast to-unit "$(credit_of "${WORK_W,,}")" ether) ETH, $(bytes_of "$(cast wallet address $U1)" "${WORK_W,,}") bytes verified"
echo "  Act 2 no-download: paid 0 ETH, 0 bytes verified"
echo "  Act 3 re-fetch:    paid $(cast to-unit "$C3" ether) ETH of $(cast to-unit $FEE ether) ETH, $B3 bytes verified (capped at one chunk)"
echo "  Act 4 dust:        paid $(cast to-unit "$C4" ether) ETH of $(cast to-unit $FEE ether) ETH, $B4 bytes verified"
echo "  Act 5 unset rate:  paid 0 ETH, $B5 bytes verified (fails closed on a missing rate)"
echo "  Act 6 rogue node:  paid 0 ETH, 0 bytes verified (uncredentialed node)"

echo
echo "✅ BANDWIDTH DEMO PASSED — real bytes pay; never-downloaded, re-fetched, dust-weight, unrated and uncredentialed-node claims are all a strict loss."
