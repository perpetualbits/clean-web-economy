//! `cwe-hub` — run the Discovery Hub HTTP server.
use std::sync::Arc;
use tokio::sync::RwLock;

use cwe_discovery_hub::api::{router, AppState};
use cwe_discovery_hub::chain::DiscoveryChain;
use cwe_discovery_hub::config::Config;
use cwe_discovery_hub::index::Index;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load config and the persisted index snapshot.
    let cfg = Config::from_env()?;
    let index = Index::load_snapshot(&cfg.snapshot)?;
    let chain = DiscoveryChain::new(&cfg.rpc_url, cfg.registry);
    let state = AppState {
        index: Arc::new(RwLock::new(index)),
        chain: Arc::new(chain),
        snapshot: cfg.snapshot.clone(),
    };
    // Bind and serve.
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    println!("discovery hub listening on {}", cfg.bind);
    axum::serve(listener, router(state)).await?;
    Ok(())
}
