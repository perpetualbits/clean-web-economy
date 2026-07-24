//! Trusted-setup key generation and (de)serialisation for the Groth16 proving/verifying keys.
//!
//! **The keys produced by [`devnet_setup`] are INSECURE and DEVNET-ONLY.** They
//! come from a fixed, publicly known RNG seed, so the setup's "toxic waste"
//! (the secret randomness that must normally be destroyed) is trivially known
//! to anyone who reads this file — a malicious prover who has it can forge
//! proofs for any statement. This is acceptable for local development and
//! integration tests, where reproducibility matters more than soundness, but
//! **a production deployment requires a real multi-party computation (MPC)
//! ceremony**, where no single participant ever learns the full toxic waste.

use std::io;
use std::path::Path;

use ark_bn254::Bn254;
use ark_groth16::{Groth16, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::rand::rngs::StdRng;
use ark_std::rand::SeedableRng;

use crate::circuit::{RowWitness, UsageCircuit};
use crate::poseidon::{
    commitment as native_commitment, digest as native_digest, pseudonym as native_pseudonym,
};
use crate::prove::PublicRow;
use crate::MAX_WORKS;

/// The fixed, all-zero seed the devnet setup's RNG is derived from. Anyone can
/// reproduce this seed, which is precisely why the resulting keys are
/// insecure: the setup's randomness (and hence its toxic waste) is public.
const DEVNET_SEED: [u8; 32] = [0u8; 32];

/// The fixed epoch the devnet setup's placeholder circuit is built for. The
/// value is arbitrary — the setup only cares about the circuit's *shape*
/// (row count, constraint structure), not the specific witness — but it must
/// be held fixed so repeated calls build byte-identical circuits.
const DEVNET_EPOCH: u64 = 0;
/// The fixed tier identifier the devnet setup's placeholder circuit uses.
const DEVNET_TIER: [u8; 32] = [0u8; 32];
/// The fixed per-epoch key the devnet setup's placeholder circuit uses to
/// derive every padding row's pseudonym.
const DEVNET_K_EPOCH: [u8; 32] = [0u8; 32];
/// The fixed DR curvature the devnet setup's placeholder circuit carries. It
/// has no effect on an all-inactive circuit (no row's DR-cap gadget fires),
/// but `UsageCircuit` still requires a value.
const DEVNET_K_PPM: u64 = 1_000_000;

/// The canonical commitment of the all-zero opening — the value every
/// inactive (padding) row carries. Mirrors `circuit::padded_commitment`
/// (private to that module and test-only downstream of it), recomputed here
/// via the same native helper so the two never drift apart.
fn padded_commitment() -> [u8; 32] {
    native_commitment(&[0u8; 32], 0, 0, &[0u8; 32])
}

/// The canonical pseudonym of a padding row under the fixed `DEVNET_K_EPOCH`:
/// `Poseidon(k_epoch, padded_commitment)`. Mirrors `circuit::padded_pseudonym`,
/// which is `#[cfg(test)]`-only in that module, so it is recomputed here from
/// the public native helper rather than depended on across the test boundary.
fn padded_pseudonym() -> [u8; 32] {
    native_pseudonym(&DEVNET_K_EPOCH, &padded_commitment())
}

/// Build the all-inactive `UsageCircuit` the devnet trusted setup runs over.
///
/// The circuit has exactly `MAX_WORKS` canonical padding rows (no real usage),
/// so the resulting proving/verifying keys are unconditionally shape-correct
/// for the real circuit (Groth16 keys are keyed to a circuit's constraint
/// *shape*, not its witness values) while remaining satisfiable — a
/// requirement for `circuit_specific_setup`, which synthesises the circuit
/// during setup to size and structure the keys. The `digest` field is computed
/// natively over the same padded rows so `generate_constraints`'s in-circuit
/// recomputation matches it exactly; a mismatch here would make the circuit
/// unsatisfiable and setup would produce keys nobody could ever prove against.
fn empty_circuit() -> UsageCircuit {
    let commitment = padded_commitment();
    let pseudonym = padded_pseudonym();
    let row = RowWitness {
        active: false,
        work_id: [0u8; 32],
        minutes: 0,
        plays: 0,
        salt: [0u8; 32],
        price_ppm: 0,
        region_ppm: 0,
        weight: 0,
        commitment,
        pseudonym,
    };
    // Every row is the identical canonical padding row: `MAX_WORKS` copies.
    let rows: Vec<RowWitness> = (0..MAX_WORKS).map(|_| row.clone()).collect();

    // Project to the public form the native digest hashes, in row order, so
    // the native digest below matches what `generate_constraints` recomputes.
    let public_rows: Vec<PublicRow> = rows
        .iter()
        .map(|r| PublicRow {
            work_id: r.work_id,
            commitment: r.commitment,
            pseudonym: r.pseudonym,
            weight: r.weight,
        })
        .collect();
    let digest = native_digest(DEVNET_EPOCH, &DEVNET_TIER, &DEVNET_K_EPOCH, &public_rows);

    UsageCircuit {
        epoch: DEVNET_EPOCH,
        tier: DEVNET_TIER,
        k_epoch: DEVNET_K_EPOCH,
        k_ppm: DEVNET_K_PPM,
        rows,
        digest,
    }
}

/// Run the (insecure, devnet-only) Groth16 trusted setup and return the
/// resulting `(proving key, verifying key)` pair.
///
/// Uses a deterministic `StdRng` seeded from the fixed all-zero `DEVNET_SEED`,
/// so every call in every process produces byte-identical keys — essential
/// for a devnet where the same verifying key must be baked into on-chain
/// verifier contracts and CI fixtures alike without shipping a key file around.
/// The circuit synthesised is [`empty_circuit`]: an all-inactive `UsageCircuit`
/// with the real circuit's exact shape.
///
/// # Panics
/// Panics if `circuit_specific_setup` fails. This can only happen if
/// `empty_circuit` stops being satisfiable (e.g. a future change to the
/// digest/padding rules without updating this module to match) — a bug to
/// fix, not a runtime condition callers should handle.
pub fn devnet_setup() -> (ProvingKey<Bn254>, VerifyingKey<Bn254>) {
    let mut rng = StdRng::from_seed(DEVNET_SEED);
    Groth16::<Bn254>::circuit_specific_setup(empty_circuit(), &mut rng)
        .expect("devnet_setup: empty_circuit must be satisfiable")
}

/// Write `pk` to `path` in `ark-serialize`'s compressed encoding.
///
/// Compressed encoding is used (rather than uncompressed) purely to keep the
/// key files small; it costs a small amount of extra decompression work on
/// load, which is negligible next to proof generation itself. `ark-serialize`
/// targets `no_std` and so defines its own `Read`/`Write` traits rather than
/// using `std::io`'s, which `std::fs::File` does not implement; serialising
/// to an in-memory buffer first (which does implement them) and writing that
/// buffer out with `std::fs::write` sidesteps the mismatch.
pub fn save_pk(pk: &ProvingKey<Bn254>, path: impl AsRef<Path>) -> io::Result<()> {
    let mut buf = Vec::new();
    pk.serialize_compressed(&mut buf)
        .map_err(|e| io::Error::other(e.to_string()))?;
    std::fs::write(path, buf)
}

/// Read a proving key previously written by [`save_pk`] from `path`.
pub fn load_pk(path: impl AsRef<Path>) -> io::Result<ProvingKey<Bn254>> {
    let buf = std::fs::read(path)?;
    ProvingKey::deserialize_compressed(buf.as_slice()).map_err(|e| io::Error::other(e.to_string()))
}

/// Write `vk` to `path` in `ark-serialize`'s compressed encoding. See
/// [`save_pk`] for why this goes through an in-memory buffer.
pub fn save_vk(vk: &VerifyingKey<Bn254>, path: impl AsRef<Path>) -> io::Result<()> {
    let mut buf = Vec::new();
    vk.serialize_compressed(&mut buf)
        .map_err(|e| io::Error::other(e.to_string()))?;
    std::fs::write(path, buf)
}

/// Read a verifying key previously written by [`save_vk`] from `path`.
pub fn load_vk(path: impl AsRef<Path>) -> io::Result<VerifyingKey<Bn254>> {
    let buf = std::fs::read(path)?;
    VerifyingKey::deserialize_compressed(buf.as_slice())
        .map_err(|e| io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed-seed devnet setup is fully reproducible: two independent
    /// calls must yield byte-identical verifying keys.
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

    /// A proving/verifying key saved to disk and reloaded round-trips to the
    /// same bytes, confirming the save/load pair agrees on encoding.
    #[test]
    fn save_and_load_round_trip() {
        let (pk, vk) = devnet_setup();
        let dir = std::env::temp_dir().join(format!("cwe-zk-setup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pk_path = dir.join("pk.bin");
        let vk_path = dir.join("vk.bin");

        save_pk(&pk, &pk_path).unwrap();
        save_vk(&vk, &vk_path).unwrap();
        let loaded_pk = load_pk(&pk_path).unwrap();
        let loaded_vk = load_vk(&vk_path).unwrap();

        let mut a = Vec::new();
        let mut b = Vec::new();
        pk.serialize_compressed(&mut a).unwrap();
        loaded_pk.serialize_compressed(&mut b).unwrap();
        assert_eq!(a, b, "proving key must round-trip through save/load");

        let mut c = Vec::new();
        let mut d = Vec::new();
        vk.serialize_compressed(&mut c).unwrap();
        loaded_vk.serialize_compressed(&mut d).unwrap();
        assert_eq!(c, d, "verifying key must round-trip through save/load");

        std::fs::remove_dir_all(&dir).ok();
    }
}
