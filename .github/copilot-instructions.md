# Copilot Instructions for sort-it-now

## Project Overview

**Sort-it-now** is a 3D packing optimization service in Rust with a web frontend. It solves the bin-packing problem: efficiently packing cuboids into containers considering weight, stability, and center of mass.

## Architecture

```
src/
├── main.rs        # Tokio runtime & server start, loads .env via dotenvy
├── config.rs      # Environment variables → AppConfig/ApiConfig/OptimizerConfig/UpdateConfig
├── model.rs       # Data structures: Box3D, PlacedBox, Container, ContainerBlueprint
├── geometry.rs    # AABB collision (intersects), overlap (overlap_1d), point_inside
├── optimizer.rs   # Packing algorithm with PackingConfig (1700+ lines, incl. tests)
├── api.rs         # Axum REST API: /pack, /pack_stream (SSE), /docs (Swagger UI)
└── update.rs      # Auto-update via GitHub Releases (platform-specific)
web/
├── index.html     # Frontend entry point
└── script.js      # Three.js 3D visualization with OrbitControls
```

## Developer Workflow

```bash
# Start server (port 8080)
cargo run

# Run tests
cargo test

# Formatting & linting (CI check - must pass before PR!)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

---

## Packing Algorithm (Detailed Core Logic)

### Main Functions in `optimizer.rs`

| Function                       | Purpose                                               |
| ------------------------------ | ----------------------------------------------------- |
| `pack_objects()`               | Entry point with default config                       |
| `pack_objects_with_config()`   | With customizable `PackingConfig`                     |
| `pack_objects_with_progress()` | With callback for live events (SSE)                   |
| `find_stable_position()`       | Finds optimal position via grid search + Z-levels     |
| `supports_weight_correctly()`  | Checks weight hierarchy (heavy BELOW light)           |
| `has_sufficient_support()`     | Checks minimum support via `support_ratio_of()`       |
| `is_center_supported()`        | Prevents overhangs (center of mass must be supported) |
| `calculate_balance_after()`    | Calculates center of mass deviation                   |

### Algorithm Flow

1. **Sorting**: Objects by `weight * volume` descending (heavy/large first)
2. **Clustering**: `FootprintClusterStrategy` groups objects with similar base area
3. **Orientations**: With `allow_item_rotation=true` → 6 permutations (deduplicated)
4. **Position Search**:
   - Iterate Z-levels (floor + tops of all placed objects)
   - Grid on X/Y axis with `grid_step`
   - Evaluate each position by `PlacementScore { z, y, x, balance }`
5. **Stability Checks** (all must pass):
   - No collision (`intersects()`)
   - Minimum support (`support_ratio >= 60%`)
   - Weight hierarchy (no heavy on light object)
   - Center of mass supported (`is_center_supported()`)
   - Balance within `balance_limit_ratio`
6. **Multi-Container**: If no space → new container from template pool

### Configuration via Builder Pattern

```rust
PackingConfig::builder()
    .grid_step(2.5)                    // Finer grid (slower)
    .support_ratio(0.7)                // 70% minimum support
    .height_epsilon(1e-3)              // Tolerance for height comparisons
    .general_epsilon(1e-6)             // General float tolerance
    .balance_limit_ratio(0.45)         // Max center of mass deviation
    .footprint_cluster_tolerance(0.15) // Clustering tolerance
    .allow_item_rotation(true)         // 90° rotations
    .build()
```

### Diagnostic Structures

```rust
// Per container
ContainerDiagnostics {
    center_of_mass_offset: f64,      // Center of mass distance from center
    balance_limit: f64,              // Allowed deviation
    imbalance_ratio: f64,            // offset / limit
    average_support_percent: f64,    // Average support
    minimum_support_percent: f64,    // Worst support
    support_samples: Vec<SupportDiagnostics>,
}

// Aggregated
PackingDiagnosticsSummary {
    max_imbalance_ratio: f64,
    worst_support_percent: f64,
    average_support_percent: f64,
}
```

### Unplaceable Objects

```rust
enum UnplacedReason {
    TooHeavyForContainer,        // Exceeds max_weight of all templates
    DimensionsExceedContainer,   // Doesn't fit in any orientation
    NoStablePosition,            // No stable position found
}
```

---

## Error Handling Patterns

### ValidationError in `model.rs`

All constructors validate inputs and return `Result<T, ValidationError>`:

```rust
pub enum ValidationError {
    InvalidDimension(String),      // Non-positive, NaN, or Infinite
    InvalidWeight(String),         // Non-positive, NaN, or Infinite
    InvalidConfiguration(String),  // Reserved for config errors
}

// Example: Box3D::new() checks
Box3D::new(id, (w, d, h), weight)?  // Error on w <= 0, NaN, Inf

// ContainerBlueprint checks analogously
ContainerBlueprint::new(id, name, dims, max_weight)?
```

### API Validation in `api.rs`

```rust
enum PackRequestValidationError {
    MissingContainers,              // Empty container list
    InvalidContainer(ValidationError),
    InvalidObject(ValidationError),
}

// Conversion to HTTP response
impl IntoResponse for PackRequestValidationError { ... }
```

---

## Frontend Integration (`script.js`)

### SSE Events for Live Visualization

```javascript
// EventSource for /pack_stream
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

### Epsilon Constants (Backend-compatible)

```javascript
const EPSILON_COMPARISON = 1e-6;      // Dimension comparisons
const EPSILON_DEDUPLICATION = 1e-6;   // Exact equality

// Usage for rotation deduplication
function dimsAlmostEqual(a, b, epsilon = EPSILON_DEDUPLICATION) {
  return Math.abs(a[0] - b[0]) <= epsilon && ...;
}
```

### Three.js Setup

```javascript
import * as THREE from 'https://esm.sh/three@0.163.0';
import { OrbitControls } from 'https://esm.sh/three@0.163.0/examples/jsm/controls/OrbitControls.js';

// Core functions
clearScene(); // Removes all Meshes/LineSegments
drawContainerFrame(); // Wireframe + Grid
drawBox(); // Object mesh with color + opacity
visualizeContainer(); // Complete container display
animateContainer(); // Step-by-step animation
updateStats(); // Statistics panel with diagnostics
```

---

## Auto-Update Mechanism (`update.rs`)

### Flow

1. **Start**: `check_for_updates_background()` spawns Tokio task
2. **GitHub API**: Calls `/repos/{owner}/{repo}/releases/latest`
3. **Version comparison**: `semver::Version` → Update only if `latest > current`
4. **Download**: Platform-specific asset (tar.gz/zip)
5. **Verification**: SHA-256 checksum from `.sha256` file
6. **Installation**: Platform-specific logic:
   - Linux/macOS: Run `install-unix.sh`
   - Windows: Replace binary (or `.new.exe` if locked)

### Configuration

| Variable            | Default | Description                      |
| ------------------- | ------- | -------------------------------- |
| `SKIP_UPDATE_CHECK` | -       | Completely disable update        |
| `GITHUB_TOKEN`      | -       | For higher rate limits           |
| `MAX_DOWNLOAD_MB`   | 200     | Asset size limit (0 = unlimited) |
| `HTTP_TIMEOUT_SECS` | 30      | Timeout for GitHub requests      |

### Rate Limiting

```rust
// Automatic detection + hint
if is_rate_limit_response(&headers) {
    println!("⏱️ GitHub rate limit reached...");
    if token.is_none() {
        println!("💡 Tip: Set GITHUB_TOKEN...");
    }
}
```

---

## API Endpoints

| Method | Path                 | Description                    |
| ------ | -------------------- | ------------------------------ |
| `POST` | `/pack`              | Batch packing → `PackResponse` |
| `POST` | `/pack_stream`       | SSE stream with `PackEvent`s   |
| `GET`  | `/docs`              | Swagger UI (SRI-protected)     |
| `GET`  | `/docs/openapi.json` | OpenAPI 3 schema               |

### Request Format

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

## Important Conventions

### Rust-specific

- **Docstrings**: Document all public functions/structs in English
- **Validation**: Always `Result<T, ValidationError>` for constructors
- **Builder Pattern**: `PackingConfig::builder()` for configuration
- **Epsilon Constants**: Consistently use `1e-6` (general) / `1e-3` (height)
- **Platform Compilation**: `#[cfg(target_os = "...")]` for OS-specific code

### Tests

- Tests in `#[cfg(test)]` modules at the end of each file
- Main tests in `optimizer.rs` (15+ tests):
  - `heavy_boxes_stay_below_lighter` - Weight hierarchy
  - `single_box_snaps_to_corner` - Positioning
  - `creates_additional_containers_when_weight_exceeded` - Multi-container
  - `reject_heavier_on_light_support` - Stability rules
- Helper function `assert_heavy_below()` checks weight sorting across all layers

### Frontend

- ESM imports from `esm.sh` for Three.js
- `config` object for containers/objects/rotations
- Validation via `collectConfigIssues()` + `ensureConfigValidOrNotify()`

---

## Geometry Functions (`geometry.rs`)

### AABB Collision Detection

```rust
/// Separating Axis Theorem: Objects do NOT intersect
/// if they are completely separated on at least one axis.
pub fn intersects(a: &PlacedBox, b: &PlacedBox) -> bool {
    !(ax + aw <= bx || bx + bw <= ax ||  // X-axis separated
      ay + ad <= by || by + bd <= ay ||  // Y-axis separated
      az + ah <= bz || bz + bh <= az)    // Z-axis separated
}
```

### Overlap Calculation

```rust
/// Calculates 1D overlap of two intervals
/// Example: overlap_1d(0.0, 5.0, 3.0, 8.0) → 2.0
pub fn overlap_1d(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    (a2.min(b2) - a1.max(b1)).max(0.0)
}

/// Overlap area in XY plane (for support calculation)
pub fn overlap_area_xy(a: &PlacedBox, b: &PlacedBox) -> f64 {
    overlap_1d(...) * overlap_1d(...)  // X × Y
}
```

### Point-in-Box Test

```rust
/// Checks if center of mass projection is carried by supporting box
pub fn point_inside(point: (f64, f64, f64), placed_box: &PlacedBox) -> bool {
    px >= bx && px <= bx + bw &&  // X within
    py >= by && py <= by + bd &&  // Y within
    pz >= bz && pz <= bz + bh     // Z within
}
```

---

## Test Patterns

### Test Structure in `optimizer.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Helper: Create single container template
    fn single_blueprint(dims: (f64, f64, f64), max_weight: f64) -> Vec<ContainerBlueprint> {
        vec![ContainerBlueprint::new(0, None, dims, max_weight).unwrap()]
    }

    // Helper: Check weight hierarchy across all layers
    fn assert_heavy_below(cont: &Container, config: &PackingConfig) {
        for lower in &cont.placed {
            for upper in &cont.placed {
                // Checks: Object directly above must be lighter
                if overlap_exists && upper_above_lower {
                    assert!(lower.weight >= upper.weight);
                }
            }
        }
    }
}
```

### Core Test Categories

| Test                                                 | Checks                                      |
| ---------------------------------------------------- | ------------------------------------------- |
| `heavy_boxes_stay_below_lighter`                     | Vertical weight sorting                     |
| `single_box_snaps_to_corner`                         | Placement at (0,0,0)                        |
| `creates_additional_containers_when_weight_exceeded` | Multi-container logic                       |
| `reports_objects_too_large_for_container`            | `UnplacedReason::DimensionsExceedContainer` |
| `reports_objects_too_heavy_for_container`            | `UnplacedReason::TooHeavyForContainer`      |
| `reject_heavier_on_light_support`                    | Stability: Heavy on light forbidden         |
| `rotation_toggle_controls_reorientation`             | `allow_item_rotation` effect                |
| `orientation_deduplication_handles_equal_dimensions` | Cube → 1, cuboid → 3-6 orientations         |
| `footprint_cluster_groups_similar_dimensions`        | Clustering strategy                         |
| `diagnostics_capture_support_and_balance_metrics`    | Diagnostic values correct                   |
| `progress_emits_diagnostics_events`                  | SSE events are emitted                      |

### Running Tests

```bash
# All tests
cargo test

# Single test with output
cargo test heavy_boxes_stay_below_lighter -- --nocapture

# Tests with pattern
cargo test rotation
```

---

## Performance Notes

### Algorithm Complexity

| Factor                | Impact                                           |
| --------------------- | ------------------------------------------------ |
| `grid_step`           | Smaller → more positions → slower, more accurate |
| Object count (n)      | O(n × p × z) where p = positions, z = Z-levels   |
| `allow_item_rotation` | 6× more orientations → 6× more checks            |
| Container templates   | Each template is checked for new containers      |

### Recommended Settings

```rust
// Fast (prototyping)
PackingConfig::builder()
    .grid_step(10.0)
    .support_ratio(0.5)
    .build()

// Precise (production)
PackingConfig::builder()
    .grid_step(2.5)
    .support_ratio(0.7)
    .allow_item_rotation(true)
    .build()
```

### Memory

- O(n) for n objects
- `PlacedBox` contains clone of `Box3D` → Moderate overhead
- SSE streaming reduces peak memory for large requests

---

## Environment Variables

All with prefix `SORT_IT_NOW_`:

| Variable                  | Default   | Description            |
| ------------------------- | --------- | ---------------------- |
| `API_HOST`                | `0.0.0.0` | Server bind IP         |
| `API_PORT`                | `8080`    | Server port            |
| `PACKING_GRID_STEP`       | `5.0`     | Grid step size         |
| `PACKING_SUPPORT_RATIO`   | `0.6`     | Minimum support (0-1)  |
| `PACKING_ALLOW_ROTATIONS` | `false`   | 90° rotations          |
| `SKIP_UPDATE_CHECK`       | -         | Disable auto-update    |
| `GITHUB_TOKEN`            | -         | For higher rate limits |

---

## Docker

```bash
# Pre-built image
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 josunlp/sort-it-now:latest

# Build your own image
docker build -t sort-it-now .
docker run -p 8080:8080 -e SORT_IT_NOW_SKIP_UPDATE_CHECK=1 sort-it-now
```

---

## CI/CD Workflows

| Workflow      | Trigger         | Actions                                  |
| ------------- | --------------- | ---------------------------------------- |
| `rust.yml`    | Push/PR on main | Format + Clippy + Tests (ubuntu/windows) |
| `release.yml` | Tag `v*`        | Platform packages (Linux/macOS/Windows)  |
| `docker.yml`  | Tag `v*`        | Multi-arch images on Docker Hub          |
| `codeql.yml`  | Push            | Security analysis                        |
| `stale.yml`   | Schedule        | Mark old issues/PRs                      |
