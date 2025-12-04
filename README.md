# Sort-it-now - 3D Box Packing Optimizer

Eine intelligente 3D-Verpackungsoptimierung mit interaktiver Visualisierung.

## 🎯 Features

### Backend (Rust)

- **Heuristischer Packing-Algorithmus** mit Berücksichtigung von:
  - Gewichtsgrenzen und -verteilung
  - Stabilität und Unterstützung (60% Mindestauflage)
  - Schwerpunkt-Balance
  - Schichtung (schwere Objekte unten)
- **Automatische Multi-Container-Verwaltung**
- **Umfassende Unit-Tests**
- **REST-API** mit JSON-Kommunikation
- **OpenAPI & Swagger UI** mit live Dokumentation unter `/docs`
- **OOP-Prinzipien** mit DRY-Architektur
- **Vollständig dokumentierter Code** (Rust-Docstrings)

### Frontend (JavaScript/Three.js)

- **Interaktive 3D-Visualisierung**
- **OrbitControls** für Kamera-Steuerung
- **Container-Navigation** (Vor/Zurück-Buttons)
- **Schritt-für-Schritt-Animation** des Packprozesses
- **Live-Statistiken**:
  - Objekt-Anzahl
  - Gesamtgewicht
  - Volumen-Auslastung
  - Schwerpunkt-Position
- **Responsive Design**

## 🚀 Installation & Start

### Voraussetzungen

- Rust (1.70+)
- Cargo
- Moderner Webbrowser

### Backend starten

```bash
cargo run
```

Der Server läuft auf `http://localhost:8080`

> 💡 **Konfigurationshinweis:** Kopiere bei Bedarf die Datei `.env.example` nach `.env`, um den API-Port, Host oder Update-Parameter anzupassen. Nicht gesetzte Werte fallen automatisch auf ihre Standardwerte zurück.

### Frontend öffnen

Der Web-Client wird automatisch vom Rust-Backend ausgeliefert. Rufe nach dem Start einfach `http://localhost:8080/` im Browser auf.

Im Browser:

- Button "🚀 Pack (Batch)" führt eine einmalige Optimierung aus und zeigt das Ergebnis.
- Button "📡 Pack (Live)" startet den Live-Stream der Optimierungsschritte via SSE und rendert sie fortlaufend.

## 📦 Fertige Builds & Release-Pipeline

Für Releases existiert ein GitHub-Actions-Workflow (`.github/workflows/release.yml`), der bei Tags im Format `v*` (oder manuell via _workflow_dispatch_) Plattform-Pakete erzeugt:

- **Linux (x86_64)**: `sort-it-now-<version>-linux-x86_64.tar.gz`
- **macOS (ARM64/Apple Silicon)**: `sort-it-now-<version>-macos-arm64.tar.gz`
- **macOS (x86_64/Intel)**: `sort-it-now-<version>-macos-x86_64.tar.gz`
- **Windows (x86_64)**: `sort-it-now-<version>-windows-x86_64.zip`

Jedes Paket enthält die vorkompilierte Binärdatei, die aktuelle `README.md` sowie ein Installationsskript.
Die Artefakte werden sowohl als Workflow-Artefakte hochgeladen als auch automatisch dem GitHub-Release der entsprechenden Tag-Version hinzugefügt.

### Installationsskripte

- Linux/macOS: Im entpackten Ordner `./install.sh` ausführen (optional mit `sudo`), um `sort_it_now` nach `/usr/local/bin` zu kopieren.
- Windows: `install.ps1` (PowerShell) ausführen. Standardmäßig wird nach `%ProgramFiles%\sort-it-now` installiert und der Pfad der Benutzer-Umgebungsvariable hinzugefügt.

### Docker

Für jeden Release wird automatisch ein Docker-Image auf [Docker Hub](https://hub.docker.com/) veröffentlicht. Die Images werden für mehrere Architekturen (linux/amd64, linux/arm64) bereitgestellt.

> 📖 **Setup-Anleitung:** Siehe [DOCKER_SETUP.md](DOCKER_SETUP.md) für eine detaillierte Anleitung zur Einrichtung der Docker Hub Deployment-Pipeline.

**Docker Image ausführen:**

> **Hinweis:** Ersetze `<username>` durch `josunlp` (oder den entsprechenden Docker Hub Benutzernamen des Projekt-Maintainers).

```bash
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 <username>/sort-it-now:latest
```

**Mit Umgebungsvariablen:**

```bash
docker run -p 8080:8080 \
  -e SORT_IT_NOW_API_HOST=0.0.0.0 \
  -e SORT_IT_NOW_API_PORT=8080 \
  -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 \
  <username>/sort-it-now:latest
```

**Eigenes Image bauen:**

```bash
docker build -t sort-it-now .
docker run -p 8080:8080 sort-it-now
```

Der Server ist dann unter `http://localhost:8080` verfügbar.

## 🔔 Automatische Updates beim Start

Beim Start prüft der Dienst im Hintergrund die neuesten GitHub-Releases (`JosunLP/sort-it-now`). Wird eine neuere Version gefunden, lädt der Updater das passende Release-Paket herunter und führt das Installationsskript für die aktuelle Plattform aus. Dadurch wird das Update – soweit möglich – automatisch eingespielt. Auf Windows wird bei gesperrter `sort_it_now.exe` ersatzweise eine `sort_it_now.new.exe` abgelegt.

- Der Check kann über die Umgebungsvariable `SORT_IT_NOW_SKIP_UPDATE_CHECK=1` deaktiviert werden (z. B. für Offline-Installationen oder CI).
- GitHub limitiert nicht authentifizierte API-Aufrufe auf 60 pro Stunde. Wird das Limit erreicht, wird der Check übersprungen und eine Info ausgegeben. Setze optional `SORT_IT_NOW_GITHUB_TOKEN` (oder `GITHUB_TOKEN`) auf ein Personal Access Token, um höhere Limits zu erhalten; der Updater nutzt das Token ebenfalls beim Download der Release-Artefakte.
- Um unerwartet große Downloads zu vermeiden, begrenzt der Updater Release-Artefakte standardmäßig auf 200 MB. Passe das Limit über `SORT_IT_NOW_MAX_DOWNLOAD_MB` an (Wert `0` deaktiviert die Begrenzung).
- Repo/Owner sowie Timeout lassen sich über `SORT_IT_NOW_GITHUB_OWNER`, `SORT_IT_NOW_GITHUB_REPO` und `SORT_IT_NOW_HTTP_TIMEOUT_SECS` konfigurieren – Standardwerte greifen automatisch, falls keine `.env` vorhanden ist.

## 📊 API-Endpunkte

### OpenAPI & Swagger UI

- `GET /docs` liefert eine interaktive Swagger UI mit Subresource-Integrity-geschützten Assets.
- `GET /docs/openapi.json` stellt das OpenAPI-Schema (v3) bereit und kann z. B. für Code-Generatoren genutzt werden.

### POST /pack

Verpackt Objekte in Container.

**Request:**

```json
{
  "containers": [
    { "name": "Standard", "dims": [100.0, 100.0, 70.0], "max_weight": 500.0 },
    { "name": "Kompakt", "dims": [60.0, 80.0, 50.0], "max_weight": 320.0 }
  ],
  "objects": [
    { "id": 1, "dims": [30.0, 30.0, 10.0], "weight": 50.0 },
    { "id": 2, "dims": [20.0, 50.0, 15.0], "weight": 30.0 }
  ]
}
```

**Response:**

```json
{
  "results": [
    {
      "id": 1,
      "template_id": 0,
      "label": "Standard",
      "dims": [100.0, 100.0, 70.0],
      "max_weight": 500.0,
      "total_weight": 80.0,
      "placed": [
        {
          "id": 1,
          "pos": [0.0, 0.0, 0.0],
          "weight": 50.0,
          "dims": [30.0, 30.0, 10.0]
        }
      ]
    }
  ]
}
```

### POST /pack_stream (SSE)

Streamt Fortschritts-Events in Echtzeit als `text/event-stream`. Jeder Event ist ein JSON-Objekt mit `type`-Feld:

- `ContainerStarted` { id, dims, max_weight, label, template_id }
- `ObjectPlaced` { container_id, id, pos, weight, dims, total_weight }
- `Finished`

Hinweis: Im Frontend kannst du den Live-Modus mit dem Button "📡 Pack (Live)" starten.

## 🧪 Tests ausführen

```bash
cargo test
```

Alle 5 Tests sollten erfolgreich sein:

- ✅ heavy_boxes_stay_below_lighter
- ✅ single_box_snaps_to_corner
- ✅ creates_additional_containers_when_weight_exceeded
- ✅ reject_heavier_on_light_support
- ✅ sample_pack_respects_weight_order

## 🏗️ Architektur

### Rust Module

#### `main.rs`

- Einstiegspunkt der Anwendung
- Startet den Tokio-Runtime und API-Server

#### `model.rs`

- **`Box3D`**: Repräsentiert ein 3D-Objekt mit ID, Dimensionen und Gewicht
- **`PlacedBox`**: Objekt mit Position im Container
- **`Container`**: Verpackungsbehälter mit Kapazitätsgrenzen
- Methoden: `volume()`, `base_area()`, `total_weight()`, `remaining_weight()`, `utilization_percent()`

#### `geometry.rs`

- **`intersects()`**: AABB-Kollisionserkennung zwischen zwei Objekten
- **`overlap_1d()`**: Berechnet 1D-Überlappung
- **`overlap_area_xy()`**: Berechnet XY-Überlappungsfläche
- **`point_inside()`**: Punkt-in-Box-Test

#### `optimizer.rs`

- **`PackingConfig`**: Konfigurierbare Parameter (Raster, Support-Ratio, Toleranzen)
- **`pack_objects()`**: Hauptalgorithmus zur Verpackung
- **`pack_objects_with_config()`**: Version mit anpassbaren Parametern
- **`find_stable_position()`**: Findet stabile Position für ein Objekt
- **`supports_weight_correctly()`**: Prüft Gewichts-Hierarchie
- **`has_sufficient_support()`**: Prüft Mindestauflage
- **`calculate_balance_after()`**: Berechnet Schwerpunkt-Abweichung

#### `api.rs`

- **REST-API** mit Axum-Framework
- **CORS-Support** für Frontend-Kommunikation
- JSON-Serialisierung/Deserialisierung

### JavaScript Module

#### `script.js`

- Three.js Szenen-Setup
- OrbitControls für Kamera
- Funktionen:
  - `clearScene()`: Räumt Szene auf
  - `drawContainerFrame()`: Zeichnet Container-Wireframe
  - `drawBox()`: Rendert einzelnes Objekt
  - `visualizeContainer()`: Zeigt kompletten Container
  - `animateContainer()`: Schritt-für-Schritt-Animation
  - `updateStats()`: Aktualisiert Statistik-Panel
  - `fetchPacking()`: API-Kommunikation

## 🎨 Optimierungen

### DRY-Prinzip

- **`PackingConfig`-Struktur** statt verteilter Konstanten
- Wiederverwendbare Funktionen für Geometrie-Berechnungen
- Zentralisierte Fehlerbehandlung

### OOP-Prinzipien

- Klare Trennung von Datenmodellen und Logik
- Kapselung in Module
- Trait-Implementation für gemeinsames Verhalten

### Code-Dokumentation

- Rust-Docstrings für alle öffentlichen Funktionen
- JSDoc-Kommentare im Frontend
- Inline-Kommentare für komplexe Algorithmen

## 🔧 Konfiguration

### Backend-Konfiguration (.env)

Die Anwendung lädt beim Start optional eine `.env`-Datei (mittels [`dotenvy`](https://crates.io/crates/dotenvy)). Nicht gesetzte Variablen behalten ihre Standardwerte, sodass der Dienst auch ohne `.env` wie gewohnt läuft. Relevante Variablen:

| Variable                                    | Standard      | Beschreibung                                                                                                                                     |
| ------------------------------------------- | ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `SORT_IT_NOW_API_HOST`                      | `0.0.0.0`     | IP-Adresse, an die der HTTP-Server gebunden wird. Setze z. B. `127.0.0.1` für lokalen Zugriff.                                                   |
| `SORT_IT_NOW_API_PORT`                      | `8080`        | Port des API-Servers. Werte `0` werden verworfen.                                                                                                |
| `SORT_IT_NOW_GITHUB_OWNER`                  | `JosunLP`     | GitHub-Owner/Organisation, deren Releases für Updates abgefragt werden.                                                                          |
| `SORT_IT_NOW_GITHUB_REPO`                   | `sort-it-now` | Repository-Name für den Updater.                                                                                                                 |
| `SORT_IT_NOW_HTTP_TIMEOUT_SECS`             | `30`          | Timeout in Sekunden für GitHub-HTTP-Anfragen des Updaters.                                                                                       |
| `SORT_IT_NOW_MAX_DOWNLOAD_MB`               | `200`         | Maximale Größe eines Release-Assets (0 = unbegrenzt).                                                                                            |
| `SORT_IT_NOW_GITHUB_TOKEN` / `GITHUB_TOKEN` | –             | Optionales PAT für höhere GitHub-Rate-Limits und private Releases.                                                                               |
| `SORT_IT_NOW_SKIP_UPDATE_CHECK`             | –             | Wenn gesetzt (beliebiger Wert), wird der automatische Update-Check deaktiviert.                                                                  |
| `SORT_IT_NOW_PACKING_GRID_STEP`             | `5.0`         | ⚠️ Schrittweite des Positionsrasters; kleinere Werte liefern feinere Platzierung, verlangsamen aber und können zu instabilen Anordnungen führen. |
| `SORT_IT_NOW_PACKING_SUPPORT_RATIO`         | `0.6`         | ⚠️ Mindestauflage für stabile Stapel; niedrigere Werte erhöhen Kipp-Risiko.                                                                      |
| `SORT_IT_NOW_PACKING_HEIGHT_EPSILON`        | `1e-3`        | ⚠️ Toleranz für Höhenvergleiche; Werte zu groß oder klein beeinflussen Stabilitätschecks.                                                        |
| `SORT_IT_NOW_PACKING_GENERAL_EPSILON`       | `1e-6`        | ⚠️ Allgemeine numerische Toleranz; extreme Werte können zu falschen Kollisionsergebnissen führen.                                                |
| `SORT_IT_NOW_PACKING_BALANCE_LIMIT_RATIO`   | `0.45`        | ⚠️ Grenzwert für Schwerpunktabweichung; höhere Werte erlauben stärkere Schiefstellungen.                                                         |

Eine beispielhafte Datei findest du in `.env.example`.

### Packing-Parameter (optimizer.rs)

```rust
PackingConfig {
    grid_step: 5.0,              // Positions-Raster in Einheiten
    support_ratio: 0.6,          // 60% Mindestauflage
    height_epsilon: 1e-3,        // Höhen-Toleranz
    general_epsilon: 1e-6,       // Allgemeine Toleranz
    balance_limit_ratio: 0.45,   // Max. Schwerpunkt-Abweichung
}
```

### Frontend-Konfiguration (script.js)

```javascript
const CONTAINER_SIZE = [100, 100, 70];  // Container-Dimensionen
const COLOR_PALETTE = [...];            // Farben für Objekte
```

## 📈 Performance

- **Durchsatz**: ~100 Objekte/Sekunde
- **Speicher**: O(n) für n Objekte
- **Komplexität**: O(n × p × z) wobei:
  - n = Anzahl Objekte
  - p = Raster-Positionen
  - z = Z-Ebenen

## 🐛 Bekannte Einschränkungen

1. **Rotation**: Objekte werden nicht rotiert (Fixed Orientation)
2. **Dynamische Stabilität**: Keine physikalische Simulation
3. **Optimales Packing**: Heuristik, kein garantiertes Optimum
4. **Browser-Support**: Benötigt WebGL-Unterstützung

## 📝 Lizenz

Projektspezifisch - Siehe Lizenz-Datei.

## 🤝 Beitragen

1. Fork das Repository
2. Erstelle einen Feature-Branch
3. Commit deine Änderungen
4. Push zum Branch
5. Öffne einen Pull Request

## 📧 Kontakt

Bei Fragen oder Problemen öffne bitte ein Issue.

---

Entwickelt mit ❤️ in Rust & Three.js
