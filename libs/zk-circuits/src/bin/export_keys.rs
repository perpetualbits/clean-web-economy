//! Devnet key + proof-fixture exporter for the on-chain Groth16 verifier.
//!
//! Runs the (insecure, devnet-only) trusted setup exactly once, saves the
//! proving/verifying keys under `chain/zk/`, produces one known-good proof over
//! a fixed sample usage row, sanity-checks it with the Rust verifier, and then
//! re-encodes the proof's curve points into the exact `uint256` layout the
//! generated `Groth16Verifier.sol` (see `gen_verifier.rs`) and the EVM
//! `alt_bn128` pairing precompile expect — writing them to
//! `chain/test/fixtures/zk_proof.json`.
//!
//! **The encoding here MUST stay identical to `gen_verifier.rs`'s VK encoding.**
//! Both convert a field element to its canonical (non-Montgomery) big-endian
//! integer, and both reverse the `Fq2` limb order for G2 points
//! (`[c1, c0]` — imaginary part first) as the precompile requires. That shared
//! convention is the whole point of this task.

use ark_bn254::{Bn254, Fq, G1Affine, G2Affine};
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::Proof;
use ark_serialize::CanonicalDeserialize;

use cwe_zk_circuits::prove::{prove, verify, UsageRowInput};
use cwe_zk_circuits::setup::{devnet_setup, save_pk, save_vk};

/// Encode one base-field element `Fq` as a 32-byte big-endian, `0x`-prefixed
/// hex string — the canonical integer value the EVM interprets as a `uint256`.
///
/// arkworks stores field elements in Montgomery form internally;
/// `into_bigint()` converts back to the canonical integer, and `to_bytes_be()`
/// renders that as big-endian bytes. The result is left-padded to a full 32
/// bytes so every literal is a fixed-width word.
fn fq_hex(f: &Fq) -> String {
    let be = f.into_bigint().to_bytes_be(); // canonical (non-Montgomery) big-endian
    let mut buf = [0u8; 32];
    buf[32 - be.len()..].copy_from_slice(&be); // left-pad to a full word
    format!("0x{}", hex::encode(buf))
}

/// Encode a G1 point as its affine `(x, y)` pair of `uint256` hex strings.
///
/// Both coordinates are plain base-field elements, so each goes through
/// [`fq_hex`] unchanged. The devnet proof points are never the point at
/// infinity, so that degenerate case is not represented here.
fn g1_hex(p: &G1Affine) -> [String; 2] {
    [fq_hex(&p.x), fq_hex(&p.y)]
}

/// Encode a G2 point in the EVM precompile's expected layout:
/// `[[x.c1, x.c0], [y.c1, y.c0]]`.
///
/// A G2 coordinate is an `Fq2` value `c0 + c1·u`. The `alt_bn128` pairing
/// precompile expects each `Fq2` with its imaginary limb (`c1`) FIRST — the
/// reverse of arkworks' `(c0, c1)` field order. Getting this reversal wrong is
/// the classic BN254-on-EVM bug, so it is applied here identically to how
/// `gen_verifier.rs` encodes the verifying key's G2 elements.
fn g2_hex(p: &G2Affine) -> [[String; 2]; 2] {
    [
        [fq_hex(&p.x.c1), fq_hex(&p.x.c0)], // x: imaginary limb first
        [fq_hex(&p.y.c1), fq_hex(&p.y.c0)], // y: imaginary limb first
    ]
}

/// Run the devnet setup, save the keys, prove a fixed sample row, verify it in
/// Rust, then export the re-encoded proof + digest fixture as JSON.
fn main() {
    // Deterministic devnet trusted setup — call ONCE (it is the slow step).
    println!("export_keys: running devnet_setup (deterministic, ~85s)...");
    let (pk, vk) = devnet_setup();

    // Persist both keys under chain/zk/ (creating the directory if missing).
    std::fs::create_dir_all("chain/zk").expect("create chain/zk");
    save_pk(&pk, "chain/zk/proving_key.bin").expect("save proving key");
    save_vk(&vk, "chain/zk/verifying_key.bin").expect("save verifying key");
    println!("export_keys: saved chain/zk/{{proving_key,verifying_key}}.bin");

    // A single fixed sample usage row — the known-good statement to prove.
    let input = UsageRowInput {
        work_id: [1u8; 32],
        minutes: 60,
        plays: 2,
        salt: [9u8; 32],
        price_ppm: 1_000_000,
        region_ppm: 1_000_000,
    };
    // Fixed public context; `k_ppm` (the DR curvature) is the 5th argument.
    let epoch: u64 = 7;
    let tier: [u8; 32] = [0xABu8; 32];
    let k_epoch: [u8; 32] = [0x5cu8; 32];
    let bundle = prove(&pk, epoch, &tier, &k_epoch, 1_000_000, &[input]).expect("prove");

    // Sanity: the proof must verify in Rust before we trust any re-encoding of
    // its points for the EVM. A failure here means the fixture would be junk.
    assert!(
        verify(&vk, &bundle.digest, &bundle.proof),
        "Rust-side verify of the freshly generated proof must pass"
    );
    println!("export_keys: Rust verify assertion PASSED");

    // Re-decode the compressed proof bytes into the actual curve points so we
    // can re-encode them ourselves in the EVM layout.
    let proof = Proof::<Bn254>::deserialize_compressed(bundle.proof.as_slice())
        .expect("re-decode compressed proof");

    let a = g1_hex(&proof.a);
    let b = g2_hex(&proof.b);
    let c = g1_hex(&proof.c);

    // The single public input, as bytes32 hex. `bad_digest` flips the first
    // byte so it is a definitely-different value the verifier must reject.
    let digest_hex = format!("0x{}", hex::encode(bundle.digest));
    let mut bad = bundle.digest;
    bad[0] ^= 0xff; // flip the top byte
    let bad_digest_hex = format!("0x{}", hex::encode(bad));

    // The first active row's DR-capped weight, carried for cross-checks.
    let weight0 = bundle.rows[0].weight;

    // Hand-assemble the JSON (the crate has no serde_json dependency). The
    // schema is documented in the generated verifier and the task report.
    let json = format!(
        "{{\n  \"digest\": \"{digest}\",\n  \"bad_digest\": \"{bad}\",\n  \"weight0\": \"{weight}\",\n  \"a\": [\"{ax}\", \"{ay}\"],\n  \"b\": [[\"{bx1}\", \"{bx0}\"], [\"{by1}\", \"{by0}\"]],\n  \"c\": [\"{cx}\", \"{cy}\"]\n}}\n",
        digest = digest_hex,
        bad = bad_digest_hex,
        weight = weight0,
        ax = a[0],
        ay = a[1],
        bx1 = b[0][0],
        bx0 = b[0][1],
        by1 = b[1][0],
        by0 = b[1][1],
        cx = c[0],
        cy = c[1],
    );

    std::fs::create_dir_all("chain/test/fixtures").expect("create chain/test/fixtures");
    std::fs::write("chain/test/fixtures/zk_proof.json", &json).expect("write fixture");
    println!("export_keys: wrote chain/test/fixtures/zk_proof.json");

    // Print everything so the run is auditable from the log alone.
    println!("--- fixture (EVM encoding) ---");
    print!("{json}");
}
