# H2 — ZK usage proofs (cycle 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the trusted disclosure file and accept-all verifier with a real Groth16 usage proof that a on-chain verifier checks, so usage is cryptographically forced honest (well-formed, range-bounded, diminishing-returns-capped, epoch-bound, per-work-unique) while raw per-user usage never leaves the device.

**Architecture:** A new native `libs/zk-circuits` crate holds an arkworks Groth16/BN254 circuit, native Poseidon, the prover, a deterministic devnet trusted-setup, and a Solidity-verifier codegen. The circuit squeezes all public outputs into one Poseidon digest `D`; on-chain a `Groth16Verifier` checks the ~200-byte proof against `D` only (cheap), and off-chain the settlement job recomputes `Poseidon(plaintext) == D` in Rust before trusting the per-work weights. `wallet-zk` migrates commitments keccak→Poseidon; `CWEConsumption` + a new `CWEEpochBeacon` carry the new submission; settlement deletes the disclosure file and pays from proven weights; the native player generates real proofs; a three-act `make zk-demo` proves the guarantee bites.

**Tech Stack:** Rust (arkworks 0.5: `ark-bn254`, `ark-groth16`, `ark-relations`, `ark-r1cs-std`, `ark-crypto-primitives` sponge, `ark-ff`, `ark-serialize`, `ark-snark`, `ark-std`), Solidity 0.8.24 (Foundry), alloy (settlement chain layer), existing `cwe-dapr`/`cwe-wallet-zk`.

## Global Constraints

- **No AI attribution anywhere** — code, comments, docs, commit messages, branch names. Ordinary human-authored work only. (Copied verbatim from project CLAUDE.md.)
- **Rust everywhere except Solidity** under `chain/`. New crate is Rust.
- **Every function/method gets a `///` doc comment** describing in detail what it does; non-trivial lines get an inline comment only when it adds understanding.
- **Deterministic integer math** — no floating point anywhere on the settlement/proof path; results must be bit-for-bit reproducible. In-circuit "division" is proven via a quotient+remainder witness with a range-checked remainder.
- **Full gate stays green** at every commit boundary: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `(cd chain && forge test)`, and the `make -C ops …-demo` set. (`forge fmt` is NOT run — the repo has never run it.)
- **Foundry** lives at `$HOME/.foundry/bin` (add to `PATH` for `forge`).
- **Work on branch `feature/h2-zk-usage-proofs`** (already created); commit frequently. Do not push until the branch is finished and merged per `finishing-a-development-branch`.
- **Constants (locked):** `MAX_WORKS = 16`, `MAX_PLAYS_CIRCUIT = 64`. A user with >16 works submits multiple proofs; plays are capped at 64 in-circuit.
- **Trusted setup is devnet-only** — a deterministic, fixed-seed key, loudly labeled insecure in code and docs. A real ceremony is a production deferral.

---

## File structure

**New — `libs/zk-circuits/` (native crate, added to workspace `members`):**
- `Cargo.toml` — arkworks deps; depends on `cwe-dapr` (for the DR table) and `cwe-wallet-zk` (for `Bytes32`).
- `src/lib.rs` — crate root, re-exports, `MAX_WORKS`/`MAX_PLAYS_CIRCUIT`.
- `src/field.rs` — `Bytes32 ↔ Fr` conversions.
- `src/poseidon.rs` — shared `PoseidonConfig`, native `poseidon_hash`, commitment/pseudonym/digest helpers.
- `src/dr.rs` — `d_ppm_table` (from `cwe_dapr::d_ppm`) + native `weight_of` matching the sim's `raw`.
- `src/circuit.rs` — the `UsageCircuit` constraint system and its gadgets.
- `src/prove.rs` — `UsageRowInput`, `PublicRow`, `ProofBundle`, `prove`, `verify`.
- `src/setup.rs` — deterministic devnet `ProvingKey`/`VerifyingKey` generation + `ark-serialize` load/save.
- `src/bin/gen_verifier.rs` — emits `chain/contracts/Groth16Verifier.sol` from the `VerifyingKey`.
- `src/bin/export_keys.rs` — writes the devnet keys + a proof/pubinput fixture for the forge test.

**Modified — Rust:**
- `Cargo.toml` (workspace) — add member + workspace deps for arkworks.
- `libs/wallet-zk/Cargo.toml`, `libs/wallet-zk/src/commit.rs` — commitment keccak→Poseidon behind `Opening::commit()`.
- `libs/wallet-zk/src/zk.rs` — replace `none-v0` placeholder with a thin re-export/bridge to `zk-circuits` proof bundle types (native-only; kept behind a feature so the wasm build stays light).
- `sims/src/lib.rs` — refactor: expose `RawRow` + `allocate_from_raw`, have `allocate` delegate.
- `services/settlement/src/settle.rs`, `src/chain.rs`, `src/config.rs`, `src/lib.rs` — delete `disclosure.rs`; read proven weights from the extended event; registry-derived escrow routing; call `allocate_from_raw`.
- `clients/player-plugin/*` — Poseidon commitments (free via wallet-zk) + generate/submit the real proof bundle.
- `clients/browser-ext/core/*` — Poseidon commitments; keep placeholder proof path (labeled), real in-wasm proving deferred.

**Modified/New — Solidity (`chain/`):**
- `contracts/interfaces/IProofVerifier.sol` — signature → `verify(bytes32 digest, bytes proof)`.
- `contracts/Groth16Verifier.sol` — **generated**; real `IProofVerifier`.
- `contracts/CWEEpochBeacon.sol` — new; publishes per-epoch `K_epoch`.
- `contracts/CWEConsumption.sol` + `interfaces/ICWEConsumption.sol` — extended `submitConsumption`, extended event.
- `script/Deploy.s.sol` — deploy beacon + Groth16Verifier; drop AcceptAllVerifier from the path.
- `contracts/AcceptAllVerifier.sol` — moved to `chain/test/` as a permissive stub if any unit test needs it, else deleted.
- `test/*` — forge tests for the verifier, beacon, consumption.

**Modified — ops / CI / docs:**
- `ops/Makefile` — `zk-demo` target + a `zk_demo.sh` script.
- `ops/zk_demo.sh` (new) — the three-act demo.
- `.github/workflows/ci.yml` — `zk-e2e` job.
- `ROADMAP.md`, `docs/roadmap.md`, `project-map.js` — status sync (final task).

---

## Phase 0 — crate scaffold, field & Poseidon

### Task 1: Scaffold `zk-circuits` and pin arkworks

**Files:**
- Create: `libs/zk-circuits/Cargo.toml`, `libs/zk-circuits/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members` + `[workspace.dependencies]`)

**Interfaces:**
- Produces: crate `cwe-zk-circuits` with `pub const MAX_WORKS: usize = 16;` and `pub const MAX_PLAYS_CIRCUIT: u64 = 64;`.

- [ ] **Step 1: Add the crate to the workspace.** In root `Cargo.toml`, add `"libs/zk-circuits",` to `members` (after `libs/wallet-zk`), and under `[workspace.dependencies]` add:

```toml
ark-bn254 = { version = "0.5", default-features = false, features = ["curve"] }
ark-ff = { version = "0.5", default-features = false }
ark-ec = { version = "0.5", default-features = false }
ark-relations = { version = "0.5", default-features = false }
ark-r1cs-std = { version = "0.5", default-features = false }
ark-crypto-primitives = { version = "0.5", default-features = false, features = ["sponge", "crh", "r1cs"] }
ark-groth16 = { version = "0.5", default-features = false }
ark-snark = { version = "0.5", default-features = false }
ark-serialize = { version = "0.5", default-features = false }
ark-std = { version = "0.5", default-features = false }
```

- [ ] **Step 2: Write `libs/zk-circuits/Cargo.toml`.**

```toml
[package]
name = "cwe-zk-circuits"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
ark-bn254.workspace = true
ark-ff.workspace = true
ark-ec.workspace = true
ark-relations.workspace = true
ark-r1cs-std.workspace = true
ark-crypto-primitives.workspace = true
ark-groth16.workspace = true
ark-snark.workspace = true
ark-serialize.workspace = true
ark-std.workspace = true
cwe-dapr = { path = "../../sims" }
cwe-wallet-zk = { path = "../wallet-zk" }
hex.workspace = true

[dev-dependencies]
```

Note: the `sims` crate's package name is `cwe-dapr` and `wallet-zk`'s is `cwe-wallet-zk` (confirm via their `Cargo.toml` `name`); adjust the `path` deps if the names differ.

- [ ] **Step 3: Write `libs/zk-circuits/src/lib.rs`** with the module declarations and constants:

```rust
//! Zero-knowledge usage-proof circuits (H2 cycle 1).
//!
//! A Groth16/BN254 circuit proving a user's per-epoch usage is well-formed,
//! range-bounded, diminishing-returns-capped, epoch-bound, and per-work unique,
//! exposing only a single Poseidon digest on-chain. See
//! `docs/superpowers/specs/2026-07-24-zk-usage-proofs-design.md`.

pub mod circuit;
pub mod dr;
pub mod field;
pub mod poseidon;
pub mod prove;
pub mod setup;

/// Maximum works a single proof covers; heavier users submit multiple proofs.
pub const MAX_WORKS: usize = 16;
/// In-circuit cap on repeat plays; `d_ppm` is near its floor by here, so the
/// bounded lookup table stays small without materially changing payouts.
pub const MAX_PLAYS_CIRCUIT: u64 = 64;
```

- [ ] **Step 4: Create empty module files** so the crate compiles: `field.rs`, `poseidon.rs`, `dr.rs`, `circuit.rs`, `prove.rs`, `setup.rs` each with a one-line `//!` doc comment.

- [ ] **Step 5: Verify it builds.** Run: `cargo build -p cwe-zk-circuits`. Expected: compiles (empty modules).

- [ ] **Step 6: Commit.**

```bash
git add Cargo.toml libs/zk-circuits
git commit -m "zk-circuits: scaffold crate and pin arkworks deps"
```

### Task 2: Field conversions (`Bytes32 ↔ Fr`)

**Files:**
- Modify: `libs/zk-circuits/src/field.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces:
  - `pub type Fr = ark_bn254::Fr;`
  - `pub fn fr_from_bytes32(b: &[u8; 32]) -> Fr` — interpret as a big-endian integer reduced mod the BN254 scalar field order.
  - `pub fn fr_to_bytes32(f: &Fr) -> [u8; 32]` — canonical big-endian 32-byte encoding (values are `< p`, so this is lossless).
  - `pub fn fr_from_u128(v: u128) -> Fr`.

- [ ] **Step 1: Write the failing test.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical field element round-trips through bytes32 losslessly.
    #[test]
    fn small_value_round_trips() {
        let f = fr_from_u128(123_456_789);
        let b = fr_to_bytes32(&f);
        assert_eq!(fr_from_bytes32(&b), f);
    }

    /// Big-endian: the integer 1 encodes with its low byte last.
    #[test]
    fn one_is_big_endian() {
        let b = fr_to_bytes32(&fr_from_u128(1));
        assert_eq!(b[31], 1);
        assert_eq!(b[..31], [0u8; 31]);
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits field::`. Expected: FAIL (functions undefined).

- [ ] **Step 3: Implement.**

```rust
use ark_ff::{BigInteger, PrimeField};

/// The BN254 scalar field — the field every circuit value lives in.
pub type Fr = ark_bn254::Fr;

/// Interpret 32 big-endian bytes as an integer and reduce it mod the field order.
/// Used for opaque 256-bit inputs (work id, salt, epoch key) that may exceed `p`.
pub fn fr_from_bytes32(b: &[u8; 32]) -> Fr {
    Fr::from_be_bytes_mod_order(b)
}

/// Encode a field element as canonical big-endian 32 bytes. Field elements are
/// always `< p < 2^254`, so the value fits and the encoding is lossless.
pub fn fr_to_bytes32(f: &Fr) -> [u8; 32] {
    let mut out = [0u8; 32];
    let be = f.into_bigint().to_bytes_be(); // minimal-length big-endian
    out[32 - be.len()..].copy_from_slice(&be);
    out
}

/// Lift a `u128` into the field (always canonical, never reduced).
pub fn fr_from_u128(v: u128) -> Fr {
    Fr::from(v)
}
```

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits field::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/field.rs
git commit -m "zk-circuits: Bytes32<->Fr field conversions"
```

### Task 3: Poseidon (native) + commitment/pseudonym/digest helpers

**Files:**
- Modify: `libs/zk-circuits/src/poseidon.rs`
- Test: same file

**Interfaces:**
- Produces:
  - `pub fn poseidon_config() -> ark_crypto_primitives::sponge::poseidon::PoseidonConfig<Fr>` — the ONE config used natively and in-circuit (agreement guarantee).
  - `pub fn poseidon_hash(inputs: &[Fr]) -> Fr` — fixed-rate sponge absorb→squeeze.
  - `pub fn commitment(work_id: &[u8;32], minutes: u64, plays: u64, salt: &[u8;32]) -> [u8;32]` — `Poseidon(work_id, minutes, plays, salt)`.
  - `pub fn pseudonym(k_epoch: &[u8;32], commitment: &[u8;32]) -> [u8;32]` — `Poseidon(k_epoch, C)`.
  - `pub fn digest(epoch: u64, tier: &[u8;32], k_epoch: &[u8;32], rows: &[crate::prove::PublicRow]) -> [u8;32]` — the on-chain public input `D` (defined precisely in Task 8; a forward declaration here is fine — implement `digest` in Task 8 once `PublicRow` exists, and keep only `poseidon_config`/`poseidon_hash`/`commitment`/`pseudonym` in this task).

- [ ] **Step 1: Write the failing test** (native Poseidon determinism + input-sensitivity; the in-circuit agreement test is Task 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The same opening always yields the same commitment; changing plays changes it.
    #[test]
    fn commitment_is_deterministic_and_binds_plays() {
        let w = [1u8; 32];
        let s = [9u8; 32];
        let c1 = commitment(&w, 60, 3, &s);
        assert_eq!(c1, commitment(&w, 60, 3, &s));
        assert_ne!(c1, commitment(&w, 60, 4, &s)); // plays bound
        assert_ne!(c1, commitment(&w, 61, 3, &s)); // minutes bound
        assert_ne!(c1, commitment(&w, 60, 3, &[8u8; 32])); // salt hides
    }

    /// Pseudonym depends on both the epoch key and the commitment.
    #[test]
    fn pseudonym_binds_epoch_and_commitment() {
        let c = commitment(&[1u8; 32], 10, 1, &[2u8; 32]);
        let p = pseudonym(&[7u8; 32], &c);
        assert_ne!(p, pseudonym(&[8u8; 32], &c)); // epoch-bound (anti-replay)
        assert_ne!(p, c);
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits poseidon::`. Expected: FAIL.

- [ ] **Step 3: Implement.** Use arkworks' Poseidon over `Fr` with parameters generated for the field (so native and R1CS share one config):

```rust
use ark_crypto_primitives::sponge::poseidon::{find_poseidon_ark_and_mds, PoseidonConfig};
use ark_crypto_primitives::sponge::{poseidon::PoseidonSponge, CryptographicSponge};

use crate::field::{fr_from_bytes32, fr_from_u128, fr_to_bytes32, Fr};

/// The single Poseidon configuration shared by the native hash and the in-circuit
/// gadget. Both derive from these exact round/MDS constants, so a commitment made
/// natively verifies inside the circuit bit-for-bit. Parameters: rate 2, capacity
/// 1 (width 3), full/partial rounds standard for a 128-bit-secure BN254 sponge.
pub fn poseidon_config() -> PoseidonConfig<Fr> {
    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5u64; // S-box exponent x^5 (BN254 standard)
    let rate = 2;
    let capacity = 1;
    // 255-bit field → 255 in the prime-bit-size argument.
    let (ark, mds) = find_poseidon_ark_and_mds::<Fr>(
        255, rate + capacity, full_rounds, partial_rounds, 0,
    );
    PoseidonConfig::new(full_rounds as usize, partial_rounds as usize, alpha, mds, ark, rate, capacity)
}

/// Absorb `inputs` and squeeze one field element — the native Poseidon hash used
/// for commitments, pseudonyms, and the public-output digest.
pub fn poseidon_hash(inputs: &[Fr]) -> Fr {
    let mut sponge = PoseidonSponge::new(&poseidon_config());
    sponge.absorb(&inputs.to_vec());
    sponge.squeeze_field_elements(1)[0]
}

/// `Poseidon(work_id, minutes, plays, salt)` — the usage commitment. `work_id`
/// and `salt` are reduced into the field; `minutes`/`plays` lift canonically.
pub fn commitment(work_id: &[u8; 32], minutes: u64, plays: u64, salt: &[u8; 32]) -> [u8; 32] {
    let f = poseidon_hash(&[
        fr_from_bytes32(work_id),
        fr_from_u128(minutes as u128),
        fr_from_u128(plays as u128),
        fr_from_bytes32(salt),
    ]);
    fr_to_bytes32(&f)
}

/// `Poseidon(k_epoch, C)` — the epoch-bound pseudonym.
pub fn pseudonym(k_epoch: &[u8; 32], commitment: &[u8; 32]) -> [u8; 32] {
    let f = poseidon_hash(&[fr_from_bytes32(k_epoch), fr_from_bytes32(commitment)]);
    fr_to_bytes32(&f)
}
```

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits poseidon::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/poseidon.rs
git commit -m "zk-circuits: native Poseidon + commitment/pseudonym helpers"
```

### Task 4: DR lookup table + native weight (matches `cwe_dapr`)

**Files:**
- Modify: `libs/zk-circuits/src/dr.rs`
- Test: same file

**Interfaces:**
- Produces:
  - `pub fn d_ppm_table(k_ppm: u64) -> [u64; MAX_PLAYS_CIRCUIT as usize]` — entry `i` is `cwe_dapr::d_ppm(i+1, k_ppm)`.
  - `pub fn weight_of(minutes: u64, plays: u64, price_ppm: u64, region_ppm: u64, k_ppm: u64) -> u128` — the sim's `raw = value·d_ppm/1e6` with plays capped at `MAX_PLAYS_CIRCUIT`.

- [ ] **Step 1: Write the failing test** (agreement with the sim on capped inputs):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_PLAYS_CIRCUIT;

    /// The table entries equal the sim's d_ppm for plays 1..=MAX.
    #[test]
    fn table_matches_sim() {
        let k = 1_000_000;
        let t = d_ppm_table(k);
        for j in 1..=MAX_PLAYS_CIRCUIT {
            assert_eq!(t[(j - 1) as usize], cwe_dapr::d_ppm(j, k));
        }
    }

    /// weight_of reproduces the sim's raw = value·D(plays)/1e6 for capped plays.
    #[test]
    fn weight_matches_sim_raw() {
        let k = 1_000_000;
        // value = 60 * 1_000_000 * 1_000_000; raw = value * d_ppm(2) / 1e6.
        let value: u128 = 60u128 * 1_000_000 * 1_000_000;
        let expected = value * (cwe_dapr::d_ppm(2, k) as u128) / 1_000_000;
        assert_eq!(weight_of(60, 2, 1_000_000, 1_000_000, k), expected);
    }

    /// Plays above the circuit cap are clamped to the cap (not the sim's 100k cap).
    #[test]
    fn plays_clamped_to_circuit_cap() {
        let k = 1_000_000;
        assert_eq!(
            weight_of(10, 1_000_000, 1_000_000, 1_000_000, k),
            weight_of(10, MAX_PLAYS_CIRCUIT, 1_000_000, 1_000_000, k)
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits dr::`. Expected: FAIL.

- [ ] **Step 3: Implement.**

```rust
use crate::MAX_PLAYS_CIRCUIT;

/// The diminishing-returns multiplier table: entry `i` is `d_ppm(i+1)` in ppm,
/// generated directly from `cwe_dapr::d_ppm` so the circuit and the payout sim can
/// never disagree. Used both to build the in-circuit constant lookup and to check
/// the prover's native weight.
pub fn d_ppm_table(k_ppm: u64) -> [u64; MAX_PLAYS_CIRCUIT as usize] {
    let mut t = [0u64; MAX_PLAYS_CIRCUIT as usize];
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = cwe_dapr::d_ppm((i as u64) + 1, k_ppm);
    }
    t
}

/// The row's DR-capped weight `raw = minutes·price_ppm·region_ppm · D(plays) / 1e6`,
/// with `plays` clamped to `MAX_PLAYS_CIRCUIT` (1 if zero). Integer math only,
/// mirroring `cwe_dapr`'s `UsageRow::value` then `mul_div(value, d_ppm, 1e6)`.
pub fn weight_of(minutes: u64, plays: u64, price_ppm: u64, region_ppm: u64, k_ppm: u64) -> u128 {
    let v = plays.clamp(1, MAX_PLAYS_CIRCUIT);
    let d = d_ppm_table(k_ppm)[(v - 1) as usize] as u128;
    let value = (minutes as u128) * (price_ppm as u128) * (region_ppm as u128);
    value * d / 1_000_000
}
```

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits dr::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/dr.rs
git commit -m "zk-circuits: DR lookup table + native weight matching the sim"
```

---

## Phase 1 — the circuit

### Task 5: Poseidon gadget agreement (in-circuit == native)

**Files:**
- Modify: `libs/zk-circuits/src/circuit.rs`
- Test: same file

**Interfaces:**
- Produces: `pub(crate) fn poseidon_hash_gadget(cs, inputs: &[FpVar<Fr>]) -> Result<FpVar<Fr>, SynthesisError>` — the in-circuit hash using `poseidon_config()`.

- [ ] **Step 1: Write the failing test** — build a tiny constraint system, hash two vars in-circuit, and assert the witnessed output equals the native hash:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::fr_from_u128;
    use crate::poseidon::poseidon_hash;
    use ark_r1cs_std::alloc::AllocVar;
    use ark_r1cs_std::fields::fp::FpVar;
    use ark_relations::r1cs::ConstraintSystem;

    /// The in-circuit Poseidon output equals the native one for the same inputs.
    #[test]
    fn gadget_matches_native() {
        let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
        let a = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(3))).unwrap();
        let b = FpVar::new_witness(cs.clone(), || Ok(fr_from_u128(5))).unwrap();
        let out = poseidon_hash_gadget(cs.clone(), &[a, b]).unwrap();
        let native = poseidon_hash(&[fr_from_u128(3), fr_from_u128(5)]);
        assert_eq!(out.value().unwrap(), native);
        assert!(cs.is_satisfied().unwrap());
    }
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits circuit::tests::gadget_matches_native`. Expected: FAIL.

- [ ] **Step 3: Implement** using `PoseidonSpongeVar`:

```rust
use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};

use crate::field::Fr;
use crate::poseidon::poseidon_config;

/// The in-circuit Poseidon hash — absorb `inputs`, squeeze one element — using the
/// exact `poseidon_config()` the native hash uses, guaranteeing agreement.
pub(crate) fn poseidon_hash_gadget(
    cs: ConstraintSystemRef<Fr>,
    inputs: &[FpVar<Fr>],
) -> Result<FpVar<Fr>, SynthesisError> {
    let mut sponge = PoseidonSpongeVar::new(cs, &poseidon_config());
    sponge.absorb(&inputs.to_vec())?;
    Ok(sponge.squeeze_field_elements(1)?[0].clone())
}
```

Add the necessary `use` lines at the top of `circuit.rs`.

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits circuit::tests::gadget_matches_native`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/circuit.rs
git commit -m "zk-circuits: in-circuit Poseidon gadget agrees with native"
```

### Task 6: The `UsageCircuit` — witness layout, commitment & range constraints

**Files:**
- Modify: `libs/zk-circuits/src/circuit.rs`
- Test: same file

**Interfaces:**
- Produces:
  - `pub struct RowWitness { pub active: bool, pub work_id: [u8;32], pub minutes: u64, pub plays: u64, pub salt: [u8;32], pub price_ppm: u64, pub region_ppm: u64, pub weight: u128, pub commitment: [u8;32], pub pseudonym: [u8;32] }`
  - `pub struct UsageCircuit { pub epoch: u64, pub tier: [u8;32], pub k_epoch: [u8;32], pub k_ppm: u64, pub rows: Vec<RowWitness>, pub digest: [u8;32] }`
  - `impl ConstraintSynthesizer<Fr> for UsageCircuit` (constraints grown across Tasks 6–8; this task lands commitment-correctness + range/well-formedness).
  - `pub const MINUTES_BITS: usize = 32; pub const PLAYS_BITS: usize = 7; pub const PPM_BITS: usize = 40;`

- [ ] **Step 1: Write the failing test** — a hand-built single-active-row circuit is satisfied; corrupting the commitment breaks it:

```rust
#[test]
fn commitment_and_range_constraints() {
    use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
    let row = super::test_row(&[1u8; 32], 60, 2, &[9u8; 32], 1_000_000, 1_000_000);
    let circ = super::test_circuit(vec![row.clone()]);
    let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
    circ.clone().generate_constraints(cs.clone()).unwrap();
    assert!(cs.is_satisfied().unwrap());

    // Corrupt the commitment: constraints must now be violated.
    let mut bad = circ;
    bad.rows[0].commitment = [0xEE; 32];
    let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
    bad.generate_constraints(cs2.clone()).unwrap();
    assert!(!cs2.is_satisfied().unwrap());
}
```

Add `test_row`/`test_circuit` helpers in the same `#[cfg(test)]` module that populate `RowWitness` (computing `weight`, `commitment`, `pseudonym` via the native helpers) and build a `UsageCircuit` with `MAX_WORKS` rows (padding inactive rows with all-zero fields), and the digest via the native `digest` (available after Task 8; for this task compute a placeholder digest of `Fr::zero()` and DEFER the digest equality constraint to Task 8).

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits circuit::tests::commitment_and_range_constraints`. Expected: FAIL.

- [ ] **Step 3: Implement the witness structs and the first constraints.** For each of the `MAX_WORKS` rows, allocate witness vars, then enforce (guarded by `active`):
  - **Commitment:** `commitment_var == poseidon_hash_gadget([work_id, minutes, plays, salt])`.
  - **Range:** bit-decompose `minutes` (≤ `MINUTES_BITS`), `plays` (≤ `PLAYS_BITS` ⇒ ≤127, and separately `plays ≤ MAX_PLAYS_CIRCUIT` via a `<=` gadget), `price_ppm`/`region_ppm` (≤ `PPM_BITS`).
  - **Well-formed:** for `active` rows enforce `work_id != 0`; for inactive rows enforce `work_id == 0`, `weight == 0`, `commitment` and `pseudonym` equal the canonical padded values (`commitment` of the all-zero opening, `pseudonym` of that commitment) so the digest is deterministic.

Reference gadgets: `FpVar::new_witness`, `Boolean::new_witness`, `field element .to_bits_le()` / `UInt64`, `FpVar::is_eq`, `EqGadget::conditional_enforce_equal`. Represent `active` as a `Boolean<Fr>`. Keep each row's logic in a private `enforce_row(cs, &RowWitness) -> Result<RowVars, SynthesisError>` helper returning the allocated `work_id_var`, `weight_var`, `pseudonym_var`, `active_var` for later tasks.

Write full gadget code here; do not abbreviate. Enforce `plays <= MAX_PLAYS_CIRCUIT` with a constant-comparison gadget, since the native `weight_of` clamps and the DR lookup (Task 7) indexes within `[1, MAX_PLAYS_CIRCUIT]`.

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits circuit::tests::commitment_and_range_constraints`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/circuit.rs
git commit -m "zk-circuits: UsageCircuit commitment-correctness + range constraints"
```

### Task 7: DR-cap constraint (lookup + proven division) and epoch binding

**Files:**
- Modify: `libs/zk-circuits/src/circuit.rs`
- Test: same file

**Interfaces:**
- Consumes: `RowVars` from Task 6, `d_ppm_table` from Task 4, `poseidon_hash_gadget` from Task 5.
- Produces: DR-cap + pseudonym constraints folded into `enforce_row`.

- [ ] **Step 1: Write the failing test** — a row whose `weight` is inflated by 1 fails; a row whose `pseudonym` is wrong (mismatched `k_epoch`) fails:

```rust
#[test]
fn weight_and_pseudonym_constraints() {
    use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
    let base = super::test_row(&[1u8; 32], 60, 2, &[9u8; 32], 1_000_000, 1_000_000);

    // Inflated weight → violated.
    let mut infl = super::test_circuit(vec![base.clone()]);
    infl.rows[0].weight += 1;
    let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
    infl.generate_constraints(cs.clone()).unwrap();
    assert!(!cs.is_satisfied().unwrap());

    // Wrong pseudonym → violated.
    let mut wp = super::test_circuit(vec![base]);
    wp.rows[0].pseudonym = [0x77; 32];
    let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
    wp.generate_constraints(cs2.clone()).unwrap();
    assert!(!cs2.is_satisfied().unwrap());
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits circuit::tests::weight_and_pseudonym_constraints`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - **DR lookup:** build the `MAX_PLAYS_CIRCUIT` constant `FpVar`s from `d_ppm_table(k_ppm)` (with `k_ppm` a circuit constant for the MVP — the neutral `1_000_000`; document that governance-tunable `k` is a deferral). Select `d = table[plays-1]` with a one-hot selector proven against the `plays` witness (`Σ selector_i == 1`, `Σ i·selector_i == plays-1`, `d == Σ selector_i · table_i`).
  - **Proven division:** enforce `value == minutes·price·region` (field mult), then `weight·1_000_000 + r == value·d` with `0 ≤ r < 1_000_000` (range-check `r` via bit decomposition). This proves `weight == value·d / 1_000_000` (floor), matching `weight_of`.
  - **Epoch binding:** `pseudonym_var == poseidon_hash_gadget([k_epoch, commitment_var])`, guarded by `active` (inactive rows already pinned to the canonical padded pseudonym in Task 6).

Write full gadget code. All three apply only to `active` rows (`conditional_enforce_*`).

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits circuit::tests::weight_and_pseudonym_constraints`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/circuit.rs
git commit -m "zk-circuits: DR-cap (lookup+proven division) + epoch-binding constraints"
```

### Task 8: Uniqueness + digest binding (public input)

**Files:**
- Modify: `libs/zk-circuits/src/circuit.rs`, `libs/zk-circuits/src/poseidon.rs` (add `digest`), `libs/zk-circuits/src/prove.rs` (add `PublicRow`)
- Test: `circuit.rs`

**Interfaces:**
- Produces:
  - `prove.rs`: `pub struct PublicRow { pub work_id: [u8;32], pub commitment: [u8;32], pub pseudonym: [u8;32], pub weight: u128 }`
  - `poseidon.rs`: `pub fn digest(epoch: u64, tier: &[u8;32], k_epoch: &[u8;32], rows: &[PublicRow]) -> [u8;32]` — `Poseidon(epoch, tier, k_epoch, [pseudonym_w, work_id_w, weight_w for w in 0..MAX_WORKS])` (fixed length `3 + 3·MAX_WORKS`; inactive rows contribute their canonical padded values).
  - `circuit.rs`: the digest is the SINGLE public input; uniqueness enforced over active rows.

- [ ] **Step 1: Write the failing tests** — (a) duplicate active work ids fail; (b) the digest is a public input and equals the native `digest`; (c) a tampered public digest fails:

```rust
#[test]
fn uniqueness_and_digest() {
    use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};
    // Two active rows, same work id → uniqueness violated.
    let r1 = super::test_row(&[1u8; 32], 10, 1, &[9u8; 32], 1_000_000, 1_000_000);
    let r2 = super::test_row(&[1u8; 32], 20, 1, &[8u8; 32], 1_000_000, 1_000_000);
    let dup = super::test_circuit(vec![r1, r2]);
    let cs = ConstraintSystem::<crate::field::Fr>::new_ref();
    dup.generate_constraints(cs.clone()).unwrap();
    assert!(!cs.is_satisfied().unwrap());

    // Distinct rows with the correct digest → satisfied.
    let a = super::test_row(&[1u8; 32], 10, 1, &[9u8; 32], 1_000_000, 1_000_000);
    let b = super::test_row(&[2u8; 32], 20, 1, &[8u8; 32], 1_000_000, 1_000_000);
    let ok = super::test_circuit(vec![a, b]);
    let cs2 = ConstraintSystem::<crate::field::Fr>::new_ref();
    ok.clone().generate_constraints(cs2.clone()).unwrap();
    assert!(cs2.is_satisfied().unwrap());

    // Tamper the public digest → violated.
    let mut bad = ok;
    bad.digest = [0x00; 32];
    let cs3 = ConstraintSystem::<crate::field::Fr>::new_ref();
    bad.generate_constraints(cs3.clone()).unwrap();
    assert!(!cs3.is_satisfied().unwrap());
}
```

Update the `test_circuit` helper to compute `digest` via the native `poseidon::digest` now that it exists.

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits circuit::tests::uniqueness_and_digest`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - `PublicRow` in `prove.rs`; `digest` in `poseidon.rs` (flatten fields to `Fr` in the fixed order above and `poseidon_hash`).
  - **Uniqueness gadget:** for every pair `i<j`, enforce `active_i · active_j · (work_id_i == work_id_j) == 0` (i.e. not both active and equal). `O(MAX_WORKS²)=256` pairs.
  - **Digest binding:** allocate `digest` as `FpVar::new_input` (the single public input). Recompute the digest in-circuit from `epoch`, `tier`, `k_epoch` (allocated as witnesses/constants) and the per-row `pseudonym_var`/`work_id_var`/`weight_var`, and enforce equality with the public input.

Write full gadget code.

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits circuit::`. Expected: PASS (all circuit tests).

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/circuit.rs libs/zk-circuits/src/poseidon.rs libs/zk-circuits/src/prove.rs
git commit -m "zk-circuits: uniqueness + single-public-input digest binding"
```

---

## Phase 2 — prover, setup, verifier

### Task 9: Deterministic devnet setup (proving/verifying keys)

**Files:**
- Modify: `libs/zk-circuits/src/setup.rs`
- Test: same file

**Interfaces:**
- Produces:
  - `pub fn devnet_setup() -> (ark_groth16::ProvingKey<ark_bn254::Bn254>, ark_groth16::VerifyingKey<ark_bn254::Bn254>)` — a FIXED-SEED, INSECURE, DEVNET-ONLY Groth16 setup over an all-inactive `UsageCircuit` shape.
  - `pub fn save_pk(path)`, `pub fn load_pk(path)`, `pub fn save_vk(path)`, `pub fn load_vk(path)` via `ark-serialize` (compressed).

- [ ] **Step 1: Write the failing test** — setup is deterministic (same seed → identical VK bytes):

```rust
#[test]
fn devnet_setup_is_deterministic() {
    let (_, vk1) = devnet_setup();
    let (_, vk2) = devnet_setup();
    let mut a = Vec::new();
    let mut b = Vec::new();
    vk1.serialize_compressed(&mut a).unwrap();
    vk2.serialize_compressed(&mut b).unwrap();
    assert_eq!(a, b, "fixed-seed setup must be reproducible");
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits setup::`. Expected: FAIL.

- [ ] **Step 3: Implement** with a seeded RNG (`ark_std::rand::rngs::StdRng` via `SeedableRng::from_seed([0u8;32])`) and `Groth16::<Bn254>::circuit_specific_setup(empty_circuit, &mut rng)`. Provide an `empty_circuit()` builder producing a `UsageCircuit` with all `MAX_WORKS` rows inactive and a matching digest. Add a **module-level doc comment stating in bold that this key is insecure and for devnet only; production requires a real multi-party ceremony.**

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits setup::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/setup.rs
git commit -m "zk-circuits: deterministic devnet Groth16 setup (labeled insecure)"
```

### Task 10: `prove` / `verify` round-trip

**Files:**
- Modify: `libs/zk-circuits/src/prove.rs`
- Test: same file

**Interfaces:**
- Consumes: `UsageCircuit`, `devnet_setup`, `digest`, native helpers.
- Produces:
  - `pub struct UsageRowInput { pub work_id: [u8;32], pub minutes: u64, pub plays: u64, pub salt: [u8;32], pub price_ppm: u64, pub region_ppm: u64 }`
  - `pub struct ProofBundle { pub proof: Vec<u8>, pub digest: [u8;32], pub epoch: u64, pub tier: [u8;32], pub k_epoch: [u8;32], pub rows: Vec<PublicRow> }`
  - `pub fn prove(pk, epoch: u64, tier: &[u8;32], k_epoch: &[u8;32], k_ppm: u64, inputs: &[UsageRowInput]) -> Result<ProofBundle, ProveError>`
  - `pub fn verify(vk, digest: &[u8;32], proof: &[u8]) -> bool`

- [ ] **Step 1: Write the failing test** — honest bundle verifies; a bundle checked against a tampered digest fails:

```rust
#[test]
fn prove_verify_round_trip() {
    let (pk, vk) = crate::setup::devnet_setup();
    let inputs = vec![UsageRowInput {
        work_id: [1u8; 32], minutes: 60, plays: 2,
        salt: [9u8; 32], price_ppm: 1_000_000, region_ppm: 1_000_000,
    }];
    let bundle = prove(&pk, 7, &[0xAB; 32], &[0x5c; 32], 1_000_000, &inputs).unwrap();
    assert!(verify(&vk, &bundle.digest, &bundle.proof));
    // The digest binds the outputs; a different digest must fail.
    assert!(!verify(&vk, &[0u8; 32], &bundle.proof));
    // The proven weight equals the native weight.
    assert_eq!(bundle.rows[0].weight, crate::dr::weight_of(60, 2, 1_000_000, 1_000_000, 1_000_000));
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-zk-circuits prove::`. Expected: FAIL.

- [ ] **Step 3: Implement.** `prove` builds `RowWitness`es (computing `weight`/`commitment`/`pseudonym` natively, padding to `MAX_WORKS` inactive rows), computes the native `digest`, builds `PublicRow`s, runs `Groth16::prove`, serializes the proof (compressed) to bytes. `verify` deserializes, maps `digest` bytes→`Fr` public input, and calls `Groth16::verify`. Reject `>MAX_WORKS` inputs with `ProveError::TooManyWorks`.

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-zk-circuits prove::`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/prove.rs
git commit -m "zk-circuits: prove/verify round-trip over the devnet key"
```

### Task 11: Solidity verifier codegen + fixture export

**Files:**
- Create: `libs/zk-circuits/src/bin/gen_verifier.rs`, `libs/zk-circuits/src/bin/export_keys.rs`
- Create (generated): `chain/contracts/Groth16Verifier.sol`, `chain/test/fixtures/zk_proof.json`

**Interfaces:**
- Produces: a Solidity `Groth16Verifier` whose baked-in VK matches `devnet_setup()`, and a JSON fixture `{ digest, proof_a, proof_b, proof_c }` (proof points in the EVM encoding) for a known-good proof, plus a known-bad variant.

- [ ] **Step 1: Write `export_keys.rs`** — runs `devnet_setup()`, `prove()` on a fixed input, and writes: the compressed VK/PK to `chain/zk/` (git-ignored PK; VK committed), and `chain/test/fixtures/zk_proof.json` with the proof re-encoded as the EVM verifier expects (G1 as `(x,y)`, G2 as `(x[2],y[2])`), the public digest as `uint256`, and a `bad_digest` field. Document the exact byte/coordinate ordering (arkworks is little-endian internally; the Solidity verifier expects big-endian `uint256` and specific G2 limb order — convert explicitly and assert a round-trip against `verify`).

- [ ] **Step 2: Write `gen_verifier.rs`** — emits `chain/contracts/Groth16Verifier.sol`: a standard BN254 Groth16 pairing verifier (`alt_bn128` precompiles `0x06/0x07/0x08`) implementing the updated `IProofVerifier` (Task 12). It hardcodes the VK constants (`alpha1`, `beta2`, `gamma2`, `delta2`, `IC[]`) read from `devnet_setup()`, and exposes `verify(bytes32 digest, bytes proof)` which decodes `proof` into `(a, b, c)` and checks the pairing equation with `digest` as the single public input. Include a header comment: SPDX `AGPL-3.0-only`, and a note that the file is generated by `gen_verifier.rs` and the key is devnet-only.

- [ ] **Step 3: Generate the artifacts.** Run:

```bash
cargo run -p cwe-zk-circuits --bin export_keys
cargo run -p cwe-zk-circuits --bin gen_verifier
```

Expected: `Groth16Verifier.sol` and `zk_proof.json` written; the round-trip assertion in `export_keys` passes.

- [ ] **Step 4: Sanity-check it compiles.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge build`. Expected: `Groth16Verifier` compiles.

- [ ] **Step 5: Commit.**

```bash
git add libs/zk-circuits/src/bin chain/contracts/Groth16Verifier.sol chain/test/fixtures/zk_proof.json chain/zk
git commit -m "zk-circuits: generate Solidity Groth16 verifier + proof fixtures"
```

### Task 12: `IProofVerifier` interface change + forge verifier test

**Files:**
- Modify: `chain/contracts/interfaces/IProofVerifier.sol`
- Create: `chain/test/Groth16Verifier.t.sol`

**Interfaces:**
- Produces: `IProofVerifier.verify(bytes32 digest, bytes calldata proof) external view returns (bool)`.

- [ ] **Step 1: Write the failing forge test** — load `zk_proof.json` via `vm.readFile`/`vm.parseJson`, deploy `Groth16Verifier`, assert the good digest+proof verifies and the bad digest does not:

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {Groth16Verifier} from "../contracts/Groth16Verifier.sol";

contract Groth16VerifierTest is Test {
    Groth16Verifier verifier;

    function setUp() public {
        verifier = new Groth16Verifier();
    }

    function test_AcceptsValidProof() public view {
        (bytes32 digest, bytes memory proof) = _loadFixture(false);
        assertTrue(verifier.verify(digest, proof));
    }

    function test_RejectsTamperedDigest() public view {
        (bytes32 badDigest, bytes memory proof) = _loadFixture(true);
        assertFalse(verifier.verify(badDigest, proof));
    }

    // _loadFixture reads chain/test/fixtures/zk_proof.json and ABI-encodes the
    // proof exactly as the verifier's `verify` expects.
    function _loadFixture(bool bad) internal view returns (bytes32, bytes memory) { /* parse JSON */ }
}
```

Implement `_loadFixture` fully (parse the JSON fields written by `export_keys.rs`, `abi.encode` the proof).

- [ ] **Step 2: Run to verify it fails.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test --match-contract Groth16VerifierTest`. Expected: FAIL until the interface + verifier line up.

- [ ] **Step 3: Update `IProofVerifier.sol`** to the new signature (keep the NatSpec, update the `@param` to `digest`). Ensure `Groth16Verifier` (Task 11) declares `is IProofVerifier` and matches.

- [ ] **Step 4: Run to verify it passes.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test --match-contract Groth16VerifierTest`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add chain/contracts/interfaces/IProofVerifier.sol chain/test/Groth16Verifier.t.sol
git commit -m "chain: IProofVerifier takes a digest; forge test for the real verifier"
```

---

## Phase 3 — on-chain intake

### Task 13: `CWEEpochBeacon`

**Files:**
- Create: `chain/contracts/CWEEpochBeacon.sol`, `chain/test/CWEEpochBeacon.t.sol`

**Interfaces:**
- Produces: `CWEEpochBeacon` with `function keyFor(uint256 epoch) external view returns (bytes32)`, an owner-only `setKey(uint256 epoch, bytes32 key)`, and event `EpochKeySet(uint256 indexed epoch, bytes32 key)`.

- [ ] **Step 1: Write the failing forge test** — unset epoch returns zero; owner can set; non-owner reverts; set value reads back.

- [ ] **Step 2: Run to verify it fails.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test --match-contract CWEEpochBeaconTest`. Expected: FAIL.

- [ ] **Step 3: Implement `CWEEpochBeacon.sol`.** Owner set in constructor. `mapping(uint256 => bytes32) private _keys`. Doc comment must state in bold that the MVP publishes a fixed, NON-RANDOM key and a real randomness beacon (VRF/drand) is deferred (`epoch_beacon_specification.md`). SPDX `AGPL-3.0-only`.

- [ ] **Step 4: Run to verify it passes.** Run the same `forge test`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add chain/contracts/CWEEpochBeacon.sol chain/test/CWEEpochBeacon.t.sol
git commit -m "chain: CWEEpochBeacon publishes per-epoch K_epoch (fixed-key MVP)"
```

### Task 14: `CWEConsumption` — extended submission + event

**Files:**
- Modify: `chain/contracts/CWEConsumption.sol`, `chain/contracts/interfaces/ICWEConsumption.sol`, `chain/test/CWEConsumption.t.sol`

**Interfaces:**
- Produces:
  - `submitConsumption(bytes32 tierId, bytes32[] calldata commitments, bytes32[] calldata pseudonyms, bytes32[] calldata workIds, uint256[] calldata weights, bytes32 digest, bytes calldata proof)`
  - event `ConsumptionSubmitted(address indexed user, uint256 indexed epoch, bytes32 tierId, bytes32 digest, bytes32[] pseudonyms, bytes32[] workIds, uint256[] weights)`
  - errors `ArityMismatch()` in addition to the existing ones.

- [ ] **Step 1: Update the failing forge tests** in `CWEConsumption.t.sol`: deploy `CWEConsumption` with a mock verifier that returns configurable bool; assert (a) a matching-digest submission with `verifier=true` emits the extended event and records submitted; (b) `verifier=false` reverts `ProofRejected`; (c) mismatched array lengths revert `ArityMismatch`; (d) a second submission in the same epoch reverts `AlreadySubmitted`. Use a `MockVerifier is IProofVerifier` in the test file.

- [ ] **Step 2: Run to verify it fails.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test --match-contract CWEConsumptionTest`. Expected: FAIL.

- [ ] **Step 3: Implement.** Extend `submitConsumption`: require `commitments.length == pseudonyms.length == workIds.length == weights.length` and non-zero (`ArityMismatch`/`NoCommitments`); call `verifier.verify(digest, proof)` (new signature); on success set `_submitted` and emit the extended event carrying the proven outputs. Update `ICWEConsumption` doc + signature. Keep the one-per-epoch and effects-before-log ordering.

- [ ] **Step 4: Run to verify it passes.** Run the same `forge test`. Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add chain/contracts/CWEConsumption.sol chain/contracts/interfaces/ICWEConsumption.sol chain/test/CWEConsumption.t.sol
git commit -m "chain: CWEConsumption carries pseudonyms/workIds/weights/digest and verifies against digest"
```

### Task 15: Deploy wiring (beacon + Groth16Verifier)

**Files:**
- Modify: `chain/script/Deploy.s.sol`; move/delete `chain/contracts/AcceptAllVerifier.sol`

**Interfaces:**
- Produces: `deployments/localhost.json` gains a `beacon` address; `verifier` is now the `Groth16Verifier`.

- [ ] **Step 1: Update `Deploy.s.sol`** — replace `import`/`new AcceptAllVerifier()` with `Groth16Verifier`; add `import`/`new CWEEpochBeacon(d.owner)`; add `beacon` to the `Deployed` struct and the serialized JSON. If any test needs a permissive verifier, move `AcceptAllVerifier.sol` to `chain/test/`; otherwise delete it (and remove its imports).

- [ ] **Step 2: Verify the deploy script compiles + a dry run.** Run:

```bash
cd chain && PATH="$HOME/.foundry/bin:$PATH" forge build
```

Expected: compiles. (Full broadcast happens in the demo, Task 20.)

- [ ] **Step 3: Run the whole forge suite.** Run: `cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test`. Expected: PASS (all contract tests green).

- [ ] **Step 4: Commit.**

```bash
git add chain/script/Deploy.s.sol chain/contracts chain/test
git commit -m "chain: deploy CWEEpochBeacon + Groth16Verifier; drop AcceptAllVerifier from the path"
```

---

## Phase 4 — commitment migration & payout refactor

### Task 16: `wallet-zk` commitment keccak→Poseidon

**Files:**
- Modify: `libs/wallet-zk/Cargo.toml`, `libs/wallet-zk/src/commit.rs`
- Test: `commit.rs` (existing tests updated)

**Interfaces:**
- Consumes: `cwe_zk_circuits::poseidon::commitment`.
- Produces: `Opening::commit()` now returns the Poseidon commitment (same `Commitment`/`Opening` API).

- [ ] **Step 1: Add the dep.** In `libs/wallet-zk/Cargo.toml`, add `cwe-zk-circuits = { path = "../zk-circuits" }`. (This keeps `wallet-zk` native for the settlement/player path; the wasm extension crate does not depend on the prover — see Task 19.)

- [ ] **Step 2: Update the failing test.** The existing `commit.rs` tests (`commitment_binds_plays`, `minutes_change_commitment`, `salt_changes_commitment`, `verify_matches_only_correct_opening`, `commit_is_deterministic`) should keep passing by behavior; add one asserting the new value equals `cwe_zk_circuits::poseidon::commitment(...)`:

```rust
#[test]
fn commit_matches_zk_poseidon() {
    let o = Opening::new(Bytes32([1; 32]), 60, 2, Bytes32([9; 32]));
    let expected = cwe_zk_circuits::poseidon::commitment(&[1u8; 32], 60, 2, &[9u8; 32]);
    assert_eq!(o.commit().0 .0, expected);
}
```

Remove the `opening_json_round_trip` hex assertion if the encoding note no longer applies; keep the round-trip itself.

- [ ] **Step 3: Run to verify it fails.** Run: `cargo test -p cwe-wallet-zk commit::`. Expected: FAIL (`commit_matches_zk_poseidon`).

- [ ] **Step 4: Implement.** Replace the keccak pre-image logic in `Opening::commit()` with a call to `cwe_zk_circuits::poseidon::commitment(self.work_id.as_bytes(), self.minutes, self.plays, self.salt.as_bytes())`, wrapping the `[u8;32]` in `Commitment(Bytes32(...))`. Update the module doc comment (the pre-image is now a Poseidon hash of four field elements, not a keccak concat).

- [ ] **Step 5: Run to verify it passes.** Run: `cargo test -p cwe-wallet-zk`. Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add libs/wallet-zk/Cargo.toml libs/wallet-zk/src/commit.rs
git commit -m "wallet-zk: commitments use Poseidon (ZK-friendly) behind Opening::commit()"
```

### Task 17: `sims` — expose `allocate_from_raw`

**Files:**
- Modify: `sims/src/lib.rs`
- Test: `sims/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct RawRow { pub user: UserId, pub work: WorkId, pub raw: u128 }`
  - `pub fn allocate_from_raw(tier_fees: &BTreeMap<UserId, u128>, rows: &[RawRow], bandwidth_ppm: &BTreeMap<WorkId, u64>) -> Result<Payouts, DaprError>` — the apportionment/bandwidth/reputation pass that today lives inside `allocate`, taking pre-computed per-row `raw`.
  - `allocate` delegates: compute each row's `raw` then call `allocate_from_raw`.

- [ ] **Step 1: Write the failing test** — `allocate_from_raw` with hand-computed raws equals `allocate` on the equivalent `UsageRow`s:

```rust
#[test]
fn allocate_from_raw_matches_allocate() {
    let d = ds(&[("u1", 1_000_000)], &[
        ("u1", "wA", 60, 1_000_000, 1_000_000, 1),
        ("u1", "wB", 20, 1_000_000, 1_000_000, 1),
    ]);
    let want = allocate(&d, &DaprParams::default()).unwrap();
    // raw = minutes·price·region·D(plays)/1e6; here plays=1 → D=1e6 → raw=value.
    let rows = vec![
        RawRow { user: "u1".into(), work: "wA".into(), raw: 60u128 * 1_000_000 * 1_000_000 },
        RawRow { user: "u1".into(), work: "wB".into(), raw: 20u128 * 1_000_000 * 1_000_000 },
    ];
    let got = allocate_from_raw(&d.tier_fees, &rows, &d.bandwidth_ppm).unwrap();
    assert_eq!(want.per_work, got.per_work);
    assert_eq!(want.unallocated, got.unallocated);
}
```

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-dapr allocate_from_raw_matches_allocate`. Expected: FAIL.

- [ ] **Step 3: Refactor.** Extract the body of `allocate` after per-row `raw` computation into `allocate_from_raw` (grouping by user, `cred = raw·bw`, `rw_u`, `target`, `apportion`, reputation). `allocate` builds `Vec<RawRow>` (computing `raw` via existing `value()`·`d_ppm`) and calls it. Keep all existing tests passing (they exercise `allocate`).

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-dapr`. Expected: PASS (all, including the H3 suite).

- [ ] **Step 5: Commit.**

```bash
git add sims/src/lib.rs
git commit -m "dapr: expose allocate_from_raw so settlement can pay from proven weights"
```

### Task 18: Settlement — dual-mode (keep disclosure for legacy; add proven-weights event path)

> **Scope revised (2026-07-25, Roland's decision):** the disclosure file is NOT deleted. Settlement gains a SECOND mode that pays from proven event weights (the ZK path), while the existing disclosure mode stays for the four legacy demos (which keep AcceptAllVerifier). Mode is chosen at runtime: `DISCLOSURE` env set → disclosure mode (legacy); unset → event-weights mode (ZK). This keeps every demo green while the real ZK settlement path ships.

**Files:**
- Modify: `services/settlement/src/settle.rs`, `src/chain.rs`, `src/config.rs`, `src/lib.rs`
- Keep: `services/settlement/src/disclosure.rs` (retained; annotate its doc as the legacy path)
- Test: `settle.rs`

**Interfaces:**
- Consumes: `cwe_dapr::{RawRow, allocate_from_raw}`, the extended `ConsumptionSubmitted` event, `CWEEpochBeacon.keyFor`, `cwe_zk_circuits::poseidon::digest` + `PublicRow`.
- Produces: a `settle_raw(epoch, tier_fees, rows: &[RawRow], bandwidth_ppm, escrow_works) -> Settlement` alongside the existing `settle(...)`; both share one payouts→Merkle/escrow tail.

- [ ] **Step 1: Write the failing test** in `settle.rs` — add `settle_raw` and a test that it conserves fees and its proofs verify (build `RawRow`s directly), mirroring `settle_conserves_and_proofs_verify`. Keep the existing `settle` tests unchanged (disclosure path still supported).

- [ ] **Step 2: Run to verify it fails.** Run: `cargo test -p cwe-settlement settle::`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - `settle.rs`: extract the current payouts→partition→Merkle/escrow tail of `settle` into a private helper `fn finalize(epoch, payouts: Payouts, escrow_works) -> Result<Settlement, SettleError>`. Keep `settle(epoch, dataset, escrow_works)` (calls `allocate` then `finalize`). Add `pub fn settle_raw(epoch, tier_fees: &BTreeMap<String,u128>, rows: &[RawRow], bandwidth_ppm: &BTreeMap<String,u64>, escrow_works) -> Result<Settlement, SettleError>` (calls `allocate_from_raw` then `finalize`). Both share `finalize`.
  - `chain.rs`: update the `sol!` `ConsumptionSubmitted` event to the new shape `(address user, uint256 epoch, bytes32 tierId, bytes32 digest, bytes32[] pseudonyms, bytes32[] workIds, uint256[] weights)`. Branch on mode:
    - **Disclosure mode** (`cfg.disclosure_path.is_some()`): read submitting `(user, epoch)` from events; load that user's `Opening`s from the disclosure file; build the `Dataset` as before; `escrow_works` from `disclosure.escrow_works`; call `settle`. NOTE: the event no longer carries `commitments`, so the old commitment-vs-event recompute is dropped — document this as a legacy-mode simplification (disclosure mode uses AcceptAllVerifier and is not the integrity path).
    - **Event mode** (no disclosure path): for each submission decode `(pseudonyms, workIds, weights, digest)`; read `K_epoch = beacon.keyFor(epoch)`; **recompute the digest via `cwe_zk_circuits`** over the active `PublicRow`s padded to `MAX_WORKS` and **reject any submission whose recomputed digest ≠ the event digest** (the off-chain half of the trust chain); build `RawRow{user, work=workId hex, raw=weight}`; read tier fees from `Tiers.feeOf`; `escrow_works` = **empty for cycle-1** (all direct — registry-derived escrow-tier routing in event mode is a documented follow-on); call `settle_raw`.
    - Add a small `sol!` binding for `CWEEpochBeacon { function keyFor(uint256) returns (bytes32); }`. For padding to `MAX_WORKS`, add (or reuse) a helper `cwe_zk_circuits::poseidon::digest_from_active(epoch, tier, k_epoch, &active) -> [u8;32]` that pads with canonical padded rows internally (add this small helper to `zk-circuits` if not present — it centralizes the padding convention; erroring if `active.len() > MAX_WORKS`).
  - `config.rs`: make `disclosure_path: Option<PathBuf>` (set from `DISCLOSURE` if present); add `beacon: String` to `Deployments` (needed for `keyFor` in event mode).
  - `lib.rs`: keep `pub mod disclosure;`.
  - Add `cwe-zk-circuits` as a dep of `services/settlement` (native).

- [ ] **Step 4: Run to verify it passes.** Run: `cargo test -p cwe-settlement`. Expected: PASS (both `settle` and `settle_raw` tests). `chain.rs` event-mode is validated end-to-end by the zk-demo (Task 20); disclosure-mode by the legacy demos (Task 19).

- [ ] **Step 5: Commit.**

```bash
git add services/settlement Cargo.toml libs/zk-circuits
git commit -m "settlement: dual-mode — keep disclosure path, add proven-weights event path"
```

---

## Phase 5 — clients, demo, CI, docs

### Task 19: Configurable verifier + migrate legacy demos/clients to the new signature

> **Scope revised (2026-07-25, Roland's decision):** the four legacy usage-submitting demos (`demo`, `ownership`, `player`, `arbitration`) keep AcceptAllVerifier + their disclosure flow, but the `submitConsumption` selector changed to 7 args for everyone, so their on-chain submit calls and the player client must be updated to the new signature (with dummy ZK fields + empty proof, which AcceptAllVerifier accepts). This restores those four demos to green. Real proof generation is NOT added to the player here — the zk-demo uses a dedicated tool (Task 20).

**Files:**
- Modify: `chain/script/Deploy.s.sol`; `ops/demo/run_demo.sh`, `run_ownership_demo.sh`, `run_player_demo.sh`, `run_arbitration_demo.sh`; `clients/player-plugin/src/settle.rs`; `clients/browser-ext/core/*` (submit shim + stale doc)

**Interfaces:**
- Produces: `make demo/ownership-demo/player-demo/arbitration-demo` pass again; `Deploy.s.sol` picks the verifier via `VERIFIER` env (default `groth16`).

- [ ] **Step 1: Deploy `VERIFIER` selector.** In `Deploy.s.sol`, read `string verifierKind = vm.envOr("VERIFIER", string("groth16"))`; if it equals `"accept-all"`, `d.verifier = address(new AcceptAllVerifier())` (re-import it), else `new Groth16Verifier()`. Default (unset) stays `groth16`. `forge build` must compile.

- [ ] **Step 2: Migrate the four legacy demo scripts.** In each of `run_demo.sh`, `run_ownership_demo.sh`, `run_player_demo.sh`, `run_arbitration_demo.sh`: (a) export `VERIFIER=accept-all` before the `forge script Deploy` step; (b) change every `submitConsumption(bytes32,bytes32[],bytes)` `cast send` to the 7-arg form `submitConsumption(bytes32,bytes32[],bytes32[],bytes32[],uint256[],bytes32,bytes)` — pass the existing commitments array for `commitments`, and for `pseudonyms`/`workIds` pass equal-length dummy arrays (reuse the commitments array is fine), `weights` an equal-length `uint256[]` of any values (e.g. `[1,...]`), `digest` `0x00..00`, `proof` `0x`. These are ignored (disclosure mode + AcceptAllVerifier), but arities must match or `submitConsumption` reverts `ArityMismatch`. Keep the disclosure-writing steps unchanged.

- [ ] **Step 3: Update the player client + browser-ext submit call to the 7-arg signature.** `clients/player-plugin/src/settle.rs`: update the `sol!` `submitConsumption` binding to the 7-arg signature and the call site to pass `commitments` + equal-length dummy `pseudonyms`/`workIds`/`weights` + zero `digest` + empty `proof` (keeps player_demo green with AcceptAllVerifier; the player still writes its disclosure). Poseidon commitments already come via `wallet-zk` (Task 16). Browser-ext core: if it references the old submit signature, update it likewise; fix the stale "keccak256 commitment" doc comment (the commitment is Poseidon now — its separate `content_hash` keccak use is unrelated and stays). Do NOT add the prover to either client.

- [ ] **Step 4: Verify the wasm build.** Run the repo's existing browser-ext wasm build (e.g. `cd clients/browser-ext/core && cargo build --target wasm32-unknown-unknown`, or `wasm-pack build` per its README). If it FAILS because `wallet-zk → cwe-zk-circuits` pulls arkworks into wasm, apply the deferred fix: feature-gate `cwe-zk-circuits`'s `circuit`/`prove`/`setup` modules behind a default `prover` feature and have `wallet-zk` depend on it with `default-features = false` (so wasm gets only `poseidon`/`field`/`dr`), and/or add `getrandom` with the `js` feature to the browser-ext crate. Document exactly what was needed.

- [ ] **Step 5: Run the four legacy demos + player tests.** Run: `for t in demo ownership-demo player-demo arbitration-demo; do make -C ops $t || exit 1; done` and `cargo test -p cwe-player`. Expected: all green.

- [ ] **Step 6: Commit.**

```bash
git add chain/script/Deploy.s.sol ops/demo clients
git commit -m "chain/clients/demos: VERIFIER selector; migrate legacy submit calls to the 7-arg signature"
```

### Task 20: `make zk-demo` — real proof path, three acts

> **Scope (per the 2026-07-25 decision):** the zk-demo deploys with the default `VERIFIER=groth16`, submits a REAL proof via a dedicated tool, and settles in EVENT mode (no disclosure file). A small submitter bin lives in `services/settlement` (it already has both alloy and, via Task 18, `cwe-zk-circuits`).

**Files:**
- Create: `services/settlement/src/bin/zk_submit.rs`, `ops/zk_demo.sh`
- Modify: `ops/Makefile`

**Interfaces:**
- Produces: `make -C ops zk-demo` running three acts on a self-contained Anvil.

- [ ] **Step 1: Write `zk_submit.rs`** — a CLI that: loads the devnet PK (`chain/zk/proving_key.bin`); reads the deployments JSON for `consumption`/`beacon`; reads `K_epoch = beacon.keyFor(currentEpoch)` (set by the demo); builds `UsageRowInput`(s) from args/env (work_id, minutes, plays, price, region); `prove`s; and submits the 7-arg `submitConsumption(commitments, pseudonyms, workIds, weights, digest, proof)` where `commitments` are the bundle's per-row commitments, `pseudonyms`/`workIds`/`weights` come from `bundle.rows`, `digest` = `bundle.digest`, `proof` = `bundle.proof`. Flags: `--mode honest|tamper-digest|row-split`. `tamper-digest` flips a byte of the submitted `digest` before sending (on-chain verify must then revert). `row-split` builds two `UsageRowInput`s with the SAME `work_id` and expects `prove` to return `Err` (uniqueness) — print the error and exit non-zero WITHOUT submitting.

- [ ] **Step 2: Write `ops/zk_demo.sh`** (model on existing demo scripts; self-contained Anvil; kill only the exact Anvil PID you started — never pattern-kill). Steps:
  1. Start Anvil; `VERIFIER=groth16 forge script Deploy` (default, but set explicitly); read `deployments/localhost.json`.
  2. As owner, `cast send beacon setKey(uint256,bytes32)` for the current epoch (a fixed non-zero key).
  3. Register a work + tier + fund a subscription so there's a fee to distribute (reuse the pattern from `run_demo.sh`).
  4. **Act 1 (honest):** `cargo run -p cwe-settlement --release --bin zk_submit -- --mode honest`; assert the tx succeeds and `ConsumptionSubmitted` carries the weights; run settlement in EVENT mode (no `DISCLOSURE` env, pass `RPC_URL`/`PRIVATE_KEY`/`EPOCH`/deployments incl. `beacon`); assert a creator is paid and fees conserve. Print `ACT 1 OK`.
  5. **Act 2 (inflation):** `zk_submit --mode tamper-digest`; assert the submit **reverts** (`ProofRejected`). Print `ACT 2 OK (rejected)`.
  6. **Act 3 (row-split):** `zk_submit --mode row-split`; assert the tool exits non-zero because `prove` refused (uniqueness). Print `ACT 3 OK (rejected)`.
  7. Exit non-zero on any failed assertion.

- [ ] **Step 3: Add the Makefile target.**

```makefile
zk-demo: ## Run the ZK usage-proof end-to-end demo (self-contained Anvil)
	./zk_demo.sh
```

Add `zk-demo` to the `.PHONY` line and the `help` list.

- [ ] **Step 4: Run it.** Run: `make -C ops zk-demo`. Expected: all three acts print OK; exit 0. (First run builds the release prover; proving is ~5s.)

- [ ] **Step 5: Commit.**

```bash
git add services/settlement/src/bin/zk_submit.rs ops/Makefile ops/zk_demo.sh
git commit -m "ops: make zk-demo — real proof honest-pays, inflation and row-split rejected"
```

### Task 21: CI `zk-e2e` job

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add a `zk-e2e` job** mirroring the existing `*-e2e` jobs (checkout, Rust + Foundry setup, `jq`, the wasm target if the browser-ext build runs in CI), running `make -C ops zk-demo`. Match the naming/structure of `player-e2e`/`antifraud-e2e`.

- [ ] **Step 2: Validate the workflow YAML.** Run: `PATH="$HOME/.foundry/bin:$PATH" python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))"`. Expected: parses.

- [ ] **Step 3: Commit.**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add zk-e2e job running make zk-demo"
```

### Task 22: Full gate + roadmap/project-map sync

**Files:**
- Modify: `ROADMAP.md`, `docs/roadmap.md`, `project-map.js`

- [ ] **Step 1: Run the full gate.**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd chain && PATH="$HOME/.foundry/bin:$PATH" forge test && cd ..
for t in demo hub-demo ownership-demo player-demo arbitration-demo antifraud-demo identity-demo zk-demo; do make -C ops $t || exit 1; done
```

Expected: all green. Fix anything red before proceeding (do not edit the roadmap until green).

- [ ] **Step 2: Update `docs/roadmap.md`** — flip H2 to done-for-cycle-1 with the deferred sub-items listed (work-blinding, manifest-signature, tier-eligibility, cross-epoch unlinkability, real beacon, **migrating the four legacy demos + player off the disclosure path onto real proofs**); update the "What is real vs stubbed" *Usage proofs* row to "real Groth16 integrity proof + within-epoch dedup, verified on-chain (`Groth16Verifier`); settlement pays from proven event weights (event mode). Disclosure file retained as the legacy path for the pre-ZK demos (dual-mode)"; bump the status date. Mirror the high-level line in `ROADMAP.md`.

- [ ] **Step 3: Update `project-map.js`** — set the H2 node `status`/`desc`/`parts` (new `CWEEpochBeacon`, `Groth16Verifier`, `zk-circuits`, dual-mode settlement), add/adjust the `roadmap[]` entry, and set `project.updated` to today's date. Keep it factual. Open `file:///home/roland/git/clean-web-economy/project-map.html` to confirm it renders and the stats recompute.

- [ ] **Step 4: Commit.**

```bash
git add ROADMAP.md docs/roadmap.md project-map.js
git commit -m "Roadmap + project map: H2 cycle 1 (ZK usage proofs) complete"
```

---

## Self-review (author checklist — completed)

**Spec coverage:** §2 digest+on-chain verify → Tasks 8,10,11,12,14; §3 Poseidon/pseudonym/beacon/arity → Tasks 3,13,16,6; §4 seven constraints → Tasks 6 (commit, range, well-formed, tier-bind via public `tier` in digest), 7 (DR-cap, epoch-binding), 8 (uniqueness, digest); §4.4 DR split → Tasks 4,7,17; §5 settlement/disclosure/escrow → Task 18; §6 contracts → Tasks 12,13,14,15; §7 trusted setup → Task 9; §8 demo+tests → Tasks 5–12,20,21; §9 roadmap sync → Task 22.

**Deferrals honored:** work-blinding, manifest-signature, tier-eligibility, cross-epoch unlinkability, real beacon, in-wasm extension proving — none implemented; each noted in code/docs where its seam lives.

**Type consistency:** `ProofBundle`/`PublicRow`/`UsageRowInput` (prove.rs), `digest(epoch,tier,k_epoch,rows)` (poseidon.rs), `weight_of`/`d_ppm_table` (dr.rs), `RawRow`/`allocate_from_raw` (sims), `verify(bytes32 digest, bytes proof)` (Solidity + Rust) — used consistently across tasks.

**Known open item for the implementer:** the exact arkworks↔EVM proof-point encoding (Task 11) is the highest-risk step; the `export_keys` round-trip assertion against Rust `verify` plus the forge fixture test (Task 12) are the guardrails that must pass before the demo.
