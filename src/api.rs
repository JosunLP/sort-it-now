//! REST-API für den Packing-Service.
//!
//! Bietet HTTP-Endpunkte zur Kommunikation mit dem Frontend.
//! Verwendet Axum als Web-Framework und unterstützt CORS.

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

use crate::config::{ApiConfig, ConfigPreset, OptimizerConfig};
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

/// Request-Struktur für den Packing-Endpunkt.
///
/// `containers` enthält die möglichen Verpackungstypen, die kombiniert werden dürfen.
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
        ]
    })
)]
pub struct PackRequest {
    pub containers: Vec<ContainerRequest>,
    pub objects: Vec<Box3D>,
}

#[derive(Debug)]
struct ValidatedPackRequest {
    containers: Vec<ContainerBlueprint>,
    objects: Vec<Box3D>,
}

impl ValidatedPackRequest {
    fn container_count(&self) -> usize {
        self.containers.len()
    }

    fn object_count(&self) -> usize {
        self.objects.len()
    }

    fn into_parts(self) -> (Vec<Box3D>, Vec<ContainerBlueprint>) {
        (self.objects, self.containers)
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
        })
    }
}

/// Response-Struktur mit allen verpackten Containern.
///
/// # Felder
/// * `results` - Vector von Containern mit platzierten Objekten
#[derive(Serialize, ToSchema)]
pub struct PackResponse {
    pub results: Vec<PackedContainer>,
    pub unplaced: Vec<PackedUnplacedObject>,
    pub is_complete: bool,
    pub diagnostics_summary: PackingDiagnosticsSummary,
}

/// Einzelner Container mit Metadaten und platzierten Objekten.
///
/// # Felder
/// * `id` - Container-Nummer (1-basiert)
/// * `total_weight` - Gesamtgewicht aller Objekte im Container
/// * `placed` - Liste der platzierten Objekte mit Positionen
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

/// Einzelnes platziertes Objekt in der Response.
///
/// # Felder
/// * `id` - Objekt-ID
/// * `pos` - Position (x, y, z) im Container
/// * `weight` - Gewicht in kg
/// * `dims` - Dimensionen (Breite, Tiefe, Höhe)
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
        "Ungültige JSON-Daten",
        err.to_string(),
    )
}

fn validation_error(details: impl Into<String>) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Ungültige Eingabedaten",
        details,
    )
}

fn container_config_error(details: impl Into<String>) -> Response {
    error_response(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Ungültige Container-Konfiguration",
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
            "Mindestens ein Verpackungstyp muss angegeben werden",
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
    /// Erstellt eine PackResponse aus einem PackingResult (DRY-Prinzip).
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
    paths(
        handle_pack,
        handle_pack_stream,
        handle_validate,
        handle_health,
        handle_config_presets
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
            Box3D,
            ContainerDiagnostics,
            SupportDiagnostics,
            PackingDiagnosticsSummary,
            HealthResponse,
            ValidationResponse,
            ConfigPresetResponse
        )
    ),
    tags(
        (name = "packing", description = "Endpunkte zur Verpackungsoptimierung"),
        (name = "monitoring", description = "Überwachung und Status"),
        (name = "configuration", description = "Konfigurations-Management")
    )
)]
struct ApiDoc;

/// Startet den API-Server auf Port 8080.
///
/// Konfiguriert CORS für Cross-Origin-Requests vom Frontend.
/// Blockiert bis der Server beendet wird.
pub async fn start_api_server(config: ApiConfig, optimizer_config: OptimizerConfig) {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_origin(Any)
        .allow_headers(Any);

    let state = ApiState { optimizer_config };

    let app = Router::new()
        // API-Endpunkte
        .route("/pack", post(handle_pack))
        .route("/pack_stream", post(handle_pack_stream))
        .route("/validate", post(handle_validate))
        .route("/health", get(handle_health))
        .route("/config/presets", get(handle_config_presets))
        // API-Dokumentation
        .route("/docs/openapi.json", get(serve_openapi_json))
        .route("/docs", get(serve_openapi_ui))
        // Web-UI (embedded)
        .route("/", get(serve_index))
        .route("/*path", get(serve_static))
        .layer(cors)
        .with_state(state);

    let addr = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            panic!("❌ Konnte API-Server nicht auf {} binden: {}", addr, err);
        }
    };

    let display_host = config.display_host().to_string();
    println!(
        "🚀 Server läuft auf http://{}:{}",
        display_host,
        config.port()
    );
    if config.binds_to_all_interfaces() && config.uses_default_host() {
        println!("💡 Lokaler Zugriff: http://localhost:{}", config.port());
    }
    println!("📦 API-Endpunkte:");
    println!("   - POST /pack");
    println!("   - POST /pack_stream");
    println!("   - POST /validate");
    println!("   - GET  /health");
    println!("   - GET  /config/presets");
    println!("📑 Dokumentation:");
    println!("   - GET /docs");
    println!("   - GET /docs/openapi.json");
    println!("🌐 Web-UI: http://{}:{}", display_host, config.port());

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("❌ API-Server wurde mit einem Fehler beendet: {err}");
    }
}

/// Handler für POST /pack Endpunkt.
///
/// Nimmt eine Liste von Objekten entgegen und verpackt sie optimal in Container.
///
/// # Parameter
/// * `payload` - JSON-Payload mit Container-Dimensionen und Objekten
///
/// # Rückgabewert
/// JSON-Response mit allen benötigten Containern und platzierten Objekten
#[utoipa::path(
    post,
    path = "/pack",
    request_body = PackRequest,
    responses(
        (status = 200, description = "Erfolgreiche Verpackung der Objekte", body = PackResponse),
        (
            status = UNPROCESSABLE_ENTITY,
            description = "Ungültige Anfrage oder Container-Konfiguration",
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
    let (objects, container_blueprints) = request.into_parts();

    println!(
        "📥 Neue Pack-Anfrage: {} Objekte, {} Verpackungstypen",
        object_count, container_count
    );
    let packing_config = state.optimizer_config.packing_config();
    let packing_result = pack_objects_with_config(objects, container_blueprints, packing_config);
    println!(
        "📦 Ergebnis: {} Container, {} unverpackte Objekte",
        packing_result.container_count(),
        packing_result.unplaced_count()
    );

    let response = PackResponse::from_packing_result(packing_result);
    (StatusCode::OK, Json(response)).into_response()
}

/// Handler für POST /pack_stream Endpunkt (SSE).
///
/// Streamt die Pack-Events in Echtzeit als Server-Sent Events (text/event-stream).
/// Das Frontend kann die Schritte live visualisieren, ohne auf das Gesamtergebnis zu warten.
#[utoipa::path(
    post,
    path = "/pack_stream",
    request_body = PackRequest,
    responses(
        (
            status = 200,
            description = "Streamt Pack-Events in Echtzeit",
            content_type = "text/event-stream",
            body = String
        ),
        (
            status = UNPROCESSABLE_ENTITY,
            description = "Ungültige Anfrage oder Container-Konfiguration",
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

    let (objects, container_blueprints) = request.into_parts();

    let (tx, rx) = mpsc::channel::<String>(32);

    let packing_config = state.optimizer_config.packing_config();

    tokio::task::spawn_blocking(move || {
        let _ = pack_objects_with_progress(objects, container_blueprints, packing_config, |evt| {
            if let Ok(json) = serde_json::to_string(evt) {
                if tx.blocking_send(json).is_err() {
                    // Empfänger hat den Stream geschlossen; verbleibende Events werden verworfen.
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

/// Serviert die index.html Hauptseite
async fn serve_index() -> Response {
    match WebAssets::get("index.html") {
        Some(content) => Html(content.data).into_response(),
        None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

/// Serviert statische Assets (JS, CSS, etc.)
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

/// Handler für GET /health Endpunkt.
///
/// Gibt den Gesundheitsstatus des Services zurück.
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service ist bereit", body = HealthResponse)
    ),
    tag = "monitoring"
)]
async fn handle_health() -> impl IntoResponse {
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        service: "sort-it-now".to_string(),
    };
    (StatusCode::OK, Json(response)).into_response()
}

/// Response-Struktur für den Health-Endpunkt.
#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub service: String,
}

/// Handler für POST /validate Endpunkt.
///
/// Validiert die Eingabedaten ohne das eigentliche Packing durchzuführen.
#[utoipa::path(
    post,
    path = "/validate",
    request_body = PackRequest,
    responses(
        (status = 200, description = "Eingabe ist gültig", body = ValidationResponse),
        (
            status = UNPROCESSABLE_ENTITY,
            description = "Ungültige Eingabe",
            body = ErrorResponse
        )
    ),
    tag = "packing"
)]
async fn handle_validate(
    payload: Result<Json<PackRequest>, JsonRejection>,
) -> impl IntoResponse {
    let request = match parse_pack_request(payload) {
        Ok(request) => request,
        Err(response) => return response,
    };

    let response = ValidationResponse {
        valid: true,
        container_count: request.container_count(),
        object_count: request.object_count(),
        message: "Eingabe erfolgreich validiert".to_string(),
    };
    (StatusCode::OK, Json(response)).into_response()
}

/// Response-Struktur für den Validate-Endpunkt.
#[derive(Serialize, ToSchema)]
pub struct ValidationResponse {
    pub valid: bool,
    pub container_count: usize,
    pub object_count: usize,
    pub message: String,
}

/// Handler für GET /config/presets Endpunkt.
///
/// Gibt vordefinierte Konfigurationspresets zurück.
#[utoipa::path(
    get,
    path = "/config/presets",
    responses(
        (status = 200, description = "Liste der verfügbaren Presets", body = Vec<ConfigPresetResponse>)
    ),
    tag = "configuration"
)]
async fn handle_config_presets() -> impl IntoResponse {
    let presets: Vec<ConfigPresetResponse> = OptimizerConfig::presets()
        .into_iter()
        .map(|preset| ConfigPresetResponse {
            name: preset.name,
            description: preset.description,
        })
        .collect();

    (StatusCode::OK, Json(presets)).into_response()
}

/// Response-Struktur für Config-Presets.
#[derive(Serialize, ToSchema)]
pub struct ConfigPresetResponse {
    pub name: String,
    pub description: String,
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
            "OpenAPI-Dokumentation fehlt der /pack Pfad"
        );
        assert!(
            paths.contains_key("/pack_stream"),
            "OpenAPI-Dokumentation fehlt der /pack_stream Pfad"
        );
        assert!(
            paths.contains_key("/validate"),
            "OpenAPI-Dokumentation fehlt der /validate Pfad"
        );
        assert!(
            paths.contains_key("/health"),
            "OpenAPI-Dokumentation fehlt der /health Pfad"
        );
        assert!(
            paths.contains_key("/config/presets"),
            "OpenAPI-Dokumentation fehlt der /config/presets Pfad"
        );
    }

    #[test]
    fn openapi_doc_contains_key_schemas() {
        let doc = openapi_doc();
        let components = doc
            .components
            .as_ref()
            .expect("OpenAPI-Dokumentation enthält keine Components");
        let schemas = &components.schemas;
        for name in [
            "PackRequest",
            "PackResponse",
            "ErrorResponse",
            "HealthResponse",
            "ValidationResponse",
            "ConfigPresetResponse",
        ] {
            assert!(
                schemas.contains_key(name),
                "Erwartetes Schema '{}' fehlt im OpenAPI-Spec",
                name
            );
        }
    }

    #[test]
    fn config_presets_are_available() {
        let presets = OptimizerConfig::presets();
        assert!(!presets.is_empty(), "Es sollten Presets vorhanden sein");

        let preset_names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
        assert!(
            preset_names.contains(&"default"),
            "Default-Preset sollte vorhanden sein"
        );
        assert!(
            preset_names.contains(&"precision"),
            "Precision-Preset sollte vorhanden sein"
        );
        assert!(
            preset_names.contains(&"fast"),
            "Fast-Preset sollte vorhanden sein"
        );
    }

    #[test]
    fn preset_can_be_loaded_by_name() {
        let config = OptimizerConfig::from_preset("precision");
        assert!(config.is_some(), "Precision-Preset sollte ladbar sein");

        let config = OptimizerConfig::from_preset("nonexistent");
        assert!(
            config.is_none(),
            "Nicht-existentes Preset sollte None zurückgeben"
        );
    }
}
