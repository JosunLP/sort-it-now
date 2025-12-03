# Sort-it-now - TypeScript/Bun Version

Eine vollständige TypeScript-Implementierung des 3D Box Packing Optimizers mit Bun Runtime.

## 🎯 Features

- **Vollständige TypeScript-Implementierung** des Packing-Algorithmus
- **Bun Runtime** für maximale Performance
- **Single File Executable** - kompiliert zu einer einzigen ausführbaren Datei
- **REST API** mit JSON-Kommunikation
- **Server-Sent Events (SSE)** für Live-Visualisierung
- **Heuristischer Packing-Algorithmus** mit Berücksichtigung von:
  - Gewichtsgrenzen und -verteilung
  - Stabilität und Unterstützung (60% Mindestauflage)
  - Schwerpunkt-Balance
  - Schichtung (schwere Objekte unten)
- **CORS-Support** für Frontend-Integration

## 🚀 Installation & Start

### Voraussetzungen

- Bun (1.0+)

### Installation von Bun

```bash
curl -fsSL https://bun.sh/install | bash
```

### Development Mode

```bash
# Direkt ausführen
bun run ts-src/index.ts

# Mit Auto-Reload
bun run dev

# Beispiele ausführen
bun run example
```

### Produktions-Build

```bash
# Single File Executable erstellen
bun run build

# Executable ausführen
./sort-it-now
```

Der Server läuft auf `http://localhost:8080`

## 📦 Single File Executable

Das Projekt kann mit Bun zu einem einzigen ausführbaren Programm kompiliert werden:

```bash
bun build ts-src/index.ts --compile --outfile sort-it-now
```

Dies erstellt eine eigenständige Binary (~100MB), die ohne installiertes Bun ausgeführt werden kann:

```bash
./sort-it-now
```

Die Binary enthält:
- Bun Runtime
- Kompletten TypeScript Code
- Alle Dependencies

## 📊 API-Endpunkte

### GET /

Zeigt eine Info-Seite mit API-Übersicht.

### POST /pack

Verpackt Objekte in Container (Batch-Modus).

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
  ],
  "unplaced": [],
  "diagnostics_summary": {
    "maxImbalanceRatio": 0.0,
    "worstSupportPercent": 100.0,
    "averageSupportPercent": 100.0
  }
}
```

### POST /pack_stream (SSE)

Streamt Fortschritts-Events in Echtzeit als `text/event-stream`. Jeder Event ist ein JSON-Objekt mit `type`-Feld:

- `ContainerStarted` { id, dims, maxWeight, label, templateId }
- `ObjectPlaced` { containerId, id, pos, weight, dims, totalWeight }
- `ContainerDiagnostics` { containerId, diagnostics }
- `ObjectRejected` { id, weight, dims, reasonCode, reasonText }
- `Finished` { containers, unplaced, diagnosticsSummary }

### GET /docs

Liefert API-Dokumentation im OpenAPI-Format.

## 🔧 Konfiguration

Die Anwendung lädt beim Start optional Umgebungsvariablen. Nicht gesetzte Variablen behalten ihre Standardwerte.

### API-Konfiguration

| Variable                  | Standard  | Beschreibung                                          |
| ------------------------- | --------- | ----------------------------------------------------- |
| `SORT_IT_NOW_API_HOST`    | `0.0.0.0` | IP-Adresse, an die der HTTP-Server gebunden wird     |
| `SORT_IT_NOW_API_PORT`    | `8080`    | Port des API-Servers                                  |

### Packing-Parameter

| Variable                                      | Standard | Beschreibung                                                       |
| --------------------------------------------- | -------- | ------------------------------------------------------------------ |
| `SORT_IT_NOW_PACKING_GRID_STEP`               | `5.0`    | Schrittweite des Positionsrasters                                  |
| `SORT_IT_NOW_PACKING_SUPPORT_RATIO`           | `0.6`    | Mindestauflage für stabile Stapel (60%)                            |
| `SORT_IT_NOW_PACKING_HEIGHT_EPSILON`          | `1e-3`   | Toleranz für Höhenvergleiche                                       |
| `SORT_IT_NOW_PACKING_GENERAL_EPSILON`         | `1e-6`   | Allgemeine numerische Toleranz                                     |
| `SORT_IT_NOW_PACKING_BALANCE_LIMIT_RATIO`     | `0.45`   | Grenzwert für Schwerpunktabweichung                                |
| `SORT_IT_NOW_PACKING_FOOTPRINT_CLUSTER_TOLERANCE` | `0.15` | Relative Toleranz bei der Vorgruppierung nach Grundfläche |

Beispiel `.env` Datei:

```env
SORT_IT_NOW_API_HOST=127.0.0.1
SORT_IT_NOW_API_PORT=3000
SORT_IT_NOW_PACKING_GRID_STEP=10.0
```

## 🏗️ Architektur

### TypeScript Module

#### `ts-src/model.ts`

- **Box3D**: Repräsentiert ein 3D-Objekt mit ID, Dimensionen und Gewicht
- **PlacedBox**: Objekt mit Position im Container
- **Container**: Verpackungsbehälter mit Kapazitätsgrenzen
- **ContainerBlueprint**: Vorlage für einen Container-Typ

Funktionen: `createBox3D()`, `boxVolume()`, `boxBaseArea()`, `containerCanFit()`, usw.

#### `ts-src/geometry.ts`

- **intersects()**: AABB-Kollisionserkennung zwischen zwei Objekten
- **overlap1d()**: Berechnet 1D-Überlappung
- **overlapAreaXY()**: Berechnet XY-Überlappungsfläche
- **pointInside()**: Punkt-in-Box-Test

#### `ts-src/optimizer.ts`

- **PackingConfig**: Konfigurierbare Parameter (Raster, Support-Ratio, Toleranzen)
- **packObjects()**: Hauptalgorithmus zur Verpackung
- **packObjectsWithProgress()**: Version mit Live-Event-Callback
- **findStablePosition()**: Findet stabile Position für ein Objekt
- **hasSufficientSupport()**: Prüft Mindestauflage
- **supportsWeightCorrectly()**: Prüft Gewichts-Hierarchie
- **maintainsBalance()**: Prüft Schwerpunkt-Abweichung

#### `ts-src/api.ts`

- **REST-API** mit Bun's nativem HTTP-Server
- **CORS-Support** für Frontend-Kommunikation
- JSON-Serialisierung/Deserialisierung
- Server-Sent Events für Live-Streaming

#### `ts-src/config.ts`

- **loadConfig()**: Lädt Konfiguration aus Umgebungsvariablen
- Fallback auf Standardwerte

#### `ts-src/index.ts`

- Einstiegspunkt der Anwendung
- Startet den API-Server

## 📈 Performance

- **Durchsatz**: ~100+ Objekte/Sekunde (abhängig vom Grid-Step)
- **Speicher**: O(n) für n Objekte
- **Komplexität**: O(n × p × z) wobei:
  - n = Anzahl Objekte
  - p = Raster-Positionen
  - z = Z-Ebenen
- **Binary Größe**: ~100MB (enthält Bun Runtime)

## 🧪 Testen

### Programmatische Verwendung

Die Bibliothek kann direkt in TypeScript/JavaScript Code verwendet werden:

```bash
# Beispiele ausführen
bun run example
```

Siehe `ts-src/example.ts` für verschiedene Verwendungsbeispiele:
- Einfaches Packing-Szenario
- Mehrere Container-Typen
- Live-Progress-Tracking
- Benutzerdefinierte Konfiguration

### API-Tests

```bash
# API testen
curl http://localhost:8080/

# Packing API testen
curl -X POST http://localhost:8080/pack \
  -H "Content-Type: application/json" \
  -d '{
    "containers": [
      {"name": "Standard", "dims": [100, 100, 70], "max_weight": 500}
    ],
    "objects": [
      {"id": 1, "dims": [30, 30, 10], "weight": 50},
      {"id": 2, "dims": [20, 50, 15], "weight": 30}
    ]
  }'

# Streaming API testen
curl -X POST http://localhost:8080/pack_stream \
  -H "Content-Type: application/json" \
  -d '{
    "containers": [
      {"name": "Standard", "dims": [100, 100, 70], "max_weight": 500}
    ],
    "objects": [
      {"id": 1, "dims": [30, 30, 10], "weight": 50}
    ]
  }'
```

## 🐛 Bekannte Einschränkungen

1. **Rotation**: Objekte werden nicht rotiert (Fixed Orientation)
2. **Dynamische Stabilität**: Keine physikalische Simulation
3. **Optimales Packing**: Heuristik, kein garantiertes Optimum

## 🔄 Vergleich zur Rust-Version

### Vorteile der TypeScript-Version

- **Entwicklungsgeschwindigkeit**: Schnellere Iteration und einfachere Wartung
- **JavaScript-Ökosystem**: Direkter Zugriff auf npm-Pakete
- **Typsicherheit**: Durch TypeScript
- **Einfachere Erweiterung**: Für JavaScript-Entwickler zugänglicher

### Vorteile der Rust-Version

- **Performance**: Schnellere Ausführung für große Datenmengen
- **Speichereffizienz**: Geringerer Memory-Footprint
- **Binary-Größe**: Kleinere ausführbare Dateien
- **Compile-Zeit-Garantien**: Strengere Typsicherheit

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

Entwickelt mit ❤️ in TypeScript & Bun
