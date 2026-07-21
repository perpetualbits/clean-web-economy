//! HTTP surface for the Discovery Hub (design §7): manifest ingest, resolution,
//! search, trending, manifest/creator reads, health, and an OpenAPI document.
//!
//! Handlers are generic over [`RegistryView`] so the ingest path — the only one
//! that talks to the chain — can be exercised in tests against a fake registry,
//! while production wiring fixes the registry to [`DiscoveryChain`].

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use cwe_wallet_zk::Bytes32;
use serde::Deserialize;
use tokio::sync::RwLock;
use utoipa::OpenApi;

use crate::chain::{DiscoveryChain, RegistryView};
use crate::index::{Index, Resolved, Summary};
use crate::manifest::{Address, WorkManifest, WorkType};

/// Body of `POST /manifests`: a manifest plus its `0x`-prefixed hex EIP-191
/// signature over the manifest's canonical bytes.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct IngestBody {
    pub manifest: WorkManifest,
    pub signature: String,
}

/// Hub state generic over the registry implementation. Kept internal so tests
/// can build one over a fake registry via [`test_state`]/`router_generic`.
struct GenericState<R> {
    index: Arc<RwLock<Index>>,
    registry: Arc<R>,
    snapshot: PathBuf,
}

// Written by hand rather than derived: `#[derive(Clone)]` would add a spurious
// `R: Clone` bound, when `Arc<R>` is always `Clone` regardless of `R`.
impl<R> Clone for GenericState<R> {
    fn clone(&self) -> Self {
        GenericState {
            index: Arc::clone(&self.index),
            registry: Arc::clone(&self.registry),
            snapshot: self.snapshot.clone(),
        }
    }
}

/// Production hub state, backed by a live [`DiscoveryChain`] registry.
pub struct AppState {
    pub index: Arc<RwLock<Index>>,
    pub chain: Arc<DiscoveryChain>,
    pub snapshot: PathBuf,
}

/// Build the production router, fixing the registry type to [`DiscoveryChain`].
pub fn router(state: AppState) -> Router {
    router_generic(GenericState {
        index: state.index,
        registry: state.chain,
        snapshot: state.snapshot,
    })
}

/// Build a router over any [`RegistryView`]; used directly by tests (with a
/// fake registry) and indirectly by [`router`] for production.
fn router_generic<R: RegistryView + Send + Sync + 'static>(state: GenericState<R>) -> Router {
    Router::new()
        .route("/manifests", post(ingest::<R>))
        .route("/resolve/{fingerprint}", get(resolve::<R>))
        .route("/search", get(search::<R>))
        .route("/trending", get(trending::<R>))
        .route("/manifest/{work_id}", get(manifest_handler::<R>))
        .route("/creator/{address}", get(creator::<R>))
        .route("/healthz", get(healthz::<R>))
        .route("/openapi.json", get(openapi_handler))
        .with_state(state)
}

/// Build a `{ "error": message }` response with the given status.
fn err(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": message })))
}

/// The current Unix time in whole seconds. Used to clamp client-supplied
/// timestamps and to compute recency for trending.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `POST /manifests` — validate a signed manifest against the chain, then index it.
async fn ingest<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Json(body): Json<IngestBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Decode the hex signature.
    let sig = hex::decode(body.signature.trim_start_matches("0x"))
        .map_err(|_| err(StatusCode::BAD_REQUEST, "bad signature hex"))?;
    // Validate against the chain, then insert and snapshot.
    crate::chain::validate_ingest(&body.manifest, &sig, state.registry.as_ref())
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    // Clamp the client-supplied timestamp to the server's clock: `created_at`
    // orders /trending, so an unclamped future value could pin a work to the top.
    let mut manifest = body.manifest;
    manifest.created_at = manifest.created_at.min(now_secs());
    let work_id = manifest.work_id;
    {
        let mut idx = state.index.write().await;
        idx.upsert(manifest)
            .map_err(|e| err(StatusCode::CONFLICT, &e.to_string()))?;
        idx.save_snapshot(&state.snapshot)
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    }
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "work_id": work_id })),
    ))
}

/// `GET /resolve/{fingerprint}` — the extension seam; resolves a fingerprint to
/// the work's payout-relevant fields.
async fn resolve<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Path(fingerprint): Path<String>,
) -> Result<Json<Resolved>, (StatusCode, Json<serde_json::Value>)> {
    let idx = state.index.read().await;
    idx.resolve(&fingerprint)
        .map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "fingerprint not found"))
}

/// Fixed page size for `/search` and `/trending` (design §5). The MVP does not
/// let clients choose a page size — this also keeps `page` bounded well away
/// from overflowing `Index::search`'s `page.saturating_sub(1) * page_size`.
const PAGE_SIZE: usize = 20;

/// Query parameters accepted by `GET /search`.
#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    #[serde(rename = "type")]
    work_type: Option<WorkType>,
    page: Option<usize>,
}

/// `GET /search` — ranked text search over title/tags/description, paginated.
async fn search<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Query(params): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let page = params.page.unwrap_or(1).max(1);
    let idx = state.index.read().await;
    let (results, total) = idx.search(
        params.q.as_deref().unwrap_or(""),
        params.work_type,
        page,
        PAGE_SIZE,
    );
    Json(serde_json::json!({ "results": results, "page": page, "total": total }))
}

/// Query parameters accepted by `GET /trending`.
#[derive(Debug, Deserialize)]
struct TrendingQuery {
    #[serde(rename = "type")]
    work_type: Option<WorkType>,
}

/// `GET /trending` — recency-ranked list of works, optionally filtered by type.
async fn trending<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Query(params): Query<TrendingQuery>,
) -> Json<serde_json::Value> {
    let idx = state.index.read().await;
    let results = idx.trending(params.work_type, now_secs(), PAGE_SIZE);
    Json(serde_json::json!({ "results": results }))
}

/// `GET /manifest/{work_id}` — the full manifest for a work id.
async fn manifest_handler<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Path(work_id): Path<String>,
) -> Result<Json<WorkManifest>, (StatusCode, Json<serde_json::Value>)> {
    let work_id =
        Bytes32::from_str(&work_id).map_err(|_| err(StatusCode::BAD_REQUEST, "bad work id"))?;
    let idx = state.index.read().await;
    idx.manifest(&work_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "work not found"))
}

/// `GET /creator/{address}` — a creator's works and their count.
async fn creator<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let address =
        Address::from_str(&address).map_err(|_| err(StatusCode::BAD_REQUEST, "bad address"))?;
    let idx = state.index.read().await;
    let works: Vec<Summary> = idx.by_creator(&address);
    let count = works.len();
    Ok(Json(
        serde_json::json!({ "creator_id": address, "works": works, "count": count }),
    ))
}

/// `GET /healthz` — liveness probe reporting how many works are indexed.
async fn healthz<R: RegistryView + Send + Sync>(
    State(state): State<GenericState<R>>,
) -> Json<serde_json::Value> {
    let idx = state.index.read().await;
    Json(serde_json::json!({ "status": "ok", "indexed": idx.len() }))
}

/// The OpenAPI document's component schemas, derived from the wire types.
///
/// `WorkManifest`/`WorkType`/`Summary`/`Resolved`/`IngestBody` all derive
/// `utoipa::ToSchema`; their `Bytes32`/`Address` fields are foreign types utoipa
/// cannot introspect, so they are annotated `#[schema(value_type = String)]` to
/// match the hex-string form those types actually serialise to.
#[derive(utoipa::OpenApi)]
#[openapi(components(schemas(WorkManifest, WorkType, Summary, Resolved, IngestBody)))]
struct ApiDoc;

/// The service's OpenAPI 3 document, as a pretty-printed JSON string.
///
/// Component schemas come from the `utoipa` derives above; the `paths` object is
/// authored by hand and merged in, because the route handlers are generic over
/// the registry type and so cannot themselves carry `#[utoipa::path]`
/// annotations (those require a concrete, non-generic function per route).
pub fn openapi_json() -> String {
    let generated = ApiDoc::openapi()
        .to_pretty_json()
        .expect("utoipa openapi document serialises to json");
    let mut doc: serde_json::Value =
        serde_json::from_str(&generated).expect("utoipa openapi document is valid json");
    doc["info"]["title"] = serde_json::json!("Discovery Hub API");
    doc["info"]["version"] = serde_json::json!(env!("CARGO_PKG_VERSION"));
    doc["paths"] = paths_json();
    serde_json::to_string_pretty(&doc).expect("openapi document serialises to json")
}

/// Hand-authored `paths` object for the OpenAPI document (see [`openapi_json`]).
fn paths_json() -> serde_json::Value {
    let work_type_schema =
        serde_json::json!({ "type": "string", "enum": ["audio", "video", "text"] });
    serde_json::json!({
        "/manifests": {
            "post": {
                "summary": "Ingest a signed work manifest",
                "operationId": "ingestManifest",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/IngestBody" }
                    } }
                },
                "responses": {
                    "201": { "description": "Manifest indexed", "content": { "application/json": {
                        "schema": { "type": "object", "properties": { "work_id": { "type": "string" } } }
                    } } },
                    "400": { "description": "Invalid signature or failed chain validation" },
                    "409": { "description": "Fingerprint already claimed by another work" }
                }
            }
        },
        "/resolve/{fingerprint}": {
            "get": {
                "summary": "Resolve a fingerprint to its payout-relevant fields",
                "operationId": "resolveFingerprint",
                "parameters": [
                    { "name": "fingerprint", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": {
                    "200": { "description": "Resolved", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Resolved" }
                    } } },
                    "404": { "description": "Fingerprint not found" }
                }
            }
        },
        "/search": {
            "get": {
                "summary": "Ranked text search over indexed works",
                "operationId": "searchWorks",
                "parameters": [
                    { "name": "q", "in": "query", "schema": { "type": "string" } },
                    { "name": "type", "in": "query", "schema": work_type_schema },
                    { "name": "page", "in": "query", "schema": { "type": "integer", "minimum": 1 } }
                ],
                "responses": {
                    "200": { "description": "Search results", "content": { "application/json": {
                        "schema": { "type": "object", "properties": {
                            "results": { "type": "array", "items": { "$ref": "#/components/schemas/Summary" } },
                            "page": { "type": "integer" },
                            "total": { "type": "integer" }
                        } }
                    } } }
                }
            }
        },
        "/trending": {
            "get": {
                "summary": "Recency-ranked list of works",
                "operationId": "trendingWorks",
                "parameters": [
                    { "name": "type", "in": "query", "schema": work_type_schema }
                ],
                "responses": {
                    "200": { "description": "Trending results", "content": { "application/json": {
                        "schema": { "type": "object", "properties": {
                            "results": { "type": "array", "items": { "$ref": "#/components/schemas/Summary" } }
                        } }
                    } } }
                }
            }
        },
        "/manifest/{work_id}": {
            "get": {
                "summary": "Fetch a work's full manifest",
                "operationId": "getManifest",
                "parameters": [
                    { "name": "work_id", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": {
                    "200": { "description": "Manifest", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/WorkManifest" }
                    } } },
                    "400": { "description": "Malformed work id" },
                    "404": { "description": "Work not found" }
                }
            }
        },
        "/creator/{address}": {
            "get": {
                "summary": "A creator's works and their count",
                "operationId": "getCreatorWorks",
                "parameters": [
                    { "name": "address", "in": "path", "required": true, "schema": { "type": "string" } }
                ],
                "responses": {
                    "200": { "description": "Creator works", "content": { "application/json": {
                        "schema": { "type": "object", "properties": {
                            "creator_id": { "type": "string" },
                            "works": { "type": "array", "items": { "$ref": "#/components/schemas/Summary" } },
                            "count": { "type": "integer" }
                        } }
                    } } },
                    "400": { "description": "Malformed address" }
                }
            }
        },
        "/healthz": {
            "get": {
                "summary": "Liveness probe",
                "operationId": "healthz",
                "responses": {
                    "200": { "description": "OK", "content": { "application/json": {
                        "schema": { "type": "object", "properties": {
                            "status": { "type": "string" },
                            "indexed": { "type": "integer" }
                        } }
                    } } }
                }
            }
        }
    })
}

/// `GET /openapi.json` — serve the generated OpenAPI 3 document.
async fn openapi_handler() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], openapi_json())
}

/// Build a [`GenericState`] over a fake `registry`, with a fresh empty index and
/// a throwaway snapshot path, for route tests.
#[cfg(test)]
fn test_state<R: RegistryView>(registry: R) -> GenericState<R> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let snapshot = std::env::temp_dir().join(format!(
        "cwe_hub_test_snapshot_{}_{}.json",
        std::process::id(),
        n
    ));
    GenericState {
        index: Arc::new(RwLock::new(Index::new())),
        registry: Arc::new(registry),
        snapshot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::OnChainWork;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::signers::SignerSync;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt; // for `oneshot`

    struct FakeReg(Address);
    impl RegistryView for FakeReg {
        async fn lookup(&self, _w: Bytes32) -> Result<Option<OnChainWork>, String> {
            Ok(Some(OnChainWork {
                registrant: self.0,
                price_per_min: 1_000_000,
                region: Bytes32([0; 32]),
            }))
        }
    }

    /// Ingesting a valid manifest then resolving its fingerprint round-trips.
    #[tokio::test]
    async fn ingest_then_resolve() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state.clone());

        let m = WorkManifest {
            work_id: Bytes32([1; 32]),
            content_id: Bytes32([1; 32]),
            fingerprint: "fp:aa".to_string(),
            title: "Song".to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: signer.address(),
            created_at: 1,
            payees: vec![(signer.address(), 1_000_000)],
        };
        let sig = format!(
            "0x{}",
            hex::encode(
                signer
                    .sign_message_sync(&m.canonical_bytes().unwrap())
                    .unwrap()
                    .as_bytes()
            )
        );
        let body = serde_json::json!({ "manifest": m, "signature": sig }).to_string();

        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(Request::get("/resolve/fp:aa").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// A far-future client `created_at` is clamped to the server clock at ingest,
    /// so it cannot be abused to pin a work to the top of /trending.
    #[tokio::test]
    async fn ingest_clamps_future_created_at() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state.clone());

        let m = WorkManifest {
            work_id: Bytes32([9; 32]),
            content_id: Bytes32([9; 32]),
            fingerprint: "fp:clamp".to_string(),
            title: "Future".to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: signer.address(),
            created_at: u64::MAX, // absurd future timestamp
            payees: vec![(signer.address(), 1_000_000)],
        };
        // Capture the work id (Copy) before `m` is moved into the request body.
        let work_id = m.work_id;
        let sig = format!(
            "0x{}",
            hex::encode(
                signer
                    .sign_message_sync(&m.canonical_bytes().unwrap())
                    .unwrap()
                    .as_bytes()
            )
        );
        let body = serde_json::json!({ "manifest": m, "signature": sig }).to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Fetch the stored manifest; its created_at must have been clamped.
        let resp = app
            .oneshot(
                Request::get(format!("/manifest/{work_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let created = stored["created_at"].as_u64().unwrap();
        let now = now_secs();
        assert!(
            created <= now,
            "created_at {created} should be clamped to <= {now}"
        );
        assert_ne!(created, u64::MAX);
    }

    /// Resolving an unknown fingerprint returns 404.
    #[tokio::test]
    async fn resolve_missing_is_not_found() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);
        let resp = app
            .oneshot(
                Request::get("/resolve/fp:missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// A manifest whose signature was not produced by the on-chain registrant is
    /// rejected with 400, and never reaches the index.
    #[tokio::test]
    async fn ingest_rejects_wrong_signer() {
        let signer = PrivateKeySigner::random();
        let registrant = PrivateKeySigner::random();
        let state = test_state(FakeReg(registrant.address()));
        let app = router_generic(state);

        let m = WorkManifest {
            work_id: Bytes32([2; 32]),
            content_id: Bytes32([2; 32]),
            fingerprint: "fp:bb".to_string(),
            title: "Song".to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: registrant.address(),
            created_at: 1,
            payees: vec![(registrant.address(), 1_000_000)],
        };
        let sig = format!(
            "0x{}",
            hex::encode(
                signer
                    .sign_message_sync(&m.canonical_bytes().unwrap())
                    .unwrap()
                    .as_bytes()
            )
        );
        let body = serde_json::json!({ "manifest": m, "signature": sig }).to_string();
        let resp = app
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// `/healthz` reports the number of indexed works.
    #[tokio::test]
    async fn healthz_reports_ok() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);
        let resp = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// `/openapi.json` serves a document with the expected paths.
    #[tokio::test]
    async fn openapi_document_is_served() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);
        let resp = app
            .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(doc["paths"]["/manifests"].is_object());
        assert!(doc["components"]["schemas"]["WorkManifest"].is_object());
    }

    /// Build a manifest with the given identity/fingerprint/title, otherwise
    /// matching the fixed on-chain facts [`FakeReg`] returns (so it always
    /// passes chain validation for the signer that owns `creator`).
    fn manifest(
        work_id: [u8; 32],
        fingerprint: &str,
        title: &str,
        creator: Address,
    ) -> WorkManifest {
        WorkManifest {
            work_id: Bytes32(work_id),
            content_id: Bytes32(work_id),
            fingerprint: fingerprint.to_string(),
            title: title.to_string(),
            description: String::new(),
            tags: vec![],
            work_type: WorkType::Audio,
            price_per_min: 1_000_000,
            region: Bytes32([0; 32]),
            creator_id: creator,
            created_at: 1,
            payees: vec![(creator, 1_000_000)],
        }
    }

    /// Sign `m` with `signer` and JSON-encode the `POST /manifests` body.
    fn ingest_body(m: &WorkManifest, signer: &PrivateKeySigner) -> String {
        let sig = format!(
            "0x{}",
            hex::encode(
                signer
                    .sign_message_sync(&m.canonical_bytes().unwrap())
                    .unwrap()
                    .as_bytes()
            )
        );
        serde_json::json!({ "manifest": m, "signature": sig }).to_string()
    }

    /// `GET /search` finds an ingested work by a token in its title.
    #[tokio::test]
    async fn search_finds_ingested_work() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);

        let m = manifest([3; 32], "fp:search", "Nightjar Melodies", signer.address());
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&m, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::get("/search?q=nightjar")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["total"], 1);
        assert_eq!(body["results"][0]["work_id"], m.work_id.to_string());
    }

    /// `GET /trending` lists an ingested work.
    #[tokio::test]
    async fn trending_lists_ingested_work() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);

        let m = manifest([4; 32], "fp:trend", "Trending Tune", signer.address());
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&m, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(Request::get("/trending").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["results"][0]["work_id"], m.work_id.to_string());
    }

    /// `GET /manifest/{work_id}` returns the manifest for a known work, and 404
    /// for an unregistered one.
    #[tokio::test]
    async fn manifest_success_and_not_found() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);

        let m = manifest([5; 32], "fp:manifest", "Manifest Work", signer.address());
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&m, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("/manifest/{}", m.work_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["title"], "Manifest Work");

        let missing = Bytes32([0xab; 32]);
        let resp = app
            .oneshot(
                Request::get(format!("/manifest/{}", missing))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// `GET /creator/{address}` returns a creator's works and count, and 400 for
    /// a malformed address.
    #[tokio::test]
    async fn creator_success_and_bad_address() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);

        let m = manifest([6; 32], "fp:creator", "Creator Work", signer.address());
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&m, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("/creator/{}", signer.address()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["count"], 1);

        let resp = app
            .oneshot(
                Request::get("/creator/not-an-address")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Ingesting a different work with a fingerprint already claimed by another
    /// work is rejected with 409, even though it otherwise passes chain
    /// validation.
    #[tokio::test]
    async fn ingest_duplicate_fingerprint_is_conflict() {
        let signer = PrivateKeySigner::random();
        let state = test_state(FakeReg(signer.address()));
        let app = router_generic(state);

        let a = manifest([7; 32], "fp:aa", "First Work", signer.address());
        let resp = app
            .clone()
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&a, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let b = manifest([8; 32], "fp:aa", "Second Work", signer.address());
        let resp = app
            .oneshot(
                Request::post("/manifests")
                    .header("content-type", "application/json")
                    .body(Body::from(ingest_body(&b, &signer)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }
}
