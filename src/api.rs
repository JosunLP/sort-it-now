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

use crate::config::{ApiConfig, OptimizerConfig};
use crate::model::{Box3D, Container, ContainerBlueprint, ValidationError};
use crate::optimizer::{
    ContainerDiagnostics, PackingDiagnosticsSummary, PackingResult, SupportDiagnostics,
    pack_objects_with_config, pack_objects_with_progress,
};

#[derive(Clone)]
struct ApiState {
    optimizer_config: OptimizerConfig,
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
    fn container_count(&self) -> usize {
        self.containers.len()
    }

    fn object_count(&self) -> usize {
        self.objects.len()
    }

    fn into_parts(self) -> (Vec<Box3D>, Vec<ContainerBlueprint>, Option<bool>) {
        (self.objects, self.containers, self.allow_rotations)
    }
}

#[derive(Debug)]
enum PackRequestValidationError {
    MissingContainers,
    InvalidContainer(ValidationError),
    InvalidObject(ValidationError),
}

impl PackRequest {
    fn into_validated(self) -> Result<ValidatedPackRequest, PackRequestValidationError> {
        if self.containers.is_empty() {
            return Err(PackRequestValidationError::MissingContainers);
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

fn parse_pack_request(
    payload: Result<Json<PackRequest>, JsonRejection>,
) -> Result<ValidatedPackRequest, Response> {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(err) => return Err(json_deserialize_error(err)),
    };

    match payload.into_validated() {
        Ok(validated) => Ok(validated),
        Err(PackRequestValidationError::MissingContainers) => Err(validation_error(
            "At least one packaging type must be specified",
        )),
        Err(PackRequestValidationError::InvalidContainer(err)) => {
            Err(container_config_error(err.to_string()))
        }
        Err(PackRequestValidationError::InvalidObject(err)) => {
            Err(validation_error(err.to_string()))
        }
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
                .zip(container_diagnostics.into_iter())
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

#[derive(OpenApi)]
#[openapi(
    paths(handle_pack, handle_pack_stream),
    components(
        schemas(
            PackRequest,
            ContainerRequest,
            PackResponse,
            PackedContainer,
            PackedObject,
            PackedUnplacedObject,
            ErrorResponse,
            Box3D,
            ContainerDiagnostics,
            SupportDiagnostics,
            PackingDiagnosticsSummary
        )
    ),
    tags((name = "packing", description = "Endpoints for packing optimization"))
)]
struct ApiDoc;

/// Starts the API server on port 8080.
///
/// Configures CORS for cross-origin requests from the frontend.
/// Blocks until the server is terminated.
pub async fn start_api_server(config: ApiConfig, optimizer_config: OptimizerConfig) {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(Any)
        .allow_headers(Any);

    let state = ApiState { optimizer_config };

    let app = Router::new()
        // API endpoints
        .route("/pack", post(handle_pack))
        .route("/pack_stream", post(handle_pack_stream))
        // API documentation
        .route("/docs/openapi.json", get(serve_openapi_json))
        .route("/docs", get(serve_openapi_ui))
        // Web-UI (embedded)
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_static))
        .layer(cors)
        .with_state(state);

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
    let request = match parse_pack_request(payload) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let object_count = request.object_count();
    let container_count = request.container_count();
    let (objects, container_blueprints, allow_rotations_override) = request.into_parts();

    println!(
        "📥 New pack request: {} objects, {} packaging types",
        object_count, container_count
    );
    let mut packing_config = state.optimizer_config.packing_config();
    if let Some(allow_rotations) = allow_rotations_override {
        packing_config.allow_item_rotation = allow_rotations;
    }
    let packing_result = pack_objects_with_config(objects, container_blueprints, packing_config);
    println!(
        "📦 Result: {} containers, {} unpacked objects",
        packing_result.container_count(),
        packing_result.unplaced_count()
    );

    let response = PackResponse::from_packing_result(packing_result);
    (StatusCode::OK, Json(response)).into_response()
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
    let request = match parse_pack_request(payload) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let (objects, container_blueprints, allow_rotations_override) = request.into_parts();

    let (tx, rx) = mpsc::channel::<String>(32);

    let mut packing_config = state.optimizer_config.packing_config();
    if let Some(allow_rotations) = allow_rotations_override {
        packing_config.allow_item_rotation = allow_rotations;
    }

    tokio::task::spawn_blocking(move || {
        let _ = pack_objects_with_progress(objects, container_blueprints, packing_config, |evt| {
            if let Ok(json) = serde_json::to_string(evt) {
                if tx.blocking_send(json).is_err() {
                    // Receiver has closed the stream; remaining events are discarded.
                    return;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_doc_lists_expected_paths() {
        let doc = openapi_doc();
        let paths = &doc.paths.paths;
        assert!(
            paths.contains_key("/pack"),
            "OpenAPI documentation is missing the /pack path"
        );
        assert!(
            paths.contains_key("/pack_stream"),
            "OpenAPI documentation is missing the /pack_stream path"
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
            .into_validated()
            .expect("Should validate successfully");
        assert_eq!(
            validated.allow_rotations,
            Some(true),
            "Validated request should preserve allow_rotations value"
        );
    }

    #[test]
    fn request_level_allow_rotations_true_overrides_config() {
        // Create a config with rotations disabled
        let mut config = crate::optimizer::PackingConfig::default();
        config.allow_item_rotation = false;

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
        let mut config = crate::optimizer::PackingConfig::default();
        config.allow_item_rotation = true;

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
        let mut config = crate::optimizer::PackingConfig::default();
        config.allow_item_rotation = false;

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
        let mut config = crate::optimizer::PackingConfig::default();
        config.allow_item_rotation = true;

        if let Some(allow_rotations) = allow_rotations_override {
            config.allow_item_rotation = allow_rotations;
        }

        assert!(
            config.allow_item_rotation,
            "When allow_rotations is None, config setting should be preserved"
        );
    }
}
