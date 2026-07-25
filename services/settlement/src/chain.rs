//! The chain layer: reads submissions and registry/tier data over RPC, and
//! commits the settled epoch root on-chain.
//!
//! This is where the concrete Ethereum stack (alloy) lives, kept apart from the
//! pure [`crate::settle`] logic so the latter stays trivially testable. The live
//! behaviour of this module is exercised end to end by the demos: the legacy
//! demos drive DISCLOSURE mode; the zk-demo drives EVENT mode.
//!
//! # Two settlement modes
//!
//! Settlement runs in one of two modes, chosen at runtime by
//! [`crate::config::Config::disclosure_path`]:
//!
//! * **Disclosure mode** (`disclosure_path.is_some()`): the legacy path used by
//!   the four legacy demos (which keep AcceptAllVerifier on-chain). Usage is
//!   opened out-of-band via a disclosure file; settlement runs the full DAPR
//!   math over the opened minutes/plays/price/region. This is NOT the integrity
//!   path — it does not re-verify anything cryptographically (see the note in
//!   [`run_disclosure`]).
//! * **Event mode** (`disclosure_path.is_none()`): the ZK path. Each submission
//!   carries proven per-work weights and a Poseidon digest in its event; the
//!   settlement job recomputes that digest from the event's own rows and rejects
//!   any submission whose recomputed digest does not match — the off-chain half
//!   of the trust chain — then pays from the proven weights via
//!   [`crate::settle::settle_raw`].

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::str::FromStr;

use alloy::network::TransactionBuilder;
use alloy::primitives::{keccak256, Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{Filter, TransactionRequest};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolEvent;

use cwe_dapr::{Dataset, RawRow, UsageRow};
use cwe_receipt::{normalize_addr, ReceiptBundle};
use cwe_wallet_zk::Bytes32;
use cwe_zk_circuits::prove::{digest_from_active, PublicRow};

use crate::config::Config;
use crate::disclosure::Disclosure;
use crate::receipts::{accept_receipts, row_credibility_ppm};
use crate::settle::{settle, settle_raw, settle_raw_with_row_credibility, Settlement};

// Minimal on-chain interfaces the settlement job touches. `#[sol(rpc)]` generates
// typed contract bindings (constructors, call builders, event decoders).
sol! {
    #[sol(rpc)]
    contract Tiers {
        function feeOf(bytes32 tierId) external view returns (uint256);
    }
    #[sol(rpc)]
    contract Registry {
        function pricePerMinOf(bytes32 workId) external view returns (uint256);
    }
    #[sol(rpc)]
    contract Consumption {
        event ConsumptionSubmitted(
            address indexed user,
            uint256 indexed epoch,
            bytes32 tierId,
            bytes32 digest,
            bytes32[] pseudonyms,
            bytes32[] workIds,
            uint256[] weights
        );
    }
    #[sol(rpc)]
    contract Beacon {
        function keyFor(uint256 epoch) external view returns (bytes32);
    }
    #[sol(rpc)]
    contract Payouts {
        function commitEpoch(uint256 epochId, bytes32 merkleRoot, uint256 totalCredits) external;
    }
    #[sol(rpc)]
    contract Escrow {
        function commit(uint256 epochId, bytes32 workId, uint256 amount) external;
    }
    /// Minimal `CWEIdentity` view used to check storage-node credentials.
    #[sol(rpc)]
    interface Identity {
        function isValid(address subject, bytes32 credType) external view returns (bool);
    }
}

/// A boxed error alias keeping the orchestration signature readable.
type BoxErr = Box<dyn Error + Send + Sync>;

/// Recompute the `STORAGE_NODE` credential type identically to the on-chain
/// `CredentialTypes.STORAGE_NODE` Solidity constant (`chain/contracts/CredentialTypes.sol`).
///
/// Both sides derive this the same way — `keccak256("cwe.credential.storage-node")`
/// — but they are two independent implementations in two different languages,
/// with nothing at compile time forcing them to agree. If this value ever
/// silently drifted from the Solidity constant, `Identity::isValid` would check
/// a credential type that no attestation on-chain ever holds, EVERY bandwidth
/// receipt would stop counting, and settlement would keep running with no error
/// — it would just quietly stop discounting fraud. `cred_type_matches_solidity_constant`
/// below pins this value against the constant as independently verified with
/// `cast keccak "cwe.credential.storage-node"`, so that drift cannot go unnoticed.
fn storage_node_credential_type() -> B256 {
    keccak256(b"cwe.credential.storage-node")
}

/// Run a full settlement against the configured chain and write the proofs file.
///
/// Connects, computes the [`Settlement`] via the mode selected by
/// [`Config::disclosure_path`] (see the module docs), commits the direct root on
/// `CWEPayouts`, routes any escrowed credit to `CWEEscrow`, and persists the
/// withdrawal proofs. The compute step differs by mode; the commit/escrow/write
/// tail is shared.
pub async fn run(cfg: &Config) -> Result<Settlement, BoxErr> {
    // Build a provider that signs with the aggregator key.
    let signer = PrivateKeySigner::from_str(&cfg.private_key)?;
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(cfg.rpc_url.parse()?);

    // Compute the settlement via the runtime-selected mode. Disclosure mode is
    // the legacy path; event mode is the proven-weights ZK path.
    let settlement = match &cfg.disclosure_path {
        Some(path) => run_disclosure(cfg, &provider, path).await?,
        None => run_events(cfg, &provider).await?,
    };

    // ---- Shared commit / escrow / write tail (mode-independent) ----

    // Commit the direct (signed) epoch root to CWEPayouts and wait for it to land.
    let payouts_addr = Address::from_str(&cfg.deployments.payouts)?;
    let payouts = Payouts::new(payouts_addr, &provider);
    let pending = payouts
        .commitEpoch(
            U256::from(settlement.epoch),
            B256::from(settlement.merkle_root.0),
            U256::from(settlement.total_credits),
        )
        .send()
        .await?;
    let receipt = pending.get_receipt().await?;
    eprintln!(
        "committed epoch {} direct root in tx {:#x}",
        settlement.epoch, receipt.transaction_hash
    );

    // Route fingerprint-matched credit to escrow. The escrow contract must hold the
    // funds before commit (its solvency check), so the aggregator funds it with the
    // escrow total first. (Production would source this from the subscription pool;
    // for the MVP the aggregator funds it.)
    if !settlement.escrow.is_empty() {
        let escrow_addr = Address::from_str(&cfg.deployments.escrow)?;
        let escrow = Escrow::new(escrow_addr, &provider);
        // Fund the escrow with the total to be committed this epoch.
        let fund = TransactionRequest::default()
            .with_to(escrow_addr)
            .with_value(U256::from(settlement.escrow_total));
        provider.send_transaction(fund).await?.get_receipt().await?;
        // Commit each fingerprint-matched work's escrowed credit.
        for entry in &settlement.escrow {
            escrow
                .commit(
                    U256::from(settlement.epoch),
                    B256::from(entry.work_id.0),
                    U256::from(entry.amount),
                )
                .send()
                .await?
                .get_receipt()
                .await?;
        }
        eprintln!(
            "escrowed {} work(s), total {}",
            settlement.escrow.len(),
            settlement.escrow_total
        );
    }

    // Persist the withdrawal proofs for creators to claim with.
    if let Some(parent) = cfg.out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&settlement)?;
    std::fs::write(&cfg.out_path, json + "\n")?;
    eprintln!("wrote {}", cfg.out_path.display());

    Ok(settlement)
}

/// Fetch every `ConsumptionSubmitted` log for the configured epoch, in canonical
/// chain order (ascending block number, then log index — the order `get_logs`
/// returns). Both modes settle from this same log set; event mode additionally
/// relies on this deterministic order so `allocate_from_raw`'s largest-remainder
/// tie-breaks are reproducible.
async fn epoch_logs<P: Provider>(
    cfg: &Config,
    provider: &P,
) -> Result<Vec<alloy::rpc::types::Log>, BoxErr> {
    let consumption_addr = Address::from_str(&cfg.deployments.consumption)?;
    // `epoch` is the second indexed topic, so filter on topic2.
    let filter = Filter::new()
        .address(consumption_addr)
        .event_signature(Consumption::ConsumptionSubmitted::SIGNATURE_HASH)
        .topic2(U256::from(cfg.epoch))
        .from_block(0);
    Ok(provider.get_logs(&filter).await?)
}

/// Disclosure mode (legacy): assemble a fully-opened DAPR [`Dataset`] from the
/// submissions plus the disclosure file, then [`settle`].
///
/// This is the AcceptAllVerifier path used by the four legacy demos. The
/// `ConsumptionSubmitted` event no longer carries the per-row commitments, so the
/// old "recompute each opening's commitment and check it against the on-chain
/// commitments" step is GONE — disclosure mode therefore trusts the disclosure
/// file's openings as-is and is NOT an integrity path. Cryptographic
/// re-verification of usage lives entirely in event mode ([`run_events`]); this
/// mode exists only to keep the legacy demos green while the ZK path ships.
async fn run_disclosure<P: Provider>(
    cfg: &Config,
    provider: &P,
    disclosure_path: &std::path::Path,
) -> Result<Settlement, BoxErr> {
    // Resolve the contracts this mode reads.
    let tiers_addr = Address::from_str(&cfg.deployments.tiers)?;
    let registry_addr = Address::from_str(&cfg.deployments.registry)?;
    let tiers = Tiers::new(tiers_addr, provider);
    let registry = Registry::new(registry_addr, provider);

    // Load the users' openings for this epoch.
    let disclosure = Disclosure::load(disclosure_path)?;

    let logs = epoch_logs(cfg, provider).await?;

    // Assemble the DAPR dataset from the submissions + disclosed openings.
    let mut tier_fees: BTreeMap<String, u128> = BTreeMap::new();
    let mut usage: Vec<UsageRow> = Vec::new();

    for log in &logs {
        let event = Consumption::ConsumptionSubmitted::decode_log(&log.inner)?;
        let user_hex = format!("{:#x}", event.user); // lowercase 0x address
        let tier_id = event.tierId;

        // Look up the tier fee this user paid.
        let fee = tiers.feeOf(tier_id).call().await?;
        tier_fees.insert(user_hex.clone(), u128::try_from(fee)?);

        // Turn each disclosed opening into a usage row. NOTE: the event no longer
        // carries commitments, so there is nothing on-chain to check the opening
        // against — disclosure mode accepts the file's openings verbatim (see the
        // fn doc: this is the legacy AcceptAllVerifier path, not the integrity path).
        if let Some(openings) = disclosure.for_user(&user_hex) {
            for opening in openings {
                // Price comes from the registry; region is 1.0 in Phase 1.
                let price = registry
                    .pricePerMinOf(B256::from(opening.work_id.0))
                    .call()
                    .await?;
                usage.push(UsageRow {
                    user: user_hex.clone(),
                    work: opening.work_id.to_string(),
                    minutes: opening.minutes,
                    price_ppm: u64::try_from(price)?,
                    region_ppm: 1_000_000,
                    // The opening carries `plays` directly; carried through as-is.
                    plays: opening.plays,
                });
            }
        }
    }

    // Works the client recognized by fingerprint (Tier 2) are escrowed; the rest
    // (Tier 1, signed) pay directly. The disclosure file declares the escrow set.
    let escrow_works: BTreeSet<String> = disclosure
        .escrow_works
        .iter()
        .map(|w| w.to_string())
        .collect();

    // Compute the settlement, split into direct (Merkle) and escrow buckets.
    // Bandwidth credibility is not yet wired into the chain layer (H3 Task 2+),
    // so every work is neutral for now.
    Ok(settle(
        cfg.epoch,
        &Dataset {
            tier_fees,
            usage,
            bandwidth_ppm: BTreeMap::new(),
        },
        &escrow_works,
    )?)
}

/// Event mode (the ZK path): pay from the proven per-work weights carried in each
/// `ConsumptionSubmitted` event, after re-verifying each submission's digest.
///
/// For each submission the job:
/// 1. reads the epoch's pseudonym key `k_epoch = Beacon::keyFor(epoch)`;
/// 2. rebuilds the submission's active [`PublicRow`]s from the event's
///    `(pseudonyms, workIds, weights)` (the per-row `commitment` is not part of
///    the digest preimage, so `[0;32]` is used);
/// 3. recomputes the expected digest with
///    [`cwe_zk_circuits::prove::digest_from_active`] (which pads to `MAX_WORKS`
///    with the canonical padding under this `k_epoch`) and **rejects the whole
///    submission** if it does not match the event's `digest` — this is the
///    off-chain half of the trust chain (the on-chain Groth16 verifier binds the
///    digest to a valid proof; this binds the digest to the paid weights);
/// 4. turns each accepted row into a [`RawRow`] paid via [`settle_raw`].
///
/// **Row ordering:** submissions are processed in canonical chain order (see
/// [`epoch_logs`]) and, within a submission, in the event's `workIds` order.
/// Since a user may submit at most once per epoch, this yields a single,
/// fully-deterministic per-user row sequence, which `allocate_from_raw` needs for
/// reproducible largest-remainder tie-breaks.
///
/// **Escrow:** escrow is EMPTY for cycle-1 — every proven work pays directly.
/// Registry-derived escrow-tier routing in event mode (mapping fingerprint-tier
/// works to escrow from on-chain state rather than a disclosure file) is a
/// documented follow-on.
///
/// **Bandwidth:** if [`Config::receipts_path`] names a receipt bundle, it is
/// verified (signatures, epoch binding, storage-node credential, anti-replay)
/// and turned into a per-row bandwidth credibility via
/// [`crate::settle::settle_raw_with_row_credibility`]. Without a bundle,
/// bandwidth stays neutral and this is byte-for-byte the pre-H5 behaviour —
/// the four legacy demos and the player never set `RECEIPTS`.
async fn run_events<P: Provider>(cfg: &Config, provider: &P) -> Result<Settlement, BoxErr> {
    // Resolve the contracts this mode reads: tier fees and the epoch beacon.
    let tiers_addr = Address::from_str(&cfg.deployments.tiers)?;
    let beacon_addr = Address::from_str(&cfg.deployments.beacon)?;
    let tiers = Tiers::new(tiers_addr, provider);
    let beacon = Beacon::new(beacon_addr, provider);

    // The per-epoch pseudonym key every submission's digest is bound to.
    let k_epoch: [u8; 32] = beacon.keyFor(U256::from(cfg.epoch)).call().await?.0;

    let logs = epoch_logs(cfg, provider).await?;

    // Accumulate proven rows and the fees of ACCEPTED submissions only, so a
    // rejected submission neither pays nor inflates `unallocated`.
    let mut tier_fees: BTreeMap<String, u128> = BTreeMap::new();
    let mut rows: Vec<RawRow> = Vec::new();

    for log in &logs {
        let event = Consumption::ConsumptionSubmitted::decode_log(&log.inner)?;
        let user_hex = format!("{:#x}", event.user); // lowercase 0x address
        let tier_bytes: [u8; 32] = event.tierId.0;

        // Rebuild the active public rows exactly as they entered the digest
        // preimage: pseudonym, work_id and weight per row (commitment is not
        // hashed, so a zero placeholder is fine here).
        let n = event.workIds.len();
        // Defensive: the three parallel arrays must have equal length; a
        // malformed submission with mismatched lengths is rejected outright.
        if event.pseudonyms.len() != n || event.weights.len() != n {
            eprintln!(
                "warning: submission from {user_hex} has mismatched array lengths; rejecting"
            );
            continue;
        }
        let mut active: Vec<PublicRow> = Vec::with_capacity(n);
        for i in 0..n {
            active.push(PublicRow {
                work_id: event.workIds[i].0,
                commitment: [0u8; 32], // not part of the digest preimage
                pseudonym: event.pseudonyms[i].0,
                weight: u128::try_from(event.weights[i])?,
            });
        }

        // Recompute the digest over the active rows (padded internally to
        // MAX_WORKS) and reject the submission if it disagrees with the event's.
        let expected = digest_from_active(cfg.epoch, &tier_bytes, &k_epoch, &active)?;
        if expected != event.digest.0 {
            eprintln!(
                "warning: submission from {user_hex} failed digest re-verification; rejecting"
            );
            continue;
        }

        // Accepted: record the tier fee this user paid and one RawRow per work.
        let fee = tiers.feeOf(event.tierId).call().await?;
        tier_fees.insert(user_hex.clone(), u128::try_from(fee)?);
        for row in &active {
            rows.push(RawRow {
                user: user_hex.clone(),
                // Canonical lowercase 0x-hex form of the work id, matching what
                // `settle` expects for its Merkle leaves.
                work: Bytes32(row.work_id).to_string(),
                raw: row.weight,
            });
        }
    }

    // Bandwidth credibility: with a receipt bundle present, verify it and turn
    // the verified bytes into a per-row discount; without one, stay neutral and
    // pay exactly as before H5.
    let credibility = match &cfg.receipts_path {
        Some(path) => {
            let raw = std::fs::read_to_string(path)?;
            let bundle = ReceiptBundle::from_json(&raw)?;

            // Resolve each distinct node's storage-node credential ONCE, then
            // hand `accept_receipts` a synchronous predicate over the results —
            // it keeps the accept/reject policy free of async and of the chain.
            let identity_addr = Address::from_str(&cfg.deployments.identity)?;
            let identity = Identity::new(identity_addr, provider);
            let cred_type = storage_node_credential_type();
            let mut valid: BTreeMap<String, bool> = BTreeMap::new();
            for signed in &bundle.receipts {
                let node = normalize_addr(&signed.receipt.node);
                if valid.contains_key(&node) {
                    continue;
                }
                // A node address that will not even parse cannot be credentialed.
                let ok = match Address::from_str(&node) {
                    Ok(addr) => identity.isValid(addr, cred_type).call().await?,
                    Err(_) => false,
                };
                valid.insert(node, ok);
            }

            let verified = accept_receipts(&bundle, cfg.epoch, &|node: &str| {
                valid.get(node).copied().unwrap_or(false)
            });
            let ppm = row_credibility_ppm(&rows, &verified, &cfg.rates);
            eprintln!(
                "bandwidth: {} receipts submitted, {} (user, work) pairs credited",
                bundle.receipts.len(),
                verified.len()
            );
            Some(ppm)
        }
        None => None,
    };

    // Pay from the proven weights. Escrow is empty for cycle-1 — every proven
    // work pays directly (see the fn doc).
    Ok(match credibility {
        Some(ppm) => {
            settle_raw_with_row_credibility(cfg.epoch, &tier_fees, &rows, &ppm, &BTreeSet::new())?
        }
        None => settle_raw(
            cfg.epoch,
            &tier_fees,
            &rows,
            &BTreeMap::new(),
            &BTreeSet::new(),
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Rust recomputation of the `STORAGE_NODE` credential type against
    /// the Solidity constant it must match bit-for-bit
    /// (`chain/contracts/CredentialTypes.sol: STORAGE_NODE`), independently
    /// verified with `cast keccak "cwe.credential.storage-node"`. A silent
    /// drift here would mean every bandwidth receipt stops counting with no
    /// error anywhere — this test is what makes that impossible to miss.
    #[test]
    fn cred_type_matches_solidity_constant() {
        let expected: B256 = "0x48b4d2e65fa9d22ac7b0381616bf8c7a839a216a3d4b829879ac674150aa8e86"
            .parse()
            .unwrap();
        assert_eq!(storage_node_credential_type(), expected);
    }
}
