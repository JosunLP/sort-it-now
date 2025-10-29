//! REST-API für den Packing-Service.
//!
//! Bietet HTTP-Endpunkte zur Kommunikation mit dem Frontend.
//! Verwendet Axum als Web-Framework und unterstützt CORS.

use axum::extract::{Json, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};

use crate::config::{ApiConfig, OptimizerConfig};
use crate::model::{Box3D, ContainerBlueprint, ValidationError};
use crate::optimizer::{
    compute_container_diagnostics, pack_objects_with_config, pack_objects_with_progress,
    ContainerDiagnostics, PackingConfig, PackingResult,
};

#[derive(Clone)]
struct ApiState {
    optimizer_config: OptimizerConfig,
}

/// Embedded Web Assets (HTML, CSS, JS)
#[derive(RustEmbed)]
#[folder = "web/"]
struct WebAssets;

/// Request-Struktur für den Packing-Endpunkt.
///
/// `containers` enthält die möglichen Verpackungstypen, die kombiniert werden dürfen.
#[derive(Deserialize, Clone)]
pub struct ContainerRequest {
    pub name: Option<String>,
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
}

impl ContainerRequest {
    fn into_blueprint(self, id: usize) -> Result<ContainerBlueprint, ValidationError> {
        ContainerBlueprint::new(id, self.name, self.dims, self.max_weight)
    }
}

#[derive(Deserialize)]
pub struct PackRequest {
    pub containers: Vec<ContainerRequest>,
    pub objects: Vec<Box3D>,
}

impl PackRequest {
    /// Validiert die Request-Daten.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.containers.is_empty() {
            return Err(ValidationError::InvalidConfiguration(
                "Mindestens ein Verpackungstyp muss angegeben werden".to_string(),
            ));
        }

        // Validiere Container-Typen
        for (idx, cont) in self.containers.iter().enumerate() {
            ContainerBlueprint::new(idx, cont.name.clone(), cont.dims, cont.max_weight)?;
        }

        // Validiere alle Objekte
        for obj in &self.objects {
            Box3D::new(obj.id, obj.dims, obj.weight)?;
        }

        Ok(())
    }
}

/// Response-Struktur mit allen verpackten Containern.
///
/// # Felder
/// * `results` - Vector von Containern mit platzierten Objekten
#[derive(Serialize)]
pub struct PackResponse {
    pub results: Vec<PackedContainer>,
    pub unplaced: Vec<PackedUnplacedObject>,
    pub is_complete: bool,
}

/// Einzelner Container mit Metadaten und platzierten Objekten.
///
/// # Felder
/// * `id` - Container-Nummer (1-basiert)
/// * `total_weight` - Gesamtgewicht aller Objekte im Container
/// * `placed` - Liste der platzierten Objekte mit Positionen
#[derive(Serialize)]
pub struct PackedContainer {
    pub id: usize,
    pub template_id: Option<usize>,
    pub label: Option<String>,
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
#[derive(Serialize)]
pub struct PackedObject {
    pub id: usize,
    pub pos: (f64, f64, f64),
    pub weight: f64,
    pub dims: (f64, f64, f64),
}

#[derive(Serialize)]
pub struct PackedUnplacedObject {
    pub id: usize,
    pub weight: f64,
    pub dims: (f64, f64, f64),
    pub reason_code: String,
    pub reason: String,
}

impl PackResponse {
    /// Erstellt eine PackResponse aus einem PackingResult (DRY-Prinzip).
    pub fn from_packing_result(result: PackingResult, config: &PackingConfig) -> Self {
        let is_complete = result.is_complete();
        let containers = result.containers;
        let unplaced_entries = result.unplaced;

        Self {
            results: containers
                .into_iter()
                .enumerate()
                .map(|(i, cont)| {
                    let diagnostics = compute_container_diagnostics(&cont, config);
                    let total_weight = cont.total_weight();
                    let placed_objects = cont
                        .placed
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
                        template_id: cont.template_id,
                        label: cont.label.clone(),
                        dims: cont.dims,
                        max_weight: cont.max_weight,
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
        }
    }
}

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
async fn handle_pack(
    State(state): State<ApiState>,
    Json(payload): Json<PackRequest>,
) -> impl IntoResponse {
    // Validiere Eingabedaten
    if let Err(e) = payload.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Ungültige Eingabedaten",
                "details": e.to_string()
            })),
        )
            .into_response();
    }

    let PackRequest {
        containers,
        objects,
    } = payload;
    let container_blueprints = match containers
        .into_iter()
        .enumerate()
        .map(|(idx, spec)| spec.into_blueprint(idx))
        .collect::<Result<Vec<_>, ValidationError>>()
    {
        Ok(list) => list,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Ungültige Container-Konfiguration",
                    "details": e.to_string()
                })),
            )
                .into_response();
        }
    };

    println!(
        "📥 Neue Pack-Anfrage: {} Objekte, {} Verpackungstypen",
        objects.len(),
        container_blueprints.len()
    );
    let packing_config = state.optimizer_config.packing_config();
    let packing_result = pack_objects_with_config(objects, container_blueprints, packing_config);
    println!(
        "📦 Ergebnis: {} Container, {} unverpackte Objekte",
        packing_result.container_count(),
        packing_result.unplaced_count()
    );

    let response = PackResponse::from_packing_result(packing_result, &packing_config);
    (StatusCode::OK, Json(response)).into_response()
}

/// Handler für POST /pack_stream Endpunkt (SSE).
///
/// Streamt die Pack-Events in Echtzeit als Server-Sent Events (text/event-stream).
/// Das Frontend kann die Schritte live visualisieren, ohne auf das Gesamtergebnis zu warten.
async fn handle_pack_stream(
    State(state): State<ApiState>,
    Json(payload): Json<PackRequest>,
) -> impl IntoResponse {
    // Validiere Eingabedaten
    if let Err(e) = payload.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Ungültige Eingabedaten",
                "details": e.to_string()
            })),
        )
            .into_response();
    }

    let PackRequest {
        containers,
        objects,
    } = payload;
    let container_blueprints = match containers
        .into_iter()
        .enumerate()
        .map(|(idx, spec)| spec.into_blueprint(idx))
        .collect::<Result<Vec<_>, ValidationError>>()
    {
        Ok(list) => list,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Ungültige Container-Konfiguration",
                    "details": e.to_string()
                })),
            )
                .into_response();
        }
    };

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
