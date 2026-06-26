//! REST API for the packing service.
//!
//! Provides HTTP endpoints for communication with the frontend.
//! Uses Axum as the web framework and supports CORS.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Router,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use serde_json::json;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use utoipa::{OpenApi, ToSchema};

use crate::config::{ApiConfig, OptimizerConfig, RequestLimits};
use crate::model::{Box3D, Container, ContainerBlueprint, ValidationError};
use crate::optimizer::{
    ContainerDiagnostics, PackingConfig, PackingDiagnosticsSummary, PackingResult,
    SupportDiagnostics, pack_objects_with_config, pack_objects_with_progress,
};
use crate::packaging::{PackagingFill, PackagingSummary};

#[derive(Clone)]
struct ApiState {
    optimizer_config: OptimizerConfig,
    limits: RequestLimits,
}

static OPENAPI_DOC: OnceLock<utoipa::openapi::OpenApi> = OnceLock::new();

// SRI hashes verified against https://unpkg.com/swagger-ui-dist@5.17.14/ on 2025-10-29.
const SWAGGER_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
    <head>
        <meta charset="utf-8" />
        <title>sort-it-now API Docs</title>
        <link
            rel="stylesheet"
            href="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui.css"
            integrity="sha384-wxLW6kwyHktdDGr6Pv1zgm/VGJh99lfUbzSn6HNHBENZlCN7W602k9VkGdxuFvPn"
            crossorigin="anonymous"
        />
    </head>
    <body>
        <div id="swagger-ui"></div>
        <script
            src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-bundle.js"
            integrity="sha384-wmyclcVGX/WhUkdkATwhaK1X1JtiNrr2EoYJ+diV3vj4v6OC5yCeSu+yW13SYJep"
            crossorigin="anonymous"
        ></script>
        <script
            src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-standalone-preset.js"
            integrity="sha384-2YH8WDRaj7V2OqU/trsmzSagmk/E2SutiCsGkdgoQwC9pNUJV1u/141DHB6jgs8t"
            crossorigin="anonymous"
        ></script>
        <script>
            window.onload = function () {
                const ui = SwaggerUIBundle({
                    url: "/docs/openapi.json",
                    dom_id: "#swagger-ui",
                    presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
                    layout: "StandaloneLayout",
                });
                window.ui = ui;
            };
        </script>
    </body>
    </html>"##;

fn openapi_doc() -> &'static utoipa::openapi::OpenApi {
    OPENAPI_DOC.get_or_init(ApiDoc::openapi)
}

/// Embedded Web Assets (HTML, CSS, JS)
#[derive(RustEmbed)]
#[folder = "web/"]
struct WebAssets;

/// Request structure for the packing endpoint.
///
/// `containers` contains the possible packaging types that can be combined.
#[derive(Deserialize, Clone, ToSchema)]
pub struct ContainerRequest {
    pub name: Option<String>,
    #[schema(value_type = [f64; 3], example = json!([120.0, 100.0, 80.0]))]
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
}

impl ContainerRequest {
    fn into_blueprint(self, id: usize) -> Result<ContainerBlueprint, ValidationError> {
        ContainerBlueprint::new(id, self.name, self.dims, self.max_weight)
    }
}

#[derive(Deserialize, ToSchema)]
#[schema(
    example = json!({
        "containers": [
            {
                "name": "Standardkiste",
                "dims": [120.0, 100.0, 80.0],
                "max_weight": 500.0
            }
        ],
        "objects": [
            { "id": 1, "dims": [30.0, 40.0, 20.0], "weight": 5.0 }
        ],
        "allow_rotations": true
    })
)]
pub struct PackRequest {
    pub containers: Vec<ContainerRequest>,
    pub objects: Vec<Box3D>,
    #[serde(default)]
    #[schema(nullable = true)]
    pub allow_rotations: Option<bool>,
}

#[derive(Debug)]
struct ValidatedPackRequest {
    containers: Vec<ContainerBlueprint>,
    objects: Vec<Box3D>,
    allow_rotations: Option<bool>,
}

impl ValidatedPackRequest {
    fn into_parts(self) -> (Vec<Box3D>, Vec<ContainerBlueprint>, Option<bool>) {
        (self.objects, self.containers, self.allow_rotations)
    }
}

/// Reasons a [`PackRequest`] can be rejected before packing begins.
#[derive(Debug)]
pub enum PackRequestValidationError {
    MissingContainers,
    InvalidContainer(ValidationError),
    InvalidObject(ValidationError),
    TooManyContainers { count: usize, max: usize },
    TooManyObjects { count: usize, max: usize },
}

impl std::fmt::Display for PackRequestValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackRequestValidationError::MissingContainers => {
                write!(f, "At least one packaging type must be specified")
            }
            PackRequestValidationError::InvalidContainer(err) => {
                write!(f, "Invalid container configuration: {err}")
            }
            PackRequestValidationError::InvalidObject(err) => write!(f, "{err}"),
            PackRequestValidationError::TooManyContainers { count, max } => write!(
                f,
                "Too many container types: {count} exceeds the configured limit of {max}"
            ),
            PackRequestValidationError::TooManyObjects { count, max } => write!(
                f,
                "Too many objects: {count} exceeds the configured limit of {max}"
            ),
        }
    }
}

impl std::error::Error for PackRequestValidationError {}

impl PackRequest {
    fn into_validated(
        self,
        limits: RequestLimits,
    ) -> Result<ValidatedPackRequest, PackRequestValidationError> {
        if self.containers.is_empty() {
            return Err(PackRequestValidationError::MissingContainers);
        }

        if !limits.allows_containers(self.containers.len()) {
            return Err(PackRequestValidationError::TooManyContainers {
                count: self.containers.len(),
                max: limits.max_containers(),
            });
        }

        if !limits.allows_objects(self.objects.len()) {
            return Err(PackRequestValidationError::TooManyObjects {
                count: self.objects.len(),
                max: limits.max_objects(),
            });
        }

        let containers = self
            .containers
            .into_iter()
            .enumerate()
            .map(|(idx, spec)| spec.into_blueprint(idx))
            .collect::<Result<Vec<_>, ValidationError>>()
            .map_err(PackRequestValidationError::InvalidContainer)?;

        let objects = self
            .objects
            .into_iter()
            .map(|obj| Box3D::new(obj.id, obj.dims, obj.weight))
            .collect::<Result<Vec<_>, ValidationError>>()
            .map_err(PackRequestValidationError::InvalidObject)?;

        Ok(ValidatedPackRequest {
            containers,
            objects,
            allow_rotations: self.allow_rotations,
        })
    }
}

/// Runs validation and packing for a deserialized request, returning a structured error.
///
/// This is the single shared entry point used by both the HTTP `/pack` handler and the offline
/// CLI, keeping request handling DRY across transports.
pub fn run_pack(
    request: PackRequest,
    base_config: PackingConfig,
    limits: RequestLimits,
) -> Result<PackResponse, PackRequestValidationError> {
    let validated = request.into_validated(limits)?;
    let (objects, container_blueprints, allow_rotations_override) = validated.into_parts();

    let mut packing_config = base_config;
    if let Some(allow_rotations) = allow_rotations_override {
        packing_config.allow_item_rotation = allow_rotations;
    }

    let packing_result = pack_objects_with_config(objects, container_blueprints, packing_config);
    Ok(PackResponse::from_packing_result(packing_result))
}

/// Response structure with all packed containers.
///
/// # Fields
/// * `results` - Vector of containers with placed objects
#[derive(Serialize, ToSchema)]
pub struct PackResponse {
    pub results: Vec<PackedContainer>,
    pub unplaced: Vec<PackedUnplacedObject>,
    pub is_complete: bool,
    pub diagnostics_summary: PackingDiagnosticsSummary,
}

/// Single container with metadata and placed objects.
///
/// # Fields
/// * `id` - Container number (1-based)
/// * `total_weight` - Total weight of all objects in the container
/// * `placed` - List of placed objects with positions
#[derive(Serialize, ToSchema)]
pub struct PackedContainer {
    pub id: usize,
    pub template_id: Option<usize>,
    pub label: Option<String>,
    #[schema(value_type = [f64; 3], example = json!([120.0, 100.0, 80.0]))]
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
    pub total_weight: f64,
    pub placed: Vec<PackedObject>,
    pub diagnostics: ContainerDiagnostics,
}

/// Single placed object in the response.
///
/// # Fields
/// * `id` - Object ID
/// * `pos` - Position (x, y, z) in the container
/// * `weight` - Weight in kg
/// * `dims` - Dimensions (width, depth, height)
#[derive(Serialize, ToSchema)]
pub struct PackedObject {
    pub id: usize,
    #[schema(value_type = [f64; 3], example = json!([0.0, 0.0, 0.0]))]
    pub pos: (f64, f64, f64),
    pub weight: f64,
    #[schema(value_type = [f64; 3], example = json!([30.0, 40.0, 20.0]))]
    pub dims: (f64, f64, f64),
}

#[derive(Serialize, ToSchema)]
pub struct PackedUnplacedObject {
    pub id: usize,
    pub weight: f64,
    #[schema(value_type = [f64; 3], example = json!([35.0, 45.0, 25.0]))]
    pub dims: (f64, f64, f64),
    pub reason_code: String,
    pub reason: String,
}

#[derive(Serialize, ToSchema)]
struct ErrorResponse {
    error: String,
    details: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: details.into(),
        }
    }
}

fn error_response(
    status: StatusCode,
    error: impl Into<String>,
    details: impl Into<String>,
) -> Response {
    (status, Json(ErrorResponse::new(error, details))).into_response()
}

fn json_deserialize_error(err: JsonRejection) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Invalid JSON data",
        err.to_string(),
    )
}

fn validation_error(details: impl Into<String>) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Invalid input data",
        details,
    )
}

fn container_config_error(details: impl Into<String>) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Invalid container configuration",
        details,
    )
}

/// Extracts a [`PackRequest`] from the request body, mapping deserialization failures to a 422.
///
/// The error variant is boxed because an axum [`Response`] is comparatively large; boxing keeps
/// the common `Ok` path cheap to move around (see `clippy::result_large_err`).
fn parse_json_body(
    payload: Result<Json<PackRequest>, JsonRejection>,
) -> Result<PackRequest, Box<Response>> {
    match payload {
        Ok(Json(payload)) => Ok(payload),
        Err(err) => Err(Box::new(json_deserialize_error(err))),
    }
}

/// Maps a structured validation error to the appropriate HTTP error response.
fn pack_validation_response(err: PackRequestValidationError) -> Response {
    match err {
        PackRequestValidationError::MissingContainers => validation_error(err.to_string()),
        PackRequestValidationError::InvalidContainer(ref inner) => {
            container_config_error(inner.to_string())
        }
        PackRequestValidationError::InvalidObject(ref inner) => validation_error(inner.to_string()),
        PackRequestValidationError::TooManyContainers { .. }
        | PackRequestValidationError::TooManyObjects { .. } => validation_error(err.to_string()),
    }
}

impl PackResponse {
    /// Creates a PackResponse from a PackingResult (DRY principle).
    pub fn from_packing_result(result: PackingResult) -> Self {
        let PackingResult {
            containers,
            unplaced,
            container_diagnostics,
            diagnostics_summary,
        } = result;

        let is_complete = unplaced.is_empty();
        let unplaced_entries = unplaced;

        Self {
            results: containers
                .into_iter()
                .zip(container_diagnostics)
                .enumerate()
                .map(|(i, (cont, diagnostics))| {
                    let Container {
                        dims,
                        max_weight,
                        placed,
                        template_id,
                        label,
                    } = cont;

                    let total_weight = placed.iter().map(|b| b.object.weight).sum();
                    let placed_objects = placed
                        .into_iter()
                        .map(|p| PackedObject {
                            id: p.object.id,
                            pos: p.position,
                            weight: p.object.weight,
                            dims: p.object.dims,
                        })
                        .collect();

                    PackedContainer {
                        id: i + 1,
                        template_id,
                        label,
                        dims,
                        max_weight,
                        total_weight,
                        placed: placed_objects,
                        diagnostics,
                    }
                })
                .collect(),
            unplaced: unplaced_entries
                .into_iter()
                .map(|entry| PackedUnplacedObject {
                    id: entry.object.id,
                    weight: entry.object.weight,
                    dims: entry.object.dims,
                    reason_code: entry.reason.code().to_string(),
                    reason: entry.reason.to_string(),
                })
                .collect(),
            is_complete,
            diagnostics_summary,
        }
    }
}

/// Liveness/readiness response for monitoring and orchestration probes.
#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always `"ok"` when the service is able to answer requests.
    pub status: &'static str,
}

impl HealthResponse {
    fn healthy() -> Self {
        Self { status: "ok" }
    }
}

/// Build and version information for the running service.
#[derive(Serialize, ToSchema)]
pub struct VersionResponse {
    /// Crate name as defined in `Cargo.toml`.
    pub name: &'static str,
    /// Semantic version of the running build.
    pub version: &'static str,
    /// Short human-readable description of the service.
    pub description: &'static str,
}

impl VersionResponse {
    fn current() -> Self {
        Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
            description: env!("CARGO_PKG_DESCRIPTION"),
        }
    }
}

/// The active server-side packing configuration and request guardrails.
///
/// Lets clients introspect the defaults that apply when a request omits `allow_rotations`, and the
/// limits that requests must respect.
#[derive(Serialize, ToSchema)]
pub struct ConfigResponse {
    pub grid_step: f64,
    pub support_ratio: f64,
    pub height_epsilon: f64,
    pub general_epsilon: f64,
    pub balance_limit_ratio: f64,
    pub footprint_cluster_tolerance: f64,
    pub allow_item_rotation: bool,
    pub max_objects: usize,
    pub max_containers: usize,
}

impl ConfigResponse {
    fn from_parts(config: PackingConfig, limits: RequestLimits) -> Self {
        Self {
            grid_step: config.grid_step,
            support_ratio: config.support_ratio,
            height_epsilon: config.height_epsilon,
            general_epsilon: config.general_epsilon,
            balance_limit_ratio: config.balance_limit_ratio,
            footprint_cluster_tolerance: config.footprint_cluster_tolerance,
            allow_item_rotation: config.allow_item_rotation,
            max_objects: limits.max_objects(),
            max_containers: limits.max_containers(),
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        handle_pack,
        handle_pack_stream,
        handle_health,
        handle_version,
        handle_config
    ),
    components(
        schemas(
            PackRequest,
            ContainerRequest,
            PackResponse,
            PackedContainer,
            PackedObject,
            PackedUnplacedObject,
            ErrorResponse,
            HealthResponse,
            VersionResponse,
            ConfigResponse,
            Box3D,
            ContainerDiagnostics,
            SupportDiagnostics,
            PackingDiagnosticsSummary,
            PackagingFill,
            PackagingSummary
        )
    ),
    tags(
        (name = "packing", description = "Endpoints for packing optimization"),
        (name = "system", description = "Service health, version, and configuration endpoints")
    )
)]
struct ApiDoc;

/// Builds the fully configured Axum [`Router`] for the service.
///
/// Exposed so that integration tests (and embedders) can exercise the complete routing and
/// handler stack without binding a TCP socket.
pub fn build_router(optimizer_config: OptimizerConfig, limits: RequestLimits) -> Router {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(Any)
        .allow_headers(Any);

    let state = ApiState {
        optimizer_config,
        limits,
    };

    Router::new()
        // API endpoints
        .route("/pack", post(handle_pack))
        .route("/pack_stream", post(handle_pack_stream))
        // System endpoints
        .route("/health", get(handle_health))
        .route("/version", get(handle_version))
        .route("/config", get(handle_config))
        // API documentation
        .route("/docs/openapi.json", get(serve_openapi_json))
        .route("/docs", get(serve_openapi_ui))
        // Web-UI (embedded)
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_static))
        .layer(cors)
        .with_state(state)
}

/// Starts the API server on the configured address.
///
/// Configures CORS for cross-origin requests from the frontend.
/// Blocks until the server is terminated.
pub async fn start_api_server(config: ApiConfig, optimizer_config: OptimizerConfig) {
    let app = build_router(optimizer_config, config.request_limits());

    let addr = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            panic!("❌ Could not bind API server to {}: {}", addr, err);
        }
    };

    let display_host = config.display_host().to_string();
    println!(
        "🚀 Server running on http://{}:{}",
        display_host,
        config.port()
    );
    if config.binds_to_all_interfaces() && config.uses_default_host() {
        println!("💡 Local access: http://localhost:{}", config.port());
    }
    println!("📦 API Endpoints:");
    println!("   - POST /pack");
    println!("   - POST /pack_stream");
    println!("   - GET /health");
    println!("   - GET /version");
    println!("   - GET /config");
    println!("📑 Documentation:");
    println!("   - GET /docs");
    println!("   - GET /docs/openapi.json");
    println!("🌐 Web-UI: http://{}:{}", display_host, config.port());

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("❌ API server terminated with an error: {err}");
    }
}

/// Handler for POST /pack endpoint.
///
/// Takes a list of objects and packs them optimally into containers.
///
/// # Parameters
/// * `payload` - JSON payload with container dimensions and objects
///
/// # Returns
/// JSON response with all required containers and placed objects
#[utoipa::path(
    post,
    path = "/pack",
    request_body = PackRequest,
    responses(
        (status = 200, description = "Successfully packed objects", body = PackResponse),
        (
            status = UNPROCESSABLE_ENTITY,
            description = "Invalid request or container configuration",
            body = ErrorResponse
        )
    ),
    tag = "packing"
)]
async fn handle_pack(
    State(state): State<ApiState>,
    payload: Result<Json<PackRequest>, JsonRejection>,
) -> impl IntoResponse {
    let request = match parse_json_body(payload) {
        Ok(request) => request,
        Err(response) => return *response,
    };

    println!(
        "📥 New pack request: {} objects, {} packaging types",
        request.objects.len(),
        request.containers.len()
    );

    match run_pack(
        request,
        state.optimizer_config.packing_config(),
        state.limits,
    ) {
        Ok(response) => {
            println!(
                "📦 Result: {} containers, {} unpacked objects",
                response.results.len(),
                response.unplaced.len()
            );
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(err) => pack_validation_response(err),
    }
}

/// Handler for POST /pack_stream endpoint (SSE).
///
/// Streams pack events in real-time as Server-Sent Events (text/event-stream).
/// The frontend can visualize the steps live without waiting for the complete result.
#[utoipa::path(
    post,
    path = "/pack_stream",
    request_body = PackRequest,
    responses(
        (
            status = 200,
            description = "Streams pack events in real-time",
            content_type = "text/event-stream",
            body = String
        ),
        (
            status = UNPROCESSABLE_ENTITY,
            description = "Invalid request or container configuration",
            body = ErrorResponse
        )
    ),
    tag = "packing"
)]
async fn handle_pack_stream(
    State(state): State<ApiState>,
    payload: Result<Json<PackRequest>, JsonRejection>,
) -> impl IntoResponse {
    let request = match parse_json_body(payload) {
        Ok(request) => request,
        Err(response) => return *response,
    };

    let validated = match request.into_validated(state.limits) {
        Ok(validated) => validated,
        Err(err) => return pack_validation_response(err),
    };

    let (objects, container_blueprints, allow_rotations_override) = validated.into_parts();

    let (tx, rx) = mpsc::channel::<String>(32);

    let mut packing_config = state.optimizer_config.packing_config();
    if let Some(allow_rotations) = allow_rotations_override {
        packing_config.allow_item_rotation = allow_rotations;
    }

    tokio::task::spawn_blocking(move || {
        let _ = pack_objects_with_progress(objects, container_blueprints, packing_config, |evt| {
            if let Ok(json) = serde_json::to_string(evt) {
                // A send error means the receiver has closed the stream; remaining events
                // are simply discarded on subsequent callback invocations.
                let _ = tx.blocking_send(json);
            }
        });
    });

    let stream = ReceiverStream::new(rx)
        .map(|msg| Ok::<_, std::convert::Infallible>(Event::default().data(msg)));
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(10))
                .text("keep-alive"),
        )
        .into_response()
}

/// Serves the index.html main page
async fn serve_index() -> Response {
    match WebAssets::get("index.html") {
        Some(content) => Html(content.data).into_response(),
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

/// Serves static assets (JS, CSS, etc.)
async fn serve_static(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

async fn serve_openapi_json(State(_state): State<ApiState>) -> impl IntoResponse {
    Json(openapi_doc())
}

async fn serve_openapi_ui(State(_state): State<ApiState>) -> impl IntoResponse {
    Html(SWAGGER_UI_HTML)
}

/// Handler for GET /health.
///
/// Lightweight liveness probe that always returns `200 OK` while the process can serve requests.
#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "Service is healthy", body = HealthResponse)),
    tag = "system"
)]
async fn handle_health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse::healthy()))
}

/// Handler for GET /version.
///
/// Reports the crate name, semantic version, and description of the running build.
#[utoipa::path(
    get,
    path = "/version",
    responses((status = 200, description = "Build and version information", body = VersionResponse)),
    tag = "system"
)]
async fn handle_version() -> impl IntoResponse {
    (StatusCode::OK, Json(VersionResponse::current()))
}

/// Handler for GET /config.
///
/// Reports the active default packing configuration and the per-request guardrails so clients can
/// align their inputs with the server.
#[utoipa::path(
    get,
    path = "/config",
    responses((status = 200, description = "Active packing configuration", body = ConfigResponse)),
    tag = "system"
)]
async fn handle_config(State(state): State<ApiState>) -> impl IntoResponse {
    let response =
        ConfigResponse::from_parts(state.optimizer_config.packing_config(), state.limits);
    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_doc_lists_expected_paths() {
        let doc = openapi_doc();
        let paths = &doc.paths.paths;
        for expected in ["/pack", "/pack_stream", "/health", "/version"] {
            assert!(
                paths.contains_key(expected),
                "OpenAPI documentation is missing the {expected} path"
            );
        }
    }

    #[test]
    fn health_response_reports_ok() {
        let health = HealthResponse::healthy();
        assert_eq!(health.status, "ok");
        let json = serde_json::to_value(&health).expect("health serializes");
        assert_eq!(json["status"], "ok");
    }

    #[test]
    fn version_response_reflects_build_metadata() {
        let version = VersionResponse::current();
        assert_eq!(version.name, env!("CARGO_PKG_NAME"));
        assert_eq!(version.version, env!("CARGO_PKG_VERSION"));
        assert!(
            !version.version.is_empty(),
            "version string should never be empty"
        );
        assert!(
            !version.description.is_empty(),
            "Cargo.toml should define a package description"
        );
    }

    #[test]
    fn openapi_doc_contains_key_schemas() {
        let doc = openapi_doc();
        let components = doc
            .components
            .as_ref()
            .expect("OpenAPI documentation contains no components");
        let schemas = &components.schemas;
        for name in ["PackRequest", "PackResponse", "ErrorResponse"] {
            assert!(
                schemas.contains_key(name),
                "Expected schema '{}' is missing from OpenAPI spec",
                name
            );
        }
    }

    #[test]
    fn pack_request_parses_allow_rotations_when_present_true() {
        let json = r#"{
            "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
            "objects": [{"id": 1, "dims": [5.0, 5.0, 5.0], "weight": 10.0}],
            "allow_rotations": true
        }"#;
        let request: PackRequest = serde_json::from_str(json).expect("Should parse valid JSON");
        assert_eq!(
            request.allow_rotations,
            Some(true),
            "allow_rotations should be Some(true) when explicitly set to true"
        );
    }

    #[test]
    fn pack_request_parses_allow_rotations_when_present_false() {
        let json = r#"{
            "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
            "objects": [{"id": 1, "dims": [5.0, 5.0, 5.0], "weight": 10.0}],
            "allow_rotations": false
        }"#;
        let request: PackRequest = serde_json::from_str(json).expect("Should parse valid JSON");
        assert_eq!(
            request.allow_rotations,
            Some(false),
            "allow_rotations should be Some(false) when explicitly set to false"
        );
    }

    #[test]
    fn pack_request_parses_allow_rotations_when_absent() {
        let json = r#"{
            "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
            "objects": [{"id": 1, "dims": [5.0, 5.0, 5.0], "weight": 10.0}]
        }"#;
        let request: PackRequest = serde_json::from_str(json).expect("Should parse valid JSON");
        assert_eq!(
            request.allow_rotations, None,
            "allow_rotations should be None when field is omitted"
        );
    }

    #[test]
    fn pack_request_parses_allow_rotations_when_null() {
        let json = r#"{
            "containers": [{"dims": [10.0, 10.0, 10.0], "max_weight": 100.0}],
            "objects": [{"id": 1, "dims": [5.0, 5.0, 5.0], "weight": 10.0}],
            "allow_rotations": null
        }"#;
        let request: PackRequest = serde_json::from_str(json).expect("Should parse valid JSON");
        assert_eq!(
            request.allow_rotations, None,
            "allow_rotations should be None when field is explicitly null"
        );
    }

    #[test]
    fn validated_request_preserves_allow_rotations_value() {
        let request = PackRequest {
            containers: vec![ContainerRequest {
                name: Some("Test".to_string()),
                dims: (10.0, 10.0, 10.0),
                max_weight: 100.0,
            }],
            objects: vec![Box3D {
                id: 1,
                dims: (5.0, 5.0, 5.0),
                weight: 10.0,
            }],
            allow_rotations: Some(true),
        };

        let validated = request
            .into_validated(RequestLimits::default())
            .expect("Should validate successfully");
        assert_eq!(
            validated.allow_rotations,
            Some(true),
            "Validated request should preserve allow_rotations value"
        );
    }

    #[test]
    fn validation_rejects_too_many_objects() {
        let request = PackRequest {
            containers: vec![ContainerRequest {
                name: None,
                dims: (10.0, 10.0, 10.0),
                max_weight: 100.0,
            }],
            objects: vec![
                Box3D {
                    id: 1,
                    dims: (5.0, 5.0, 5.0),
                    weight: 10.0,
                },
                Box3D {
                    id: 2,
                    dims: (5.0, 5.0, 5.0),
                    weight: 10.0,
                },
            ],
            allow_rotations: None,
        };

        let limits = RequestLimits::with_limits(1, 10);
        let err = request
            .into_validated(limits)
            .expect_err("object count above the limit should be rejected");
        assert!(
            matches!(
                err,
                PackRequestValidationError::TooManyObjects { count: 2, max: 1 }
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn config_response_reflects_packing_config_and_limits() {
        let config = PackingConfig::default();
        let limits = RequestLimits::with_limits(42, 7);
        let response = ConfigResponse::from_parts(config, limits);
        assert_eq!(response.grid_step, PackingConfig::DEFAULT_GRID_STEP);
        assert_eq!(response.allow_item_rotation, config.allow_item_rotation);
        assert_eq!(response.max_objects, 42);
        assert_eq!(response.max_containers, 7);
    }

    #[test]
    fn request_level_allow_rotations_true_overrides_config() {
        // Create a config with rotations disabled
        let mut config = crate::optimizer::PackingConfig {
            allow_item_rotation: false,
            ..Default::default()
        };

        // Simulate request-level override
        let allow_rotations_override = Some(true);
        if let Some(allow_rotations) = allow_rotations_override {
            config.allow_item_rotation = allow_rotations;
        }

        assert!(
            config.allow_item_rotation,
            "Request-level allow_rotations=true should override config setting"
        );
    }

    #[test]
    fn request_level_allow_rotations_false_overrides_config() {
        // Create a config with rotations enabled
        let mut config = crate::optimizer::PackingConfig {
            allow_item_rotation: true,
            ..Default::default()
        };

        // Simulate request-level override
        let allow_rotations_override = Some(false);
        if let Some(allow_rotations) = allow_rotations_override {
            config.allow_item_rotation = allow_rotations;
        }

        assert!(
            !config.allow_item_rotation,
            "Request-level allow_rotations=false should override config setting"
        );
    }

    #[test]
    fn request_level_allow_rotations_none_preserves_config() {
        // Create a config with rotations disabled
        let mut config = crate::optimizer::PackingConfig {
            allow_item_rotation: false,
            ..Default::default()
        };

        // Simulate request-level override with None
        let allow_rotations_override: Option<bool> = None;
        if let Some(allow_rotations) = allow_rotations_override {
            config.allow_item_rotation = allow_rotations;
        }

        assert!(
            !config.allow_item_rotation,
            "When allow_rotations is None, config setting should be preserved"
        );

        // Now test with rotations enabled
        let mut config = crate::optimizer::PackingConfig {
            allow_item_rotation: true,
            ..Default::default()
        };

        if let Some(allow_rotations) = allow_rotations_override {
            config.allow_item_rotation = allow_rotations;
        }

        assert!(
            config.allow_item_rotation,
            "When allow_rotations is None, config setting should be preserved"
        );
    }
}
