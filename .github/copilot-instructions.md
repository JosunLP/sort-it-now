# Copilot Instructions für sort-it-now

## Projektübersicht

**Sort-it-now** ist ein 3D-Verpackungsoptimierungs-Service in Rust mit Web-Frontend. Er löst das Bin-Packing-Problem: Quader effizient in Container packen unter Berücksichtigung von Gewicht, Stabilität und Schwerpunkt.

## Architektur

```
src/
├── main.rs        # Tokio-Runtime & Server-Start, lädt .env via dotenvy
├── config.rs      # Umgebungsvariablen → AppConfig/ApiConfig/OptimizerConfig/UpdateConfig
├── model.rs       # Datenstrukturen: Box3D, PlacedBox, Container, ContainerBlueprint
├── geometry.rs    # AABB-Kollision (intersects), Überlappung (overlap_1d), point_inside
├── optimizer.rs   # Packing-Algorithmus mit PackingConfig (1700+ Zeilen, inkl. Tests)
├── api.rs         # Axum REST-API: /pack, /pack_stream (SSE), /docs (Swagger UI)
└── update.rs      # Auto-Update via GitHub Releases (plattformspezifisch)
web/
├── index.html     # Frontend-Einstieg
└── script.js      # Three.js 3D-Visualisierung mit OrbitControls
```

## Entwickler-Workflow

```bash
# Server starten (Port 8080)
cargo run

# Tests ausführen
cargo test

# Formatierung & Linting (CI-Check - muss vor PR passen!)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

---

## Packing-Algorithmus (Detaillierte Kernlogik)

### Hauptfunktionen in `optimizer.rs`

| Funktion                       | Zweck                                                 |
| ------------------------------ | ----------------------------------------------------- |
| `pack_objects()`               | Einstiegspunkt mit Default-Config                     |
| `pack_objects_with_config()`   | Mit anpassbarer `PackingConfig`                       |
| `pack_objects_with_progress()` | Mit Callback für Live-Events (SSE)                    |
| `find_stable_position()`       | Findet optimale Position via Rastersuche + Z-Ebenen   |
| `supports_weight_correctly()`  | Prüft Gewichtshierarchie (schwere UNTER leichten)     |
| `has_sufficient_support()`     | Prüft Mindestauflage via `support_ratio_of()`         |
| `is_center_supported()`        | Verhindert Überhänge (Schwerpunkt muss gestützt sein) |
| `calculate_balance_after()`    | Berechnet Schwerpunktabweichung                       |

### Algorithmus-Ablauf

1. **Sortierung**: Objekte nach `weight * volume` absteigend (schwere/große zuerst)
2. **Clustering**: `FootprintClusterStrategy` gruppiert Objekte mit ähnlicher Grundfläche
3. **Orientierungen**: Bei `allow_item_rotation=true` → 6 Permutationen (dedupliziert)
4. **Positionssuche**:
   - Iteriere Z-Ebenen (Boden + Oberseiten aller platzierten Objekte)
   - Raster auf X/Y-Achse mit `grid_step`
   - Bewerte jede Position nach `PlacementScore { z, y, x, balance }`
5. **Stabilitätsprüfungen** (alle müssen bestanden werden):
   - Keine Kollision (`intersects()`)
   - Mindestauflage (`support_ratio >= 60%`)
   - Gewichtshierarchie (kein schweres auf leichtem Objekt)
   - Schwerpunkt gestützt (`is_center_supported()`)
   - Balance innerhalb `balance_limit_ratio`
6. **Multi-Container**: Wenn kein Platz → neuer Container aus Template-Pool

### Konfiguration via Builder-Pattern

```rust
PackingConfig::builder()
    .grid_step(2.5)                    // Feineres Raster (langsamer)
    .support_ratio(0.7)                // 70% Mindestauflage
    .height_epsilon(1e-3)              // Toleranz für Höhenvergleiche
    .general_epsilon(1e-6)             // Allgemeine Float-Toleranz
    .balance_limit_ratio(0.45)         // Max. Schwerpunktabweichung
    .footprint_cluster_tolerance(0.15) // Clustering-Toleranz
    .allow_item_rotation(true)         // 90°-Rotationen
    .build()
```

### Diagnose-Strukturen

```rust
// Pro Container
ContainerDiagnostics {
    center_of_mass_offset: f64,      // Schwerpunkt-Distanz zur Mitte
    balance_limit: f64,              // Erlaubte Abweichung
    imbalance_ratio: f64,            // offset / limit
    average_support_percent: f64,    // Durchschnittliche Auflage
    minimum_support_percent: f64,    // Schlechteste Auflage
    support_samples: Vec<SupportDiagnostics>,
}

// Aggregiert
PackingDiagnosticsSummary {
    max_imbalance_ratio: f64,
    worst_support_percent: f64,
    average_support_percent: f64,
}
```

### Nicht-Platzierbare Objekte

```rust
enum UnplacedReason {
    TooHeavyForContainer,        // Überschreitet max_weight aller Templates
    DimensionsExceedContainer,   // Passt in keine Orientierung
    NoStablePosition,            // Keine stabile Position gefunden
}
```

---

## Error-Handling-Patterns

### ValidationError in `model.rs`

Alle Konstruktoren prüfen Eingaben und geben `Result<T, ValidationError>` zurück:

```rust
pub enum ValidationError {
    InvalidDimension(String),      // Nicht-positiv, NaN oder Infinite
    InvalidWeight(String),         // Nicht-positiv, NaN oder Infinite
    InvalidConfiguration(String),  // Reserviert für Konfig-Fehler
}

// Beispiel: Box3D::new() prüft
Box3D::new(id, (w, d, h), weight)?  // Fehler bei w <= 0, NaN, Inf

// Container-Blueprint prüft analog
ContainerBlueprint::new(id, name, dims, max_weight)?
```

### API-Validierung in `api.rs`

```rust
enum PackRequestValidationError {
    MissingContainers,              // Leere Container-Liste
    InvalidContainer(ValidationError),
    InvalidObject(ValidationError),
}

// Konvertierung zu HTTP-Response
impl IntoResponse for PackRequestValidationError { ... }
```

---

## Frontend-Integration (`script.js`)

### SSE-Events für Live-Visualisierung

```javascript
// EventSource für /pack_stream
const es = new EventSource('/pack_stream', { method: 'POST', body: ... });

es.onmessage = (event) => {
  const data = JSON.parse(event.data);
  switch (data.type) {
    case 'ContainerStarted':
      // { id, dims, max_weight, label, template_id }
      break;
    case 'ObjectPlaced':
      // { container_id, id, pos, weight, dims, total_weight }
      break;
    case 'ContainerDiagnostics':
      // { container_id, diagnostics }
      break;
    case 'ObjectRejected':
      // { id, weight, dims, reason_code, reason_text }
      break;
    case 'Finished':
      // { containers, unplaced, diagnostics_summary }
      es.close();
      break;
  }
};
```

### Epsilon-Konstanten (Backend-kompatibel)

```javascript
const EPSILON_COMPARISON = 1e-6;      // Dimensionsvergleiche
const EPSILON_DEDUPLICATION = 1e-6;   // Exakte Gleichheit

// Verwendung für Rotations-Deduplizierung
function dimsAlmostEqual(a, b, epsilon = EPSILON_DEDUPLICATION) {
  return Math.abs(a[0] - b[0]) <= epsilon && ...;
}
```

### Three.js-Setup

```javascript
import * as THREE from 'https://esm.sh/three@0.163.0';
import { OrbitControls } from 'https://esm.sh/three@0.163.0/examples/jsm/controls/OrbitControls.js';

// Kernfunktionen
clearScene(); // Entfernt alle Meshes/LineSegments
drawContainerFrame(); // Wireframe + Grid
drawBox(); // Objekt-Mesh mit Farbe + Opacity
visualizeContainer(); // Komplette Container-Darstellung
animateContainer(); // Schritt-für-Schritt-Animation
updateStats(); // Statistik-Panel mit Diagnostik
```

---

## Auto-Update-Mechanismus (`update.rs`)

### Ablauf

1. **Start**: `check_for_updates_background()` spawnt Tokio-Task
2. **GitHub API**: Ruft `/repos/{owner}/{repo}/releases/latest` ab
3. **Versionsvergleich**: `semver::Version` → Update nur bei `latest > current`
4. **Download**: Plattform-spezifisches Asset (tar.gz/zip)
5. **Verifikation**: SHA-256 Checksumme aus `.sha256`-Datei
6. **Installation**: Plattform-spezifische Logik:
   - Linux/macOS: `install-unix.sh` ausführen
   - Windows: Binary ersetzen (oder `.new.exe` bei Sperre)

### Konfiguration

| Variable            | Default | Beschreibung                           |
| ------------------- | ------- | -------------------------------------- |
| `SKIP_UPDATE_CHECK` | -       | Update komplett deaktivieren           |
| `GITHUB_TOKEN`      | -       | Für höhere Rate-Limits                 |
| `MAX_DOWNLOAD_MB`   | 200     | Limit für Asset-Größe (0 = unbegrenzt) |
| `HTTP_TIMEOUT_SECS` | 30      | Timeout für GitHub-Requests            |

### Rate-Limiting

```rust
// Automatische Erkennung + Hinweis
if is_rate_limit_response(&headers) {
    println!("⏱️ GitHub-Rate-Limit erreicht...");
    if token.is_none() {
        println!("💡 Tipp: Setze GITHUB_TOKEN...");
    }
}
```

---

## API-Endpunkte

| Methode | Pfad                 | Beschreibung                      |
| ------- | -------------------- | --------------------------------- |
| `POST`  | `/pack`              | Batch-Verpackung → `PackResponse` |
| `POST`  | `/pack_stream`       | SSE-Stream mit `PackEvent`s       |
| `GET`   | `/docs`              | Swagger UI (SRI-geschützt)        |
| `GET`   | `/docs/openapi.json` | OpenAPI 3 Schema                  |

### Request-Format

```json
{
  "containers": [
    { "name": "Standard", "dims": [100.0, 100.0, 70.0], "max_weight": 500.0 }
  ],
  "objects": [{ "id": 1, "dims": [30.0, 30.0, 10.0], "weight": 50.0 }],
  "allow_rotations": true
}
```

---

## Wichtige Konventionen

### Rust-spezifisch

- **Docstrings**: Alle öffentlichen Funktionen/Structs auf Deutsch dokumentieren
- **Validierung**: Immer `Result<T, ValidationError>` für Konstruktoren
- **Builder-Pattern**: `PackingConfig::builder()` für Konfiguration
- **Epsilon-Konstanten**: Konsistent `1e-6` (general) / `1e-3` (height) verwenden
- **Plattform-Kompilierung**: `#[cfg(target_os = "...")]` für OS-spezifischen Code

### Tests

- Tests in `#[cfg(test)]`-Modulen am Ende jeder Datei
- Haupttests in `optimizer.rs` (15+ Tests):
  - `heavy_boxes_stay_below_lighter` - Gewichtshierarchie
  - `single_box_snaps_to_corner` - Positionierung
  - `creates_additional_containers_when_weight_exceeded` - Multi-Container
  - `reject_heavier_on_light_support` - Stabilitätsregeln
- Hilfs-Funktion `assert_heavy_below()` prüft Gewichtssortierung über alle Schichten

### Frontend

- ESM-Imports von `esm.sh` für Three.js
- `config`-Objekt für Container/Objekte/Rotationen
- Validierung via `collectConfigIssues()` + `ensureConfigValidOrNotify()`

---

## Geometrie-Funktionen (`geometry.rs`)

### AABB-Kollisionserkennung

```rust
/// Separating Axis Theorem: Objekte überschneiden sich NICHT,
/// wenn sie in mindestens einer Achse vollständig getrennt sind.
pub fn intersects(a: &PlacedBox, b: &PlacedBox) -> bool {
    !(ax + aw <= bx || bx + bw <= ax ||  // X-Achse getrennt
      ay + ad <= by || by + bd <= ay ||  // Y-Achse getrennt
      az + ah <= bz || bz + bh <= az)    // Z-Achse getrennt
}
```

### Überlappungs-Berechnung

```rust
/// Berechnet 1D-Überlappung zweier Intervalle
/// Beispiel: overlap_1d(0.0, 5.0, 3.0, 8.0) → 2.0
pub fn overlap_1d(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    (a2.min(b2) - a1.max(b1)).max(0.0)
}

/// Überlappungsfläche in XY-Ebene (für Support-Berechnung)
pub fn overlap_area_xy(a: &PlacedBox, b: &PlacedBox) -> f64 {
    overlap_1d(...) * overlap_1d(...)  // X × Y
}
```

### Punkt-in-Box-Test

```rust
/// Prüft, ob Schwerpunkt-Projektion von stützender Box getragen wird
pub fn point_inside(point: (f64, f64, f64), placed_box: &PlacedBox) -> bool {
    px >= bx && px <= bx + bw &&  // X innerhalb
    py >= by && py <= by + bd &&  // Y innerhalb
    pz >= bz && pz <= bz + bh     // Z innerhalb
}
```

---

## Test-Patterns

### Struktur der Tests in `optimizer.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Hilfsfunktion: Einzelnes Container-Template erstellen
    fn single_blueprint(dims: (f64, f64, f64), max_weight: f64) -> Vec<ContainerBlueprint> {
        vec![ContainerBlueprint::new(0, None, dims, max_weight).unwrap()]
    }

    // Hilfsfunktion: Gewichtshierarchie über alle Schichten prüfen
    fn assert_heavy_below(cont: &Container, config: &PackingConfig) {
        for lower in &cont.placed {
            for upper in &cont.placed {
                // Prüft: Objekt direkt drüber muss leichter sein
                if overlap_exists && upper_above_lower {
                    assert!(lower.weight >= upper.weight);
                }
            }
        }
    }
}
```

### Kerntest-Kategorien

| Test                                                 | Prüft                                       |
| ---------------------------------------------------- | ------------------------------------------- |
| `heavy_boxes_stay_below_lighter`                     | Gewichtssortierung vertikal                 |
| `single_box_snaps_to_corner`                         | Platzierung bei (0,0,0)                     |
| `creates_additional_containers_when_weight_exceeded` | Multi-Container-Logik                       |
| `reports_objects_too_large_for_container`            | `UnplacedReason::DimensionsExceedContainer` |
| `reports_objects_too_heavy_for_container`            | `UnplacedReason::TooHeavyForContainer`      |
| `reject_heavier_on_light_support`                    | Stabilität: Schwer auf leicht verboten      |
| `rotation_toggle_controls_reorientation`             | `allow_item_rotation` Effekt                |
| `orientation_deduplication_handles_equal_dimensions` | Würfel → 1, Quader → 3-6 Orientierungen     |
| `footprint_cluster_groups_similar_dimensions`        | Clustering-Strategie                        |
| `diagnostics_capture_support_and_balance_metrics`    | Diagnosewerte korrekt                       |
| `progress_emits_diagnostics_events`                  | SSE-Events werden emittiert                 |

### Test ausführen

```bash
# Alle Tests
cargo test

# Einzelner Test mit Output
cargo test heavy_boxes_stay_below_lighter -- --nocapture

# Tests mit Pattern
cargo test rotation
```

---

## Performance-Hinweise

### Algorithmus-Komplexität

| Faktor                | Einfluss                                         |
| --------------------- | ------------------------------------------------ |
| `grid_step`           | Kleiner → mehr Positionen → langsamer, genauer   |
| Objektanzahl (n)      | O(n × p × z) wobei p = Positionen, z = Z-Ebenen  |
| `allow_item_rotation` | 6× mehr Orientierungen → 6× mehr Prüfungen       |
| Container-Templates   | Jedes Template wird bei neuen Containern geprüft |

### Empfohlene Einstellungen

```rust
// Schnell (Prototyping)
PackingConfig::builder()
    .grid_step(10.0)
    .support_ratio(0.5)
    .build()

// Präzise (Produktion)
PackingConfig::builder()
    .grid_step(2.5)
    .support_ratio(0.7)
    .allow_item_rotation(true)
    .build()
```

### Speicher

- O(n) für n Objekte
- `PlacedBox` enthält Klon von `Box3D` → Moderater Overhead
- SSE-Streaming reduziert Peak-Speicher bei großen Anfragen

---

## Umgebungsvariablen

Alle mit Prefix `SORT_IT_NOW_`:

| Variable                  | Default   | Beschreibung             |
| ------------------------- | --------- | ------------------------ |
| `API_HOST`                | `0.0.0.0` | Server-Bind-IP           |
| `API_PORT`                | `8080`    | Server-Port              |
| `PACKING_GRID_STEP`       | `5.0`     | Raster-Schrittweite      |
| `PACKING_SUPPORT_RATIO`   | `0.6`     | Mindestauflage (0-1)     |
| `PACKING_ALLOW_ROTATIONS` | `false`   | 90°-Rotationen           |
| `SKIP_UPDATE_CHECK`       | -         | Auto-Update deaktivieren |
| `GITHUB_TOKEN`            | -         | Für höhere Rate-Limits   |

---

## Docker

```bash
# Fertiges Image
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 josunlp/sort-it-now:latest

# Eigenes Image bauen
docker build -t sort-it-now .
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 sort-it-now
```

---

## CI/CD Workflows

| Workflow      | Trigger          | Aktionen                                 |
| ------------- | ---------------- | ---------------------------------------- |
| `rust.yml`    | Push/PR auf main | Format + Clippy + Tests (ubuntu/windows) |
| `release.yml` | Tag `v*`         | Plattform-Pakete (Linux/macOS/Windows)   |
| `docker.yml`  | Tag `v*`         | Multi-Arch Images auf Docker Hub         |
| `codeql.yml`  | Push             | Security-Analyse                         |
| `stale.yml`   | Schedule         | Alte Issues/PRs markieren                |
