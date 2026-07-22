//! The chain layer: reads submissions and registry/tier data over RPC, and
//! commits the settled epoch root on-chain.
//!
//! This is where the concrete Ethereum stack (alloy) lives, kept apart from the
//! pure [`crate::settle`] logic so the latter stays trivially testable. The live
//! behaviour of this module is exercised by the WP7 end-to-end demo on Anvil.

use std::collections::BTreeMap;
use std::error::Error;
use std::str::FromStr;

use alloy::network::TransactionBuilder;
use alloy::primitives::{Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{Filter, TransactionRequest};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolEvent;

use cwe_dapr::{Dataset, UsageRow};

use crate::config::Config;
use crate::disclosure::Disclosure;
use crate::settle::{settle, Settlement};

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
            address indexed user, uint256 indexed epoch, bytes32 tierId, bytes32[] commitments
        );
    }
    #[sol(rpc)]
    contract Payouts {
        function commitEpoch(uint256 epochId, bytes32 merkleRoot, uint256 totalCredits) external;
    }
    #[sol(rpc)]
    contract Escrow {
        function commit(uint256 epochId, bytes32 workId, uint256 amount) external;
    }
}

/// A boxed error alias keeping the orchestration signature readable.
type BoxErr = Box<dyn Error + Send + Sync>;

/// Run a full settlement against the configured chain and write the proofs file.
///
/// Steps: connect → read this epoch's submissions → open and verify commitments
/// from the disclosure file → assemble the DAPR dataset → settle → commit the root
/// on-chain → persist the withdrawal proofs.
pub async fn run(cfg: &Config) -> Result<Settlement, BoxErr> {
    // Build a provider that signs with the aggregator key.
    let signer = PrivateKeySigner::from_str(&cfg.private_key)?;
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(cfg.rpc_url.parse()?);

    // Resolve the contract addresses from the deployments map.
    let tiers_addr = Address::from_str(&cfg.deployments.tiers)?;
    let registry_addr = Address::from_str(&cfg.deployments.registry)?;
    let consumption_addr = Address::from_str(&cfg.deployments.consumption)?;
    let payouts_addr = Address::from_str(&cfg.deployments.payouts)?;

    let tiers = Tiers::new(tiers_addr, &provider);
    let registry = Registry::new(registry_addr, &provider);
    let payouts = Payouts::new(payouts_addr, &provider);

    // Load the users' openings for this epoch.
    let disclosure = Disclosure::load(&cfg.disclosure_path)?;

    // Pull every ConsumptionSubmitted log for this epoch. `epoch` is the second
    // indexed topic, so filter on topic2.
    let filter = Filter::new()
        .address(consumption_addr)
        .event_signature(Consumption::ConsumptionSubmitted::SIGNATURE_HASH)
        .topic2(U256::from(cfg.epoch))
        .from_block(0);
    let logs = provider.get_logs(&filter).await?;

    // Assemble the DAPR dataset from the submissions + verified openings.
    let mut tier_fees: BTreeMap<String, u128> = BTreeMap::new();
    let mut usage: Vec<UsageRow> = Vec::new();

    for log in &logs {
        let event = Consumption::ConsumptionSubmitted::decode_log(&log.inner)?;
        let user_hex = format!("{:#x}", event.user); // lowercase 0x address
        let tier_id = event.tierId;

        // Look up the tier fee this user paid.
        let fee = tiers.feeOf(tier_id).call().await?;
        tier_fees.insert(user_hex.clone(), u128::try_from(fee)?);

        // The set of commitments this user actually submitted on-chain.
        let submitted: Vec<[u8; 32]> = event.commitments.iter().map(|c| c.0).collect();

        // Turn each disclosed opening into a usage row — but only if its commitment
        // matches one the user submitted, so nobody can inflate their own usage.
        if let Some(openings) = disclosure.for_user(&user_hex) {
            for opening in openings {
                let commit = *opening.commit().as_bytes();
                if !submitted.contains(&commit) {
                    eprintln!(
                        "warning: opening for work {} from {} has no matching on-chain commitment; skipping",
                        opening.work_id, user_hex
                    );
                    continue;
                }
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
                    // The opening's commitment binds `plays`, so this is exactly
                    // what the user committed to on-chain, not a stand-in value.
                    plays: opening.plays,
                });
            }
        }
    }

    // Works the client recognized by fingerprint (Tier 2) are escrowed; the rest
    // (Tier 1, signed) pay directly. The disclosure file declares the escrow set.
    let escrow_works: std::collections::BTreeSet<String> = disclosure
        .escrow_works
        .iter()
        .map(|w| w.to_string())
        .collect();

    // Compute the settlement, split into direct (Merkle) and escrow buckets.
    // Bandwidth credibility is not yet wired into the chain layer (H3 Task 2+),
    // so every work is neutral for now.
    let settlement = settle(
        cfg.epoch,
        &Dataset {
            tier_fees,
            usage,
            bandwidth_ppm: BTreeMap::new(),
        },
        &escrow_works,
    )?;

    // Commit the direct (signed) epoch root to CWEPayouts and wait for it to land.
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
