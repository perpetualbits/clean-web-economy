# H2 — ZK usage proofs (cycle 1): design

**Date:** 2026-07-24
**Status:** Design (approved in brainstorming; pending spec review)
**Governing spec:** `docs/specs/zk_usage_proof_requirements.md`
**Roadmap item:** H2 (hardening track) — *real circuits behind the `ZK`/`IProofVerifier`
seam, replacing the disclosure file.*

---

## 1. Purpose and scope

Today usage accounting is honest-but-stubbed. A client posts opaque keccak
commitments on-chain, an `AcceptAllVerifier` waves the proof through, and the
settlement service learns what the commitments mean from a **trusted disclosure
file** — the plaintext `(work_id, minutes, plays)` for every user. That leaves two
holes this cycle closes:

1. **Surveillance.** The aggregator sees the full plaintext table of who consumed
   what — the global consumption log the project exists to avoid.
2. **Honesty.** A modified client can commit to works it never accessed, or inflate
   minutes; the keccak hash only binds the user to a number *they chose*.

This cycle delivers the **smallest honest end-to-end slice plus within-epoch
double-count defense**: a real Groth16 proof, verified on-chain by a real verifier
that replaces `AcceptAllVerifier`, that forces usage to be well-formed,
range-bounded, diminishing-returns-capped, epoch-bound, and per-work unique — while
the raw per-user usage stream never leaves the device and the disclosure file is
deleted.

### 1.1 In scope

- A pure-Rust Groth16 circuit (arkworks, BN254) proving the seven constraints in §4.
- A real on-chain `Groth16Verifier` implementing `IProofVerifier`, replacing
  `AcceptAllVerifier`.
- Commitment scheme migration keccak → **Poseidon** (ZK-friendly), behind the
  existing `Opening::commit()` API.
- A minimal `CWEEpochBeacon` publishing a per-epoch key `K_epoch`, and pseudonyms
  `P_w = Poseidon(K_epoch, C_w)`.
- Settlement reworked to read *proven* public outputs from events and pay from them;
  `disclosure.rs` deleted; `escrow_works` routing derived from registry recognition
  data instead.
- A deterministic, honestly-labeled devnet trusted-setup key.
- `make zk-demo` + `zk-e2e` CI job demonstrating the guarantee biting.

### 1.2 Explicitly deferred (each a named future cycle)

- **Work-identity blinding** — hiding *which* works a user consumed (strong-privacy
  target; fights the per-user payout math, needs homomorphic aggregation / MPC).
- **Manifest-signature circuit** — proving each `work_id` is creator-signed *in* ZK
  (this cycle reveals and trusts the public `work_id`).
- **Tier-eligibility circuit** — proving possession of tier `T` (this cycle binds `T`
  as a stated public input).
- **Cross-epoch unlinkability** and a **real randomness beacon** (VRF/drand); the MVP
  beacon publishes a fixed per-epoch value.
- **Full DAPR in-circuit** — bandwidth-credibility and reputation stay aggregator-side
  (they are not derived from private usage).

### 1.3 Decisions locked in brainstorming

| # | Decision | Choice |
|---|---|---|
| D1 | Cycle ambition | Smallest honest slice **+** within-epoch double-count defense |
| D2 | Toolchain | arkworks **Groth16 / BN254** (pure Rust, EVM-native verifier) |
| D3 | Privacy boundary | **Integrity-first**: aggregator sees capped per-work weight + `work_id`; raw usage private. Work-blinding deferred |
| D4 | In-circuit math | Only the **diminishing-returns cap** is in-circuit; bandwidth × reputation applied by the aggregator afterward |
| D5 | Verification location | **On-chain** at submit time (the point of the `IProofVerifier` seam) |

---

## 2. Architecture

### 2.1 The digest trick (why on-chain verification stays cheap)

A Groth16 verifier on the EVM is cheap only with a small, fixed number of public
inputs. Our proof must expose a variable-length list (pseudonyms + per-work capped
weights), which would blow up gas. So the circuit squeezes its entire public output
into **one** Poseidon hash `D`:

```
D = Poseidon(E, T, K_epoch, {P_w}, {(work_id_w, weight_w)})
```

- **On-chain** the `Groth16Verifier` checks the ~200-byte proof against the single
  public input `D` (O(1), ~250k gas via BN254 precompiles). It guarantees *"a valid
  proof exists for digest D."*
- **Off-chain** the settlement service recomputes `Poseidon(...) == D` in Rust and
  only then trusts the plaintext list — safe because `D` is a *proven* output, so the
  client cannot have lied about the numbers.

No trusted disclosure file is needed at any point.

### 2.2 End-to-end flow

```
CLIENT (device)                         CHAIN                       SETTLEMENT (aggregator)
─────────────────                       ─────                       ───────────────────────
private: raw events, minutes,
  plays, salts, per-work rows
        │
  Poseidon commitments C_w
  pseudonyms P_w = Poseidon(K_epoch, C_w)
  weight_w = DR_cap(minutes,plays,T)   (in-circuit)
  digest D
        │
  Groth16 prove  ───────────►  submitConsumption(
                                 tier, commitments,
                                 pseudonyms, workIds,
                                 weights, D, proof)
                                     │
                               Groth16Verifier.verify(D, proof)  ← replaces AcceptAllVerifier
                                     │  (revert ProofRejected on fail)
                               emit ConsumptionSubmitted ─────────►  read events
                                 (…, pseudonyms, workIds,            recompute Poseidon == D
                                  weights, D)                        dedupe pseudonyms
                                                                     apply bandwidth × reputation
                                                                     aggregate per work → pay
```

### 2.3 Components touched / added

| Component | Change |
|---|---|
| `libs/zk-circuits` (new crate) | The arkworks Groth16 circuit, native Poseidon, prover, verifier-key export, deterministic devnet setup |
| `libs/wallet-zk` (`commit.rs`, `zk.rs`) | Commitment keccak → Poseidon behind `Opening::commit()`; `zk.rs` `none-v0` placeholder replaced by real prover call |
| `chain/contracts/Groth16Verifier.sol` (new) | Real `IProofVerifier` impl; verifying key baked in |
| `chain/contracts/AcceptAllVerifier.sol` | Dropped from the deploy path; retained under `chain/test/` as a permissive stub only if a unit test needs one, else deleted |
| `chain/contracts/CWEEpochBeacon.sol` (new) | Publishes per-epoch `K_epoch` |
| `chain/contracts/CWEConsumption.sol` | `submitConsumption` carries pseudonyms, workIds, weights, `D`; verifies against `D`; event extended |
| `services/settlement` | `disclosure.rs` deleted; reads proven outputs from events; `escrow_works` from registry recognition; bandwidth × reputation applied to capped weights |
| `clients/browser-ext`, `clients/player-plugin` | Poseidon commitment swap; produce/submit the real proof bundle |
| `ops/` | `make zk-demo`; `zk-e2e` CI job |

---

## 3. Commitments, pseudonyms, and the epoch beacon

### 3.1 Commitment: Poseidon

```
C_w = Poseidon(work_id, minutes, plays, salt)
```

Replaces `keccak256(work_id ‖ minutes ‖ plays ‖ salt)`. Rationale: keccak is very
expensive as an in-circuit gadget; Poseidon is designed for it. The on-chain type is
unchanged (`bytes32`, a BN254 field element). **Nothing hashes Poseidon on-chain** —
the chain only stores/emits commitments and verifies the Groth16 proof — so no
Solidity Poseidon is required. Poseidon lives only in Rust: one implementation used
both natively (client) and as the arkworks gadget (circuit), with a test asserting
the two agree.

The `Opening`/`Commitment` API in `wallet-zk` is preserved; only the hash inside
`Opening::commit()` changes. `plays` remains bound (H1 property).

### 3.2 Pseudonyms and epoch binding

```
P_w = Poseidon(K_epoch, C_w)
```

`K_epoch` is published per epoch by a new minimal `CWEEpochBeacon`. For this cycle the
beacon publishes a **fixed per-epoch value** (owner-set or deterministically derived);
this suffices for anti-replay — a commitment from a prior epoch produces a different
`P_w` and cannot be resubmitted as this epoch's — and seats the pseudonym layer for
the future blinding cycle. A real unpredictable beacon (VRF/drand,
`epoch_beacon_specification.md`) is deferred and the MVP beacon is labeled as such.

### 3.3 Fixed circuit arity

R1CS circuits are fixed-size. A proof covers up to `MAX_WORKS` works, padded with
inert rows (a padded row must satisfy the constraints trivially and contribute zero
weight and a distinct-but-inert pseudonym slot). `MAX_WORKS = 16` for the MVP; a user
with more works submits multiple proofs. The padding scheme is part of the circuit
design and must be tested (a padded row cannot smuggle weight).

---

## 4. The circuit

### 4.1 Private witness (per work `w`, `w = 1..MAX_WORKS`)

`work_id_w`, `minutes_w`, `plays_w`, `salt_w`. Padded rows carry a sentinel that the
constraints treat as zero-weight.

### 4.2 Public inputs

Individually: `E` (epoch), `T` (tier), `K_epoch`, and per work `C_w`, `P_w`,
`work_id_w`, `weight_w`. **All are folded into the single public input `D`** (§2.1);
the plaintext values travel in calldata/event for the aggregator, not as separate
on-chain public inputs.

### 4.3 Constraints (each must fail closed when its witness is corrupted)

1. **Commitment correctness** — `C_w = Poseidon(work_id_w, minutes_w, plays_w, salt_w)`.
2. **Well-formedness & range** — `0 ≤ minutes_w ≤ MAX_MINUTES`,
   `0 ≤ plays_w ≤ MAX_PLAYS`, `work_id_w ≠ 0` (via bit-decomposition range checks).
3. **Diminishing-returns cap** — `weight_w = DR(minutes_w, plays_w, T)`, the concave
   per-user-per-work cap from H3, computed in-circuit as fixed-point integer math.
   `weight_w` is the only usage figure exposed.
4. **Tier binding** — `T` is the tier the weights were computed under. *(Membership
   proof deferred.)*
5. **Epoch binding** — `P_w = Poseidon(K_epoch, C_w)`.
6. **Uniqueness** — the `work_id_w` (equivalently `P_w`) are pairwise distinct: one
   row per work. This is what makes the DR cap un-bypassable by row-splitting.
7. **Digest binding** — `D = Poseidon(E, T, K_epoch, {P_w}, {(work_id_w, weight_w)})`,
   the single public input the verifier checks.

### 4.4 The DR cap in-circuit

H3's DAPR is `DR(raw usage) × bandwidth-credibility × reputation`. Only `DR(raw usage)`
depends on private numbers, so only it is in-circuit. It is reproduced from
`sims/cwe-dapr` as deterministic fixed-point integer math with explicit range checks
(no field-overflow, no division-by-zero). Bandwidth-credibility and reputation are
aggregator-known signals applied to `weight_w` after the fact (§5). The in-circuit DR
math must match the sim's DR component exactly, verified by a cross-test.

---

## 5. Settlement changes

- **`disclosure.rs` deleted.** Settlement reads the extended `ConsumptionSubmitted`
  events, recomputes `Poseidon(...) == D` per submission (reject on mismatch), and
  dedupes pseudonyms across submissions within the epoch.
- **Payout.** For each `(user, work)` it takes the proven `weight_w`, multiplies by the
  aggregator-side bandwidth-credibility and reputation factors, aggregates per work,
  and produces the same committable Merkle root / withdrawal proofs as today.
- **`escrow_works` rehomed.** The Tier-1 vs Tier-2 (signed vs fingerprint) routing that
  the disclosure file used to carry is a property of the *content* (does a signed
  registration exist?), so settlement derives it from the **registry / hub recognition
  data it already reads**, not from any user input. H1's escrow behavior is preserved.

---

## 6. On-chain changes

- **`Groth16Verifier.sol`** — implements the (updated) `IProofVerifier`, verifying the
  ~200-byte proof against the single public input `D`. The verifying key (from the
  devnet setup) is baked in; uses BN254 precompiles (`0x06/0x07/0x08`).
  *Interface change:* `D` **cannot** be recomputed on-chain (that would need Solidity
  Poseidon, which we deliberately avoid), so it is passed in. `IProofVerifier.verify`
  is updated from `(bytes32[] commitments, bytes proof)` to `verify(bytes32 digest,
  bytes proof)`; `CWEConsumption` supplies `D`. On-chain therefore checks only
  *proof-vs-`D`* — the plaintext pseudonyms/workIds/weights ride in the event and are
  bound to `D` only later, by settlement's off-chain `Poseidon(...) == D` check (§2.1).
  The change is confined to this seam and the one caller.
- **`CWEEpochBeacon.sol`** — `keyFor(uint256 epoch) → bytes32`, plus an owner setter for
  the MVP. Honestly labeled as a fixed (non-random) devnet beacon.
- **`CWEConsumption.sol`** — `submitConsumption` extended to accept pseudonyms,
  `workIds`, `weights`, and `D`; runs the verifier against `D`; the
  `ConsumptionSubmitted` event carries the proven public outputs. One-submission-per-
  user-per-epoch and the existing effects-before-log ordering are preserved.
- **`AcceptAllVerifier.sol`** — dropped from the deploy path.

---

## 7. Trusted setup (honesty)

Groth16 requires a per-circuit proving/verifying key from a trusted setup. This cycle
ships a **deterministic, in-repo devnet key, loudly labeled insecure / devnet-only** —
the same ethic as `AcceptAllVerifier` and the `none-v0` tag. Generation is a
reproducible command (fixed seed) so CI and the demo regenerate byte-identical keys. A
real multi-party ceremony is a production deferral, documented at the seam.

---

## 8. Demo and tests

### 8.1 `make zk-demo` (CI job `zk-e2e`) — three acts on Anvil

1. **Honest client** — real proof; on-chain verifier accepts; settlement pays creators
   correctly from proofs alone (no disclosure file).
2. **Inflating client** — tampers weight / raw minutes so they no longer match the
   commitment → **on-chain verifier rejects** (`ProofRejected`); no payment.
3. **Row-splitter** — lists one work as several under-cap rows to dodge the DR cap →
   **uniqueness constraint rejects**.

### 8.2 Unit / integration tests

- Circuit: each constraint fails closed under a corrupted witness; padded rows cannot
  smuggle weight.
- Native-vs-in-circuit Poseidon agreement.
- In-circuit DR cap vs `sims/cwe-dapr` DR component agreement.
- Prove/verify round-trip in Rust.
- `forge test` on `Groth16Verifier` with known-good and known-bad proof fixtures.
- Settlement: reads proven outputs, rejects `Poseidon ≠ D`, pays correctly; escrow
  routing from registry recognition unchanged.

### 8.3 Full gate stays green

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `(cd chain && forge test)`, and the now-**eight**
`make -C ops …-demo`s (adding `zk-demo`).

---

## 9. Roadmap / project-map sync (at merge)

Flip together: `ROADMAP.md` and `docs/roadmap.md` H2 → done-for-cycle-1 with the
deferred sub-items listed; the "What is real vs stubbed" *Usage proofs* row updated;
`project-map.js` node status + `roadmap[]` entry + `project.updated`. Keep it factual:
H2 cycle 1 (integrity-first proof + dedup) done; work-blinding, manifest, tier,
cross-epoch, real beacon still open.
