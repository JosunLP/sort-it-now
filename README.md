# Sort-it-now - 3D Box Packing Optimizer

An intelligent 3D packing optimization service with interactive visualization.

## 🎯 Features

### Backend (Rust)

- **Heuristic packing algorithm** considering:
  - Weight limits and distribution
  - Stability and support (60% minimum support ratio)
  - Center of mass balance
  - Layering (heavy objects at the bottom)
- **Automatic multi-container management**
- **Optional object rotations** (enabled via request flag or environment variable)
- **Comprehensive unit tests**
- **REST API** with JSON communication
- **OpenAPI & Swagger UI** with live documentation at `/docs`
- **OOP principles** with DRY architecture
- **Fully documented code** (Rust docstrings)

### Frontend (JavaScript/Three.js)

- **Interactive 3D visualization**
- **OrbitControls** for camera control
- **Container navigation** (Previous/Next buttons)
- **Step-by-step animation** of the packing process
- **Live statistics**:
  - Object count
  - Total weight
  - Volume utilization
  - Center of mass position
- **Configuration modal** with object rotation toggle
- **Responsive design**

## 🚀 Installation & Startup

### Prerequisites

- Rust (1.70+)
- Cargo
- Modern web browser

### Start the backend

```bash
cargo run
```

The server runs on `http://localhost:8080`

> 💡 **Configuration note:** Copy `.env.example` to `.env` if needed to customize the API port, host, or update parameters. Unset values automatically fall back to their defaults.

### Open the frontend

The web client is automatically served by the Rust backend. After startup, simply open `http://localhost:8080/` in your browser.

In the browser:

- Button "🚀 Pack (Batch)" performs a one-time optimization and displays the result.
- Button "📡 Pack (Live)" starts the live stream of optimization steps via SSE and renders them continuously.

## 📦 Pre-built Releases & Release Pipeline

A GitHub Actions workflow (`.github/workflows/release.yml`) exists for releases that generates platform packages when tags in the format `v*` are created (or manually via _workflow_dispatch_):

- **Linux (x86_64)**: `sort-it-now-<version>-linux-x86_64.tar.gz`
- **macOS (ARM64/Apple Silicon)**: `sort-it-now-<version>-macos-arm64.tar.gz`
- **macOS (x86_64/Intel)**: `sort-it-now-<version>-macos-x86_64.tar.gz`
- **Windows (x86_64)**: `sort-it-now-<version>-windows-x86_64.zip`

Each package contains the pre-compiled binary, the current `README.md`, and an installation script.
The artifacts are uploaded both as workflow artifacts and automatically added to the GitHub release for the corresponding tag version.

### Installation Scripts

- Linux/macOS: Run `./install.sh` in the extracted folder (optionally with `sudo`) to copy `sort_it_now` to `/usr/local/bin`.
- Windows: Run `install.ps1` (PowerShell). By default, it installs to `%ProgramFiles%\sort-it-now` and adds the path to the user environment variable.

### Docker

For each release, a Docker image is automatically published to [Docker Hub](https://hub.docker.com/). Images are provided for multiple architectures (linux/amd64, linux/arm64).

> 📖 **Setup guide:** See [DOCKER_SETUP.md](DOCKER_SETUP.md) for a detailed guide on setting up the Docker Hub deployment pipeline.

**Run Docker image:**

> **Note:** Replace `<username>` with `josunlp` (or the corresponding Docker Hub username of the project maintainer).

```bash
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 <username>/sort-it-now:latest
```

**With environment variables:**

```bash
docker run -p 8080:8080 \
  -e SORT_IT_NOW_API_HOST=0.0.0.0 \
  -e SORT_IT_NOW_API_PORT=8080 \
  -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 \
  <username>/sort-it-now:latest
```

**Build your own image:**

```bash
docker build -t sort-it-now .
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 sort-it-now
```

The server is then available at `http://localhost:8080`.

## 🔔 Automatic Updates on Startup

On startup, the service checks for the latest GitHub releases (`JosunLP/sort-it-now`) in the background. If a newer version is found, the updater downloads the appropriate release package and runs the installation script for the current platform. This automatically applies the update where possible. On Windows, if `sort_it_now.exe` is locked, a `sort_it_now.new.exe` is placed instead.

- The check can be disabled via the environment variable `SORT_IT_NOW_SKIP_UPDATE_CHECK=1` (e.g., for offline installations or CI).
- GitHub limits unauthenticated API calls to 60 per hour. If the limit is reached, the check is skipped and info is displayed. Optionally set `SORT_IT_NOW_GITHUB_TOKEN` (or `GITHUB_TOKEN`) to a Personal Access Token to get higher limits; the updater also uses the token when downloading release artifacts.
- To avoid unexpectedly large downloads, the updater limits release artifacts to 200 MB by default. Adjust the limit via `SORT_IT_NOW_MAX_DOWNLOAD_MB` (value `0` disables the limit).
- Repo/owner and timeout can be configured via `SORT_IT_NOW_GITHUB_OWNER`, `SORT_IT_NOW_GITHUB_REPO`, and `SORT_IT_NOW_HTTP_TIMEOUT_SECS` – defaults apply automatically if no `.env` is present.

## 📊 API Endpoints

### OpenAPI & Swagger UI

- `GET /docs` delivers an interactive Swagger UI with Subresource Integrity-protected assets.
- `GET /docs/openapi.json` provides the OpenAPI schema (v3) and can be used for code generators.

### POST /pack

Packs objects into containers.

**Request:**

```json
{
  "containers": [
    { "name": "Standard", "dims": [100.0, 100.0, 70.0], "max_weight": 500.0 },
    { "name": "Compact", "dims": [60.0, 80.0, 50.0], "max_weight": 320.0 }
  ],
  "objects": [
    { "id": 1, "dims": [30.0, 30.0, 10.0], "weight": 50.0 },
    { "id": 2, "dims": [20.0, 50.0, 15.0], "weight": 30.0 }
  ],
  "allow_rotations": true
}
```

The optional field `allow_rotations` enables 90° rotations per request. If omitted, the default setting from the environment variable `SORT_IT_NOW_PACKING_ALLOW_ROTATIONS` (default: false) applies.

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

Streams progress events in real-time as `text/event-stream`. Each event is a JSON object with a `type` field:

- `ContainerStarted` { id, dims, max_weight, label, template_id }
- `ObjectPlaced` { container_id, id, pos, weight, dims, total_weight }
- `Finished`

Note: In the frontend, you can start live mode with the "📡 Pack (Live)" button.

## 🧪 Running Tests

```bash
cargo test
```

All tests should pass successfully:

- ✅ heavy_boxes_stay_below_lighter
- ✅ single_box_snaps_to_corner
- ✅ creates_additional_containers_when_weight_exceeded
- ✅ reject_heavier_on_light_support
- ✅ sample_pack_respects_weight_order

## 🏗️ Architecture

### Rust Modules

#### `main.rs`

- Application entry point
- Starts the Tokio runtime and API server

#### `model.rs`

- **`Box3D`**: Represents a 3D object with ID, dimensions, and weight
- **`PlacedBox`**: Object with position in the container
- **`Container`**: Packaging container with capacity limits
- Methods: `volume()`, `base_area()`, `total_weight()`, `remaining_weight()`, `utilization_percent()`

#### `geometry.rs`

- **`intersects()`**: AABB collision detection between two objects
- **`overlap_1d()`**: Calculates 1D overlap
- **`overlap_area_xy()`**: Calculates XY overlap area
- **`point_inside()`**: Point-in-box test

#### `optimizer.rs`

- **`PackingConfig`**: Configurable parameters (grid, support ratio, tolerances)
- **`pack_objects()`**: Main packing algorithm
- **`pack_objects_with_config()`**: Version with customizable parameters
- **`find_stable_position()`**: Finds stable position for an object
- **`supports_weight_correctly()`**: Checks weight hierarchy
- **`has_sufficient_support()`**: Checks minimum support ratio
- **`calculate_balance_after()`**: Calculates center of mass deviation

#### `api.rs`

- **REST API** with Axum framework
- **CORS support** for frontend communication
- JSON serialization/deserialization

### JavaScript Modules

#### `script.js`

- Three.js scene setup
- OrbitControls for camera
- Functions:
  - `clearScene()`: Clears scene
  - `drawContainerFrame()`: Draws container wireframe
  - `drawBox()`: Renders individual object
  - `visualizeContainer()`: Shows complete container
  - `animateContainer()`: Step-by-step animation
  - `updateStats()`: Updates statistics panel
  - `fetchPacking()`: API communication

## 🎨 Optimizations

### DRY Principle

- **`PackingConfig` structure** instead of scattered constants
- Reusable functions for geometry calculations
- Centralized error handling

### OOP Principles

- Clear separation of data models and logic
- Encapsulation in modules
- Trait implementation for common behavior

### Code Documentation

- Rust docstrings for all public functions
- JSDoc comments in frontend
- Inline comments for complex algorithms

## 🔧 Configuration

### Backend Configuration (.env)

The application optionally loads a `.env` file on startup (using [`dotenvy`](https://crates.io/crates/dotenvy)). Unset variables retain their defaults, so the service runs normally even without `.env`. Relevant variables:

| Variable                                    | Default       | Description                                                                                                        |
| ------------------------------------------- | ------------- | ------------------------------------------------------------------------------------------------------------------ |
| `SORT_IT_NOW_API_HOST`                      | `0.0.0.0`     | IP address the HTTP server binds to. Set e.g. `127.0.0.1` for local access.                                        |
| `SORT_IT_NOW_API_PORT`                      | `8080`        | API server port. Values of `0` are rejected.                                                                       |
| `SORT_IT_NOW_GITHUB_OWNER`                  | `JosunLP`     | GitHub owner/organization whose releases are queried for updates.                                                  |
| `SORT_IT_NOW_GITHUB_REPO`                   | `sort-it-now` | Repository name for the updater.                                                                                   |
| `SORT_IT_NOW_HTTP_TIMEOUT_SECS`             | `30`          | Timeout in seconds for GitHub HTTP requests by the updater.                                                        |
| `SORT_IT_NOW_MAX_DOWNLOAD_MB`               | `200`         | Maximum size of a release asset (0 = unlimited).                                                                   |
| `SORT_IT_NOW_GITHUB_TOKEN` / `GITHUB_TOKEN` | –             | Optional PAT for higher GitHub rate limits and private releases.                                                   |
| `SORT_IT_NOW_SKIP_UPDATE_CHECK`             | –             | If set (any value), disables automatic update check.                                                               |
| `SORT_IT_NOW_PACKING_GRID_STEP`             | `5.0`         | ⚠️ Position grid step size; smaller values give finer placement but slow down and may cause unstable arrangements. |
| `SORT_IT_NOW_PACKING_SUPPORT_RATIO`         | `0.6`         | ⚠️ Minimum support ratio for stable stacking; lower values increase tipping risk.                                  |
| `SORT_IT_NOW_PACKING_HEIGHT_EPSILON`        | `1e-3`        | ⚠️ Tolerance for height comparisons; values too large or small affect stability checks.                            |
| `SORT_IT_NOW_PACKING_GENERAL_EPSILON`       | `1e-6`        | ⚠️ General numerical tolerance; extreme values may cause incorrect collision results.                              |
| `SORT_IT_NOW_PACKING_BALANCE_LIMIT_RATIO`   | `0.45`        | ⚠️ Center of mass deviation limit; higher values allow more tilting.                                               |
| `SORT_IT_NOW_PACKING_ALLOW_ROTATIONS`       | `false`       | Enables all 90° object rotations. Can also be set per request via `allow_rotations`.                               |

An example file can be found in `.env.example`.

### Packing Parameters (optimizer.rs)

```rust
PackingConfig {
    grid_step: 5.0,              // Position grid in units
    support_ratio: 0.6,          // 60% minimum support
    height_epsilon: 1e-3,        // Height tolerance
    general_epsilon: 1e-6,       // General tolerance
    balance_limit_ratio: 0.45,   // Max center of mass deviation
    allow_item_rotation: false,  // Enable object rotations (disabled by default)
}
```

### Frontend Configuration (script.js)

```javascript
const CONTAINER_SIZE = [100, 100, 70];  // Container dimensions
const COLOR_PALETTE = [...];            // Object colors
```

## 📈 Performance

- **Throughput**: ~100 objects/second
- **Memory**: O(n) for n objects
- **Complexity**: O(n × p × z) where:
  - n = number of objects
  - p = grid positions
  - z = Z-levels

## 🐛 Known Limitations

1. **Rotation**: Only 90° rotations; complex freeform rotations are not covered
2. **Dynamic stability**: No physical simulation
3. **Optimal packing**: Heuristic, no guaranteed optimum
4. **Browser support**: Requires WebGL support

## 📝 License

Project-specific - See license file.

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Commit your changes
4. Push to the branch
5. Open a pull request

## 📧 Contact

For questions or issues, please open an issue.

---

Developed with ❤️ in Rust & Three.js
