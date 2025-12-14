# Copilot Instructions for sort-it-now

## Project Overview

**Sort-it-now** is a 3D packing optimization service in Rust with a web frontend. It solves the bin-packing problem: efficiently packing cuboids into containers considering weight, stability, and center of mass.

## Architecture

```
src/
├── main.rs        # Tokio runtime & server start, loads .env via dotenvy
├── config.rs      # Environment variables → AppConfig/ApiConfig/OptimizerConfig/UpdateConfig
├── types.rs       # Core types: Vec3, BoundingBox, Traits (Dimensional, Positioned, Weighted)
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

# Run tests (42 tests across all modules)
cargo test

# Formatting & linting (CI check - must pass before PR!)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

---

## Core Types (`types.rs`)

The `types.rs` module provides reusable types and trait abstractions following OOP and DRY principles.

### Vec3 - 3D Vector Type

```rust
/// Represents a 3D vector or point in space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,  // Width
    pub y: f64,  // Depth
    pub z: f64,  // Height
}

// Operator overloading for intuitive math
let center = position + dimensions * 0.5;

// Key methods
vec.volume()           // x * y * z
vec.base_area()        // x * y
vec.distance_to(other) // 3D Euclidean distance
vec.distance_2d(other) // XY plane distance
vec.fits_within(container, tolerance)
vec.is_valid_dimension()
```

### BoundingBox - AABB Collision

```rust
/// Axis-Aligned Bounding Box for efficient collision detection.
pub struct BoundingBox {
    pub min: Vec3,  // Lower corner
    pub max: Vec3,  // Upper corner
}

// Key methods
BoundingBox::from_position_and_dims(position, dims)
bbox.intersects(other)       // SAT collision test
bbox.overlap_area_xy(other)  // XY overlap for support
bbox.contains_point(point)   // Point-in-box test
bbox.center()                // Center point
bbox.top_z()                 // Maximum Z value
```

### Trait Abstractions (OOP Compliance)

```rust
/// Objects with 3D dimensions
pub trait Dimensional {
    fn dimensions(&self) -> Vec3;
    fn volume(&self) -> f64 { self.dimensions().volume() }
    fn base_area(&self) -> f64 { self.dimensions().base_area() }
    fn fits_in(&self, container_dims: &Vec3, tolerance: f64) -> bool;
}

/// Objects with a position in 3D space
pub trait Positioned {
    fn position(&self) -> Vec3;
}

/// Objects with weight
pub trait Weighted {
    fn weight(&self) -> f64;
}
```

### CenterOfMassCalculator

```rust
/// Accumulator for weighted center of mass calculation
let mut calc = CenterOfMassCalculator::new();
calc.add_point(x, y, weight);
let (cx, cy) = calc.compute().unwrap();
let offset = calc.distance_to((ref_x, ref_y));
```

### Epsilon Constants

```rust
pub const EPSILON_GENERAL: f64 = 1e-6;  // Dimension/weight comparisons
pub const EPSILON_HEIGHT: f64 = 1e-3;   // Height matching for stacking
```

---

## Data Model (`model.rs`)

All structures implement traits from `types.rs` for OOP compliance.

### Box3D

```rust
pub struct Box3D {
    pub id: u32,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub weight: f64,
}

// Implements: Dimensional, Weighted
Box3D::new(id, (w, d, h), weight)?  // Returns Result<Box3D, ValidationError>
```

### PlacedBox

```rust
pub struct PlacedBox {
    pub id: u32,
    pub x: f64, pub y: f64, pub z: f64,  // Position
    pub width: f64, pub depth: f64, pub height: f64,
    pub weight: f64,
}

// Implements: Positioned, Dimensional, Weighted
PlacedBox::from_box3d(box3d, x, y, z)
placed.bounding_box()  // Returns BoundingBox
placed.center_xy()     // (center_x, center_y)
placed.top_z()         // z + height
```

### Container

```rust
pub struct Container {
    pub id: u32,
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
    pub total_weight: f64,
    pub placed: Vec<PlacedBox>,
    pub label: Option<String>,
    pub template_id: Option<u32>,
}

// Implements: Dimensional
Container::new(id, dims, max_weight)
container.remaining_weight_capacity()
container.volume_utilization()
```

### ContainerBlueprint

```rust
pub struct ContainerBlueprint {
    pub id: u32,
    pub name: Option<String>,
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
}

ContainerBlueprint::new(id, name, dims, max_weight)?
blueprint.create_container(container_id)
blueprint.fits_box(box3d, epsilon)
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

### Validation Functions in `types.rs`

```rust
use crate::types::validation::{validate_dimension, validate_weight, validate_dimensions_3d};

validate_dimension(value, "Width")?;
validate_weight(value)?;
validate_dimensions_3d((w, d, h))?;
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

- **Trait-Based Design**: Use `Dimensional`, `Positioned`, `Weighted` traits for polymorphism
- **DRY Principle**: Use types from `types.rs` (Vec3, BoundingBox, CenterOfMassCalculator)
- **Docstrings**: Document all public functions/structs in English
- **Validation**: Always `Result<T, ValidationError>` for constructors
- **Builder Pattern**: `PackingConfig::builder()` for configuration
- **Epsilon Constants**: Use `EPSILON_GENERAL` (1e-6) and `EPSILON_HEIGHT` (1e-3) from `types.rs`
- **Platform Compilation**: `#[cfg(target_os = "...")]` for OS-specific code

### Code Organization

- `types.rs` - Core types and traits (foundation layer)
- `model.rs` - Domain objects implementing traits
- `geometry.rs` - Spatial algorithms using BoundingBox
- `optimizer.rs` - Business logic (packing algorithm)
- `api.rs` - HTTP interface
- `config.rs` - Configuration management
- `update.rs` - Self-update mechanism

### Tests

- Tests in `#[cfg(test)]` modules at the end of each file
- **42 tests** across all modules:
  - `types.rs`: Vec3 operations, BoundingBox, validation, CenterOfMassCalculator
  - `geometry.rs`: Intersection, overlap, distance calculations
  - `model.rs`: Validation errors, trait implementations
  - `optimizer.rs`: Packing algorithm (20+ tests)
  - `api.rs`: Request parsing, OpenAPI validation
  - `config.rs`: Boolean parsing, config defaults

### Test Categories in `optimizer.rs`

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
# All tests (42 total)
cargo test

# Single test with output
cargo test heavy_boxes_stay_below_lighter -- --nocapture

# Tests with pattern
cargo test rotation

# Tests by module
cargo test types::
cargo test geometry::
cargo test optimizer::
```

### Frontend

- ESM imports from `esm.sh` for Three.js
- `config` object for containers/objects/rotations
- Validation via `collectConfigIssues()` + `ensureConfigValidOrNotify()`

---

## Geometry Functions (`geometry.rs`)

### Using BoundingBox from types.rs

```rust
use crate::types::BoundingBox;

// Convert PlacedBox to BoundingBox for calculations
let bbox = placed_box.bounding_box();
let intersects = bbox.intersects(&other.bounding_box());
let overlap = bbox.overlap_area_xy(&support.bounding_box());
```

### AABB Collision Detection

```rust
/// Separating Axis Theorem: Objects do NOT intersect
/// if they are completely separated on at least one axis.
pub fn intersects(a: &PlacedBox, b: &PlacedBox) -> bool {
    a.bounding_box().intersects(&b.bounding_box())
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
    a.bounding_box().overlap_area_xy(&b.bounding_box())
}
```

### Point-in-Box Test

```rust
/// Checks if center of mass projection is carried by supporting box
pub fn point_inside(point: (f64, f64, f64), placed_box: &PlacedBox) -> bool {
    placed_box.bounding_box().contains_point(&Vec3::from_tuple(point))
}
```

### Distance Functions

```rust
/// 2D distance in XY plane (using Vec3::distance_2d)
pub fn distance_2d(a: (f64, f64), b: (f64, f64)) -> f64
```

---

## Test Patterns

### Test Structure

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

### Types Module Tests

```rust
#[test]
fn test_vec3_operations() {
    let a = Vec3::new(1.0, 2.0, 3.0);
    let b = Vec3::new(4.0, 5.0, 6.0);
    assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
}

#[test]
fn test_bounding_box_intersects() {
    let a = BoundingBox::from_position_and_dims(Vec3::zero(), Vec3::new(10.0, 10.0, 10.0));
    let b = BoundingBox::from_position_and_dims(Vec3::new(5.0, 5.0, 5.0), Vec3::new(10.0, 10.0, 10.0));
    assert!(a.intersects(&b));
}
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

### Type System Benefits

- `Vec3` operations are `#[inline]` for zero-cost abstraction
- `BoundingBox` calculations avoid redundant recomputation
- Trait-based polymorphism enables compiler optimizations

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

---

## Quick Reference

### Adding a New Object Type

1. Define struct in `model.rs`
2. Implement `Dimensional`, `Positioned`, `Weighted` traits as needed
3. Add validation in constructor using `types::validation`
4. Add tests

### Using Geometry Calculations

```rust
use crate::types::{Vec3, BoundingBox, EPSILON_GENERAL};

// Create bounding box
let bbox = BoundingBox::from_position_and_dims(
    Vec3::new(x, y, z),
    Vec3::new(w, d, h)
);

// Check collision
if bbox.intersects(&other_bbox) { ... }

// Calculate support
let support_area = bbox.overlap_area_xy(&support_bbox);
```

### Center of Mass Calculation

```rust
use crate::types::CenterOfMassCalculator;

let mut calc = CenterOfMassCalculator::new();
for placed in &container.placed {
    let (cx, cy) = placed.center_xy();
    calc.add_point(cx, cy, placed.weight);
}
let offset = calc.distance_to(container_center);
```
