//! Optimization logic for 3D object packing.
//!
//! This module implements a heuristic algorithm for efficient placement
//! of objects in containers, considering:
//! - Weight limits and distribution
//! - Stability and support
//! - Center of gravity balance
//! - Layering (heavy objects at the bottom)
//!
//! ## Algorithm Overview
//!
//! The packing algorithm works in the following phases:
//!
//! 1. **Sorting**: Objects are sorted by `weight × volume` descending
//!    (heavy and large objects first for better stability)
//!
//! 2. **Clustering**: The `FootprintClusterStrategy` groups objects with similar
//!    footprints to reduce fragmentation
//!
//! 3. **Orientation**: When rotation is enabled, up to 6 orientations
//!    per object are tested (deduplicated for symmetric objects)
//!
//! 4. **Position Search**: For each object, the best position is searched:
//!    - Iterate over all Z-layers (floor + tops of placed objects)
//!    - Grid search on X/Y axis with configurable step size
//!    - Evaluation by `PlacementScore { z, y, x, balance }`
//!
//! 5. **Stability Checks**: Each candidate position must pass:
//!    - No collision with existing objects
//!    - Minimum support (`support_ratio`) satisfied
//!    - Weight hierarchy maintained (heavy under light)
//!    - Center of gravity supported
//!    - Balance within limits
//!
//! 6. **Multi-Container**: When space is insufficient, a new container is created
//!
//! ## Performance Notes
//!
//! - **grid_step**: Smaller values → more accurate, but O(n²) slower
//! - **allow_item_rotation**: 6× more orientations → 6× more checks
//! - Complexity: O(n × p × z) with n=objects, p=positions, z=Z-layers
//!
//! ## Example
//!
//! ```ignore
//! use sort_it_now::optimizer::{pack_objects, PackingConfig};
//!
//! let config = PackingConfig::builder()
//!     .grid_step(2.5)
//!     .support_ratio(0.7)
//!     .allow_item_rotation(true)
//!     .build();
//!
//! let result = pack_objects_with_config(objects, templates, config);
//! ```

use std::cmp::Ordering;

use crate::geometry::{intersects, overlap_1d, point_inside};
use crate::model::{Box3D, Container, ContainerBlueprint, PlacedBox};
use utoipa::ToSchema;

/// Configuration for the packing algorithm.
///
/// Contains all tolerances and limits for controlling the optimization behavior.
#[derive(Copy, Clone, Debug)]
pub struct PackingConfig {
    /// Step size for position grid (smaller values = more accurate, but slower)
    pub grid_step: f64,
    /// Minimum fraction of the base area that must be supported (0.0 to 1.0)
    pub support_ratio: f64,
    /// Tolerance for height comparisons
    pub height_epsilon: f64,
    /// General numerical tolerance
    pub general_epsilon: f64,
    /// Maximum allowed deviation of center of gravity from center point (as ratio of diagonal)
    pub balance_limit_ratio: f64,
    /// Relative tolerance for pre-grouping by footprint to reduce backtracking
    pub footprint_cluster_tolerance: f64,
    /// Allows rotating objects to test alternative orientations
    pub allow_item_rotation: bool,
}

impl PackingConfig {
    pub const DEFAULT_GRID_STEP: f64 = 5.0;
    pub const DEFAULT_SUPPORT_RATIO: f64 = 0.6;
    pub const DEFAULT_HEIGHT_EPSILON: f64 = 1e-3;
    pub const DEFAULT_GENERAL_EPSILON: f64 = 1e-6;
    pub const DEFAULT_BALANCE_LIMIT_RATIO: f64 = 0.45;
    pub const DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE: f64 = 0.15;
    pub const DEFAULT_ALLOW_ITEM_ROTATION: bool = false;

    /// Creates a builder for custom configuration.
    pub fn builder() -> PackingConfigBuilder {
        PackingConfigBuilder::default()
    }
}

impl Default for PackingConfig {
    fn default() -> Self {
        Self {
            grid_step: Self::DEFAULT_GRID_STEP,
            support_ratio: Self::DEFAULT_SUPPORT_RATIO,
            height_epsilon: Self::DEFAULT_HEIGHT_EPSILON,
            general_epsilon: Self::DEFAULT_GENERAL_EPSILON,
            balance_limit_ratio: Self::DEFAULT_BALANCE_LIMIT_RATIO,
            footprint_cluster_tolerance: Self::DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE,
            allow_item_rotation: Self::DEFAULT_ALLOW_ITEM_ROTATION,
        }
    }
}

/// Builder pattern for PackingConfig (OOP principle).
#[derive(Clone, Debug, Default)]
pub struct PackingConfigBuilder {
    config: PackingConfig,
}

impl PackingConfigBuilder {
    /// Sets the grid step size.
    pub fn grid_step(mut self, step: f64) -> Self {
        self.config.grid_step = step;
        self
    }

    /// Sets the minimum support ratio.
    pub fn support_ratio(mut self, ratio: f64) -> Self {
        self.config.support_ratio = ratio;
        self
    }

    /// Sets the height tolerance.
    pub fn height_epsilon(mut self, epsilon: f64) -> Self {
        self.config.height_epsilon = epsilon;
        self
    }

    /// Sets the general tolerance.
    pub fn general_epsilon(mut self, epsilon: f64) -> Self {
        self.config.general_epsilon = epsilon;
        self
    }

    /// Sets the balance limit as a ratio of the diagonal.
    pub fn balance_limit_ratio(mut self, ratio: f64) -> Self {
        self.config.balance_limit_ratio = ratio;
        self
    }

    /// Sets the tolerance for pre-grouping based on footprint.
    pub fn footprint_cluster_tolerance(mut self, tolerance: f64) -> Self {
        self.config.footprint_cluster_tolerance = tolerance;
        self
    }

    /// Enables or disables rotation of objects.
    pub fn allow_item_rotation(mut self, allow: bool) -> Self {
        self.config.allow_item_rotation = allow;
        self
    }

    /// Creates the final configuration.
    pub fn build(self) -> PackingConfig {
        self.config
    }
}

/// Abstract strategies for grouping/reordering objects before packing.
///
/// This internal trait defines the interface for strategies that influence the order
/// (and possibly selection) of objects before the packing process. Implementations
/// can change the order of objects, form groups, or filter objects to improve
/// packing efficiency. It is guaranteed that the return is a (possibly filtered)
/// subset of the input; objects can be removed but not modified. The trait is
/// intentionally private, as it is only intended for internal optimization
/// strategies and does not guarantee a stable API.
trait ObjectClusterStrategy {
    fn reorder(&self, objects: Vec<Box3D>) -> Vec<Box3D>;
}

/// Groups objects with compatible footprints to reduce backtracking.
#[derive(Clone, Debug)]
struct FootprintClusterStrategy {
    tolerance: f64,
}

impl FootprintClusterStrategy {
    fn new(tolerance: f64) -> Self {
        Self { tolerance }
    }

    fn compatible(&self, a: (f64, f64), b: (f64, f64)) -> bool {
        if self.tolerance <= 0.0 {
            return false;
        }

        let width_close = self.relative_diff(a.0, b.0) <= self.tolerance;
        let depth_close = self.relative_diff(a.1, b.1) <= self.tolerance;
        width_close && depth_close
    }

    fn relative_diff(&self, a: f64, b: f64) -> f64 {
        let denom = a.abs().max(b.abs()).max(1.0);
        (a - b).abs() / denom
    }
}

impl ObjectClusterStrategy for FootprintClusterStrategy {
    fn reorder(&self, objects: Vec<Box3D>) -> Vec<Box3D> {
        if self.tolerance <= 0.0 {
            return objects;
        }

        let mut clusters: Vec<ObjectCluster> = Vec::new();
        for object in objects.into_iter() {
            let dims = (object.dims.0, object.dims.1);
            if let Some(cluster) = clusters
                .iter_mut()
                .find(|cluster| self.compatible(cluster.representative, dims))
            {
                cluster.add(object);
            } else {
                clusters.push(ObjectCluster::new(object));
            }
        }

        clusters
            .into_iter()
            .flat_map(ObjectCluster::into_members)
            .collect()
    }
}

#[derive(Clone, Debug)]
struct ObjectCluster {
    representative: (f64, f64),
    members: Vec<Box3D>,
}

impl ObjectCluster {
    fn new(object: Box3D) -> Self {
        let dims = (object.dims.0, object.dims.1);
        Self {
            representative: dims,
            members: vec![object],
        }
    }

    fn add(&mut self, object: Box3D) {
        let dims = (object.dims.0, object.dims.1);
        let count = self.members.len() as f64;
        let (rw, rd) = self.representative;
        self.representative = (
            (rw * count + dims.0) / (count + 1.0),
            (rd * count + dims.1) / (count + 1.0),
        );
        self.members.push(object);
    }

    fn into_members(self) -> Vec<Box3D> {
        self.members
    }
}

fn orientations_for(object: &Box3D, allow_rotation: bool) -> Vec<Box3D> {
    if !allow_rotation {
        return vec![object.clone()];
    }

    let (w, d, h) = object.dims;
    let permutations = [
        (w, d, h),
        (w, h, d),
        (d, w, h),
        (d, h, w),
        (h, w, d),
        (h, d, w),
    ];

    // Use HashSet for efficient deduplication
    // Convert dimensions to integer representation to avoid floating point comparison issues
    // Scale factor provides precision of 1e-6 units while avoiding overflow for typical dimensions
    const DIM_HASH_SCALE: f64 = 1e6;
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<Box3D> = Vec::new();

    for dims in permutations.into_iter() {
        // Create a key based on the actual dimensions (not sorted)
        // Use integer representation for reliable hashing
        let key = (
            (dims.0 * DIM_HASH_SCALE).round() as i64,
            (dims.1 * DIM_HASH_SCALE).round() as i64,
            (dims.2 * DIM_HASH_SCALE).round() as i64,
        );

        if seen.insert(key) {
            let mut rotated = object.clone();
            rotated.dims = dims;
            unique.push(rotated);
        }
    }

    unique
}

/// Support metrics per object.
#[derive(Clone, Debug, serde::Serialize, ToSchema)]
pub struct SupportDiagnostics {
    pub object_id: usize,
    pub support_percent: f64,
    pub rests_on_floor: bool,
}

/// Diagnostic metrics per container for monitoring.
#[derive(Clone, Debug, serde::Serialize, ToSchema)]
pub struct ContainerDiagnostics {
    pub center_of_mass_offset: f64,
    pub balance_limit: f64,
    pub imbalance_ratio: f64,
    pub average_support_percent: f64,
    pub minimum_support_percent: f64,
    pub support_samples: Vec<SupportDiagnostics>,
}

/// Summary of key metrics across all containers.
#[derive(Clone, Debug, serde::Serialize, ToSchema)]
pub struct PackingDiagnosticsSummary {
    pub max_imbalance_ratio: f64,
    pub worst_support_percent: f64,
    pub average_support_percent: f64,
}

impl Default for PackingDiagnosticsSummary {
    fn default() -> Self {
        Self {
            max_imbalance_ratio: 0.0,
            worst_support_percent: 100.0,
            average_support_percent: 100.0,
        }
    }
}

/// Result of the packing calculation.
#[derive(Clone, Debug)]
pub struct PackingResult {
    pub containers: Vec<Container>,
    pub unplaced: Vec<UnplacedBox>,
    pub container_diagnostics: Vec<ContainerDiagnostics>,
    pub diagnostics_summary: PackingDiagnosticsSummary,
}

impl PackingResult {
    /// Indicates whether all objects were packed.
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.unplaced.is_empty()
    }

    /// Returns the total number of containers.
    pub fn container_count(&self) -> usize {
        self.containers.len()
    }

    /// Returns the number of unpacked objects.
    pub fn unplaced_count(&self) -> usize {
        self.unplaced.len()
    }

    /// Calculates the average utilization of all containers.
    #[allow(dead_code)]
    pub fn average_utilization(&self) -> f64 {
        if self.containers.is_empty() {
            return 0.0;
        }
        let sum: f64 = self
            .containers
            .iter()
            .map(|c| c.utilization_percent())
            .sum();
        sum / self.containers.len() as f64
    }

    /// Calculates the total weight of all packed objects.
    #[allow(dead_code)]
    pub fn total_packed_weight(&self) -> f64 {
        self.containers.iter().map(|c| c.total_weight()).sum()
    }

    /// Returns the aggregated diagnostic values.
    #[allow(dead_code)]
    pub fn diagnostics_summary(&self) -> &PackingDiagnosticsSummary {
        &self.diagnostics_summary
    }
}

/// Object that could not be placed.
#[derive(Clone, Debug)]
pub struct UnplacedBox {
    pub object: Box3D,
    pub reason: UnplacedReason,
}

/// Reasons why an object could not be placed.
#[derive(Clone, Debug)]
pub enum UnplacedReason {
    TooHeavyForContainer,
    DimensionsExceedContainer,
    NoStablePosition,
}

impl UnplacedReason {
    pub fn code(&self) -> &'static str {
        match self {
            UnplacedReason::TooHeavyForContainer => "too_heavy_for_container",
            UnplacedReason::DimensionsExceedContainer => "dimensions_exceed_container",
            UnplacedReason::NoStablePosition => "no_stable_position",
        }
    }
}

impl std::fmt::Display for UnplacedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnplacedReason::TooHeavyForContainer => {
                write!(f, "Object exceeds the maximum allowed weight")
            }
            UnplacedReason::DimensionsExceedContainer => {
                write!(
                    f,
                    "Object does not fit in the container in at least one dimension"
                )
            }
            UnplacedReason::NoStablePosition => {
                write!(
                    f,
                    "No stable position found within the container"
                )
            }
        }
    }
}

fn determine_unfit_reason_across_templates(
    templates: &[ContainerBlueprint],
    object: &Box3D,
    config: &PackingConfig,
) -> UnplacedReason {
    if templates.is_empty() {
        return UnplacedReason::DimensionsExceedContainer;
    }

    let weight_blocked = templates
        .iter()
        .all(|tpl| object.weight > tpl.max_weight + config.general_epsilon);
    if weight_blocked {
        return UnplacedReason::TooHeavyForContainer;
    }

    let orientations = orientations_for(object, config.allow_item_rotation);
    let dimension_blocked = templates.iter().all(|tpl| {
        orientations.iter().all(|orientation| {
            orientation.dims.0 > tpl.dims.0 + config.general_epsilon
                || orientation.dims.1 > tpl.dims.1 + config.general_epsilon
                || orientation.dims.2 > tpl.dims.2 + config.general_epsilon
        })
    });
    if dimension_blocked {
        return UnplacedReason::DimensionsExceedContainer;
    }

    UnplacedReason::NoStablePosition
}

/// Main function for packing objects into containers.
///
/// Sorts objects by weight and volume (heavy/large first) and places
/// them sequentially into containers. Creates new containers when needed.
///
/// # Parameters
/// * `objects` - List of objects to pack
/// * `container_templates` - Available container types
///
/// # Returns
/// `PackingResult` with placed containers and possibly unpacked objects
#[allow(dead_code)]
pub fn pack_objects(
    objects: Vec<Box3D>,
    container_templates: Vec<ContainerBlueprint>,
) -> PackingResult {
    pack_objects_with_config(objects, container_templates, PackingConfig::default())
}

/// Packing with custom configuration.
///
/// Like `pack_objects`, but with customizable parameters.
///
/// # Parameters
/// * `objects` - List of objects to pack
/// * `container_templates` - Available container types
/// * `config` - Configuration parameters for the algorithm
pub fn pack_objects_with_config(
    objects: Vec<Box3D>,
    container_templates: Vec<ContainerBlueprint>,
    config: PackingConfig,
) -> PackingResult {
    pack_objects_with_progress(objects, container_templates, config, |_| {})
}

/// Events that occur during packing to enable live visualization.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum PackEvent {
    /// A new container is started.
    ContainerStarted {
        id: usize,
        dims: (f64, f64, f64),
        max_weight: f64,
        label: Option<String>,
        template_id: Option<usize>,
    },
    /// An object was placed.
    ObjectPlaced {
        container_id: usize,
        id: usize,
        pos: (f64, f64, f64),
        weight: f64,
        dims: (f64, f64, f64),
        total_weight: f64,
    },
    /// Updated diagnostics for a container.
    ContainerDiagnostics {
        container_id: usize,
        diagnostics: ContainerDiagnostics,
    },
    /// An object could not be placed.
    ObjectRejected {
        id: usize,
        weight: f64,
        dims: (f64, f64, f64),
        reason_code: String,
        reason_text: String,
    },
    /// Packing completed.
    Finished {
        containers: usize,
        unplaced: usize,
        diagnostics_summary: PackingDiagnosticsSummary,
    },
}

/// Packing with custom configuration and live progress callback.
///
/// Calls a callback for each important step (suitable for SSE/WebSocket).
pub fn pack_objects_with_progress(
    objects: Vec<Box3D>,
    container_templates: Vec<ContainerBlueprint>,
    config: PackingConfig,
    mut on_event: impl FnMut(&PackEvent),
) -> PackingResult {
    if objects.is_empty() {
        on_event(&PackEvent::Finished {
            containers: 0,
            unplaced: 0,
            diagnostics_summary: PackingDiagnosticsSummary::default(),
        });
        return PackingResult {
            containers: Vec::new(),
            unplaced: Vec::new(),
            container_diagnostics: Vec::new(),
            diagnostics_summary: PackingDiagnosticsSummary::default(),
        };
    }

    if container_templates.is_empty() {
        let mut unplaced = Vec::new();
        for obj in objects {
            on_event(&PackEvent::ObjectRejected {
                id: obj.id,
                weight: obj.weight,
                dims: obj.dims,
                reason_code: UnplacedReason::DimensionsExceedContainer.code().to_string(),
                reason_text: UnplacedReason::DimensionsExceedContainer.to_string(),
            });
            unplaced.push(UnplacedBox {
                object: obj,
                reason: UnplacedReason::DimensionsExceedContainer,
            });
        }
        on_event(&PackEvent::Finished {
            containers: 0,
            unplaced: unplaced.len(),
            diagnostics_summary: PackingDiagnosticsSummary::default(),
        });
        return PackingResult {
            containers: Vec::new(),
            unplaced,
            container_diagnostics: Vec::new(),
            diagnostics_summary: PackingDiagnosticsSummary::default(),
        };
    }

    let mut templates = container_templates;
    templates.sort_by(|a, b| {
        a.volume()
            .partial_cmp(&b.volume())
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                a.max_weight
                    .partial_cmp(&b.max_weight)
                    .unwrap_or(Ordering::Equal)
            })
    });

    // Sorting: Heavy and large objects first (stability principle)
    let mut objects = objects;
    objects.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                b.volume()
                    .partial_cmp(&a.volume())
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    let cluster_strategy = FootprintClusterStrategy::new(config.footprint_cluster_tolerance);
    objects = cluster_strategy.reorder(objects);

    let mut containers: Vec<Container> = Vec::new();
    let mut unplaced: Vec<UnplacedBox> = Vec::new();
    let mut container_diagnostics: Vec<ContainerDiagnostics> = Vec::new();

    // Platziere jedes Objekt
    'object_loop: for obj in objects {
        let orientations = orientations_for(&obj, config.allow_item_rotation);

        for oriented in &orientations {
            // Versuche, in bestehenden Containern zu platzieren
            for idx in 0..containers.len() {
                if !containers[idx].can_fit(&oriented) {
                    continue;
                }

                if let Some(position) = find_stable_position(&oriented, &containers[idx], &config) {
                    containers[idx].placed.push(PlacedBox {
                        object: oriented.clone(),
                        position,
                    });
                    let total_w = containers[idx].total_weight();
                    let placed = containers[idx]
                        .placed
                        .last()
                        .expect("missing placed object after insertion");
                    on_event(&PackEvent::ObjectPlaced {
                        container_id: idx + 1,
                        id: placed.object.id,
                        pos: placed.position,
                        weight: placed.object.weight,
                        dims: placed.object.dims,
                        total_weight: total_w,
                    });
                    let diagnostics = compute_container_diagnostics(&containers[idx], &config);
                    if let Some(slot) = container_diagnostics.get_mut(idx) {
                        *slot = diagnostics.clone();
                    } else if idx == container_diagnostics.len() {
                        container_diagnostics.push(diagnostics.clone());
                    } else {
                        panic!(
                            "diagnostics vector out of sync with containers (idx = {}, len = {})",
                            idx,
                            container_diagnostics.len()
                        );
                    }
                    on_event(&PackEvent::ContainerDiagnostics {
                        container_id: idx + 1,
                        diagnostics,
                    });
                    continue 'object_loop;
                }
            }

            // Keine bestehenden Container geeignet, versuche neue Container
            for template in &templates {
                if !template.can_fit(&oriented) {
                    continue;
                }

                let mut new_container = template.instantiate();
                if let Some(position) = find_stable_position(&oriented, &new_container, &config) {
                    let new_id = containers.len() + 1;
                    let dims = new_container.dims;
                    let max_weight = new_container.max_weight;
                    let label = new_container.label.clone();
                    let template_id = new_container.template_id;
                    on_event(&PackEvent::ContainerStarted {
                        id: new_id,
                        dims,
                        max_weight,
                        label,
                        template_id,
                    });

                    new_container.placed.push(PlacedBox {
                        object: oriented.clone(),
                        position,
                    });
                    let total_w = new_container.total_weight();
                    containers.push(new_container);
                    let placed = containers
                        .last()
                        .and_then(|c| c.placed.last())
                        .expect("missing newly placed box");
                    on_event(&PackEvent::ObjectPlaced {
                        container_id: new_id,
                        id: placed.object.id,
                        pos: placed.position,
                        weight: placed.object.weight,
                        dims: placed.object.dims,
                        total_weight: total_w,
                    });
                    let diagnostics = containers
                        .last()
                        .map(|c| compute_container_diagnostics(c, &config))
                        .expect("missing container for diagnostics");
                    container_diagnostics.push(diagnostics.clone());
                    on_event(&PackEvent::ContainerDiagnostics {
                        container_id: new_id,
                        diagnostics,
                    });
                    continue 'object_loop;
                }
            }
        }

        let reason = determine_unfit_reason_across_templates(&templates, &obj, &config);
        on_event(&PackEvent::ObjectRejected {
            id: obj.id,
            weight: obj.weight,
            dims: obj.dims,
            reason_code: reason.code().to_string(),
            reason_text: reason.to_string(),
        });
        unplaced.push(UnplacedBox {
            object: obj,
            reason,
        });
    }

    let diagnostics_summary = summarize_diagnostics(container_diagnostics.iter());
    on_event(&PackEvent::Finished {
        containers: containers.len(),
        unplaced: unplaced.len(),
        diagnostics_summary: diagnostics_summary.clone(),
    });
    PackingResult {
        containers,
        unplaced,
        container_diagnostics,
        diagnostics_summary,
    }
}

/// Finds a stable position for an object in a container.
///
/// Searches through different Z-layers, Y and X positions and evaluates each
/// position for stability, support, weight distribution, and balance.
///
/// # Parameters
/// * `b` - The object to place
/// * `cont` - The container
/// * `config` - Configuration parameters
///
/// # Returns
/// `Some((x, y, z))` on successful placement, otherwise `None`
fn find_stable_position(
    b: &Box3D,
    cont: &Container,
    config: &PackingConfig,
) -> Option<(f64, f64, f64)> {
    if !cont.can_fit(b) {
        return None;
    }

    let xs = axis_positions(
        cont.dims.0,
        b.dims.0,
        config.grid_step,
        config.general_epsilon,
    );
    let ys = axis_positions(
        cont.dims.1,
        b.dims.1,
        config.grid_step,
        config.general_epsilon,
    );

    // Collect all relevant Z-layers (floor + tops of all placed objects)
    let mut z_layers: Vec<f64> = cont
        .placed
        .iter()
        .map(|p| p.position.2 + p.object.dims.2)
        .collect();
    z_layers.push(0.0);
    z_layers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    z_layers.dedup_by(|a, b| (*a - *b).abs() < config.height_epsilon);

    let balance_limit = calculate_balance_limit(cont, config);

    let mut best_in_limit: Option<((f64, f64, f64), PlacementScore)> = None;
    let mut best_any: Option<((f64, f64, f64), PlacementScore)> = None;

    for &z in &z_layers {
        if z + b.dims.2 > cont.dims.2 + config.general_epsilon {
            continue;
        }

        for &y in &ys {
            if y + b.dims.1 > cont.dims.1 + config.general_epsilon {
                continue;
            }

            for &x in &xs {
                if x + b.dims.0 > cont.dims.0 + config.general_epsilon {
                    continue;
                }

                let candidate = PlacedBox {
                    object: b.clone(),
                    position: (x, y, z),
                };

                // Check for collisions
                if cont.placed.iter().any(|p| intersects(p, &candidate)) {
                    continue;
                }

                // For placement above the floor: Check stability
                if z > 0.0 {
                    if !has_sufficient_support(&candidate, cont, config) {
                        continue;
                    }
                    if !supports_weight_correctly(&candidate, cont, config) {
                        continue;
                    }
                    if !is_center_supported(&candidate, cont, config) {
                        // Prevents overhangs where the center of gravity is not supported
                        continue;
                    }
                }

                let balance = calculate_balance_after(cont, &candidate);
                let score = PlacementScore { z, y, x, balance };

                update_best(&mut best_any, (x, y, z), score, config);

                if balance <= balance_limit + config.general_epsilon {
                    update_best(&mut best_in_limit, (x, y, z), score, config);
                }
            }
        }
    }

    best_in_limit.or(best_any).map(|(pos, _)| pos)
}

/// Generates possible positions along an axis.
///
/// Creates a grid of positions with the specified step size.
///
/// # Parameters
/// * `container_len` - Length of the container in this dimension
/// * `object_len` - Length of the object in this dimension
/// * `step` - Step size of the grid
/// * `epsilon` - Numerical tolerance
fn axis_positions(container_len: f64, object_len: f64, step: f64, epsilon: f64) -> Vec<f64> {
    let max_pos = (container_len - object_len).max(0.0);
    let mut positions = Vec::new();

    if max_pos <= epsilon {
        positions.push(0.0);
        return positions;
    }

    let mut pos = 0.0;
    while pos <= max_pos + epsilon {
        positions.push(pos.min(max_pos));
        pos += step;
    }

    if let Some(&last) = positions.last() {
        if (last - max_pos).abs() > epsilon {
            positions.push(max_pos);
        }
    } else {
        positions.push(max_pos);
    }

    positions.sort_by(|a, b| a.partial_cmp(b).unwrap());
    positions.dedup_by(|a, b| (*a - *b).abs() < epsilon);
    positions
}

/// Checks if an object is correctly supported by weight.
///
/// Ensures that no heavier objects rest on lighter ones.
///
/// # Parameters
/// * `b` - The placed object to check
/// * `cont` - The container
/// * `config` - Configuration parameters
fn supports_weight_correctly(b: &PlacedBox, cont: &Container, config: &PackingConfig) -> bool {
    if b.position.2 <= config.height_epsilon {
        return true;
    }

    let (bx, by, bz) = b.position;
    let (bw, bd, _) = b.object.dims;
    let mut has_support = false;

    for p in &cont.placed {
        let top_z = p.position.2 + p.object.dims.2;
        if (bz - top_z).abs() > config.height_epsilon {
            continue;
        }

        let over_x = overlap_1d(bx, bx + bw, p.position.0, p.position.0 + p.object.dims.0);
        let over_y = overlap_1d(by, by + bd, p.position.1, p.position.1 + p.object.dims.1);

        if over_x <= 0.0 || over_y <= 0.0 {
            continue;
        }

        has_support = true;

        // Heavier object must not rest on lighter one
        if p.object.weight + config.general_epsilon < b.object.weight {
            return false;
        }
    }

    has_support
}

/// Checks if an object is sufficiently supported.
///
/// Calculates the fraction of the base area resting on other objects.
///
/// # Parameters
/// * `b` - The placed object to check
/// * `cont` - The container
/// * `config` - Configuration parameters
fn support_ratio_of(b: &PlacedBox, cont: &Container, config: &PackingConfig) -> f64 {
    if b.position.2 <= config.height_epsilon {
        return 1.0;
    }

    let (bx, by, bz) = b.position;
    let (bw, bd, _) = b.object.dims;
    let base_area = bw * bd;
    let min_support_area = config.general_epsilon * config.general_epsilon;
    if base_area <= min_support_area {
        return 0.0;
    }

    let mut support_area = 0.0;

    for p in &cont.placed {
        let support_surface_z = p.position.2 + p.object.dims.2;
        if (bz - support_surface_z).abs() > config.height_epsilon {
            continue;
        }

        let over_x = overlap_1d(bx, bx + bw, p.position.0, p.position.0 + p.object.dims.0);
        let over_y = overlap_1d(by, by + bd, p.position.1, p.position.1 + p.object.dims.1);

        if over_x > 0.0 && over_y > 0.0 {
            support_area += over_x * over_y;
        }
    }

    (support_area / base_area).clamp(0.0, 1.0)
}

fn has_sufficient_support(b: &PlacedBox, cont: &Container, config: &PackingConfig) -> bool {
    if b.position.2 <= config.height_epsilon {
        return true;
    }

    let required_support = (config.support_ratio - config.general_epsilon).max(0.0);
    support_ratio_of(b, cont, config) >= required_support
}

/// Checks if the center of gravity of the object (XY projection) is supported by the bearing surface.
///
/// A simple, robust stability heuristic: There must be at least one supporting box directly under
/// the projected center point (same Z-level, XY contains center point).
fn is_center_supported(b: &PlacedBox, cont: &Container, config: &PackingConfig) -> bool {
    if b.position.2 <= config.height_epsilon {
        return true;
    }

    let center_xy = (
        b.position.0 + b.object.dims.0 / 2.0,
        b.position.1 + b.object.dims.1 / 2.0,
        b.position.2,
    );

    for p in &cont.placed {
        let top_z = p.position.2 + p.object.dims.2;
        if (b.position.2 - top_z).abs() > config.height_epsilon {
            continue;
        }

        if point_inside(center_xy, p) {
            return true;
        }
    }
    false
}

/// Calculates the balance/center of gravity deviation after adding an object.
///
/// Computes the weighted center of gravity of all objects and its distance
/// to the geometric center of the container.
///
/// # Parameters
/// * `cont` - The container
/// * `new_box` - The object to add
fn calculate_balance_after(cont: &Container, new_box: &PlacedBox) -> f64 {
    let new_point = (
        new_box.position.0 + new_box.object.dims.0 / 2.0,
        new_box.position.1 + new_box.object.dims.1 / 2.0,
        new_box.object.weight,
    );

    match compute_center_of_mass_xy(
        cont.placed
            .iter()
            .map(|p| {
                (
                    p.position.0 + p.object.dims.0 / 2.0,
                    p.position.1 + p.object.dims.1 / 2.0,
                    p.object.weight,
                )
            })
            .chain(std::iter::once(new_point)),
    ) {
        Some(cm) => distance_2d(cm, container_center_xy(cont)),
        None => 0.0,
    }
}

/// Evaluation of a placement position.
///
/// Lower values are better (z first, then y, then x, then balance).
#[derive(Clone, Copy)]
struct PlacementScore {
    z: f64,
    y: f64,
    x: f64,
    balance: f64,
}

/// Updates the best found position.
///
/// # Parameters
/// * `best` - Currently best position
/// * `position` - New candidate position
/// * `score` - Score of the new position
/// * `config` - Configuration parameters
fn update_best(
    best: &mut Option<((f64, f64, f64), PlacementScore)>,
    position: (f64, f64, f64),
    score: PlacementScore,
    config: &PackingConfig,
) {
    match best {
        None => {
            *best = Some((position, score));
        }
        Some((_, current)) => {
            if is_better_score(score, *current, config) {
                *best = Some((position, score));
            }
        }
    }
}

/// Compares two placement scores.
///
/// Priority: z (low) > y (low) > x (low) > balance (low)
///
/// # Parameters
/// * `new` - New score
/// * `current` - Current score
/// * `config` - Configuration parameters
fn is_better_score(new: PlacementScore, current: PlacementScore, config: &PackingConfig) -> bool {
    match compare_with_epsilon(new.z, current.z, config.height_epsilon) {
        Ordering::Less => return true,
        Ordering::Greater => return false,
        Ordering::Equal => {}
    }

    match compare_with_epsilon(new.y, current.y, config.general_epsilon) {
        Ordering::Less => return true,
        Ordering::Greater => return false,
        Ordering::Equal => {}
    }

    match compare_with_epsilon(new.x, current.x, config.general_epsilon) {
        Ordering::Less => return true,
        Ordering::Greater => return false,
        Ordering::Equal => {}
    }

    new.balance + config.general_epsilon < current.balance
}

/// Compares two values with tolerance.
///
/// # Parameters
/// * `a` - First value
/// * `b` - Second value
/// * `eps` - Tolerance
fn compare_with_epsilon(a: f64, b: f64, eps: f64) -> Ordering {
    if (a - b).abs() <= eps {
        Ordering::Equal
    } else if a < b {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Calculates the maximum allowed balance deviation.
///
/// # Parameters
/// * `cont` - The container
/// * `config` - Configuration parameters
fn calculate_balance_limit(cont: &Container, config: &PackingConfig) -> f64 {
    let half_x = cont.dims.0 / 2.0;
    let half_y = cont.dims.1 / 2.0;
    (half_x.powi(2) + half_y.powi(2)).sqrt() * config.balance_limit_ratio
}

fn calculate_current_balance_offset(cont: &Container) -> f64 {
    if cont.placed.is_empty() {
        return 0.0;
    }

    match compute_center_of_mass_xy(cont.placed.iter().map(|p| {
        (
            p.position.0 + p.object.dims.0 / 2.0,
            p.position.1 + p.object.dims.1 / 2.0,
            p.object.weight,
        )
    })) {
        Some(cm) => distance_2d(cm, container_center_xy(cont)),
        None => 0.0,
    }
}

fn container_center_xy(cont: &Container) -> (f64, f64) {
    (cont.dims.0 / 2.0, cont.dims.1 / 2.0)
}

fn distance_2d(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn compute_center_of_mass_xy<I>(points: I) -> Option<(f64, f64)>
where
    I: Iterator<Item = (f64, f64, f64)>,
{
    let mut total_w = 0.0;
    let mut x_c = 0.0;
    let mut y_c = 0.0;

    for (x, y, w) in points {
        total_w += w;
        x_c += x * w;
        y_c += y * w;
    }

    if total_w <= 0.0 {
        None
    } else {
        Some((x_c / total_w, y_c / total_w))
    }
}

/// Calculates diagnostic metrics for a container.
pub fn compute_container_diagnostics(
    cont: &Container,
    config: &PackingConfig,
) -> ContainerDiagnostics {
    let balance_limit = calculate_balance_limit(cont, config);
    let center_offset = calculate_current_balance_offset(cont);

    let imbalance_ratio = if balance_limit > config.general_epsilon {
        center_offset / balance_limit
    } else {
        0.0
    };

    let mut support_samples = Vec::with_capacity(cont.placed.len());
    let mut total_support = 0.0;
    let mut min_support: f64 = 1.0;

    for placed in &cont.placed {
        let ratio = support_ratio_of(placed, cont, config);
        total_support += ratio;
        min_support = min_support.min(ratio);
        support_samples.push(SupportDiagnostics {
            object_id: placed.object.id,
            support_percent: ratio * 100.0,
            rests_on_floor: placed.position.2 <= config.height_epsilon,
        });
    }

    let count = cont.placed.len() as f64;
    let average_support_percent = if count > 0.0 {
        (total_support / count) * 100.0
    } else {
        100.0
    };
    let minimum_support_percent = if cont.placed.is_empty() {
        100.0
    } else {
        min_support * 100.0
    };

    ContainerDiagnostics {
        center_of_mass_offset: center_offset,
        balance_limit,
        imbalance_ratio,
        average_support_percent,
        minimum_support_percent,
        support_samples,
    }
}

struct SummaryAccumulator {
    max_imbalance_ratio: f64,
    worst_support_percent: f64,
    support_percent_sum: f64,
    support_sample_count: usize,
}

impl SummaryAccumulator {
    fn new() -> Self {
        Self {
            max_imbalance_ratio: 0.0,
            worst_support_percent: 100.0,
            support_percent_sum: 0.0,
            support_sample_count: 0,
        }
    }

    fn record(&mut self, diagnostics: &ContainerDiagnostics) {
        self.max_imbalance_ratio = self.max_imbalance_ratio.max(diagnostics.imbalance_ratio);
        self.worst_support_percent = self
            .worst_support_percent
            .min(diagnostics.minimum_support_percent);

        let sample_count = diagnostics.support_samples.len();
        if sample_count > 0 {
            let support_sum: f64 = diagnostics
                .support_samples
                .iter()
                .map(|sample| sample.support_percent)
                .sum();
            self.support_percent_sum += support_sum;
            self.support_sample_count += sample_count;
        }
    }

    fn finish(self) -> PackingDiagnosticsSummary {
        let average_support_percent = if self.support_sample_count > 0 {
            self.support_percent_sum / self.support_sample_count as f64
        } else {
            100.0
        };

        PackingDiagnosticsSummary {
            max_imbalance_ratio: self.max_imbalance_ratio,
            worst_support_percent: self.worst_support_percent,
            average_support_percent,
        }
    }
}

/// Aggregates diagnostics across multiple containers.
pub fn summarize_diagnostics<'a, I>(diagnostics: I) -> PackingDiagnosticsSummary
where
    I: IntoIterator<Item = &'a ContainerDiagnostics>,
{
    let mut acc = SummaryAccumulator::new();
    for diag in diagnostics {
        acc.record(diag);
    }
    acc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_blueprint(dims: (f64, f64, f64), max_weight: f64) -> Vec<ContainerBlueprint> {
        vec![ContainerBlueprint::new(0, None, dims, max_weight).unwrap()]
    }

    fn assert_heavy_below(cont: &Container, config: &PackingConfig) {
        for lower in &cont.placed {
            let lower_top = lower.position.2 + lower.object.dims.2;

            for upper in &cont.placed {
                if std::ptr::eq(lower, upper) {
                    continue;
                }

                let upper_bottom = upper.position.2;
                if upper_bottom + config.height_epsilon < lower_top - config.height_epsilon {
                    continue;
                }

                let overlap_x = overlap_1d(
                    lower.position.0,
                    lower.position.0 + lower.object.dims.0,
                    upper.position.0,
                    upper.position.0 + upper.object.dims.0,
                );
                let overlap_y = overlap_1d(
                    lower.position.1,
                    lower.position.1 + lower.object.dims.1,
                    upper.position.1,
                    upper.position.1 + upper.object.dims.1,
                );

                if overlap_x <= config.general_epsilon || overlap_y <= config.general_epsilon {
                    continue;
                }

                assert!(
                    lower.object.weight + config.general_epsilon >= upper.object.weight,
                    "Object {} ({}kg) under object {} ({}kg) violates weight sorting",
                    lower.object.id,
                    lower.object.weight,
                    upper.object.id,
                    upper.object.weight
                );
            }
        }
    }

    #[test]
    fn heavy_boxes_stay_below_lighter() {
        let objects = vec![
            Box3D {
                id: 1,
                dims: (10.0, 10.0, 10.0),
                weight: 10.0,
            },
            Box3D {
                id: 2,
                dims: (10.0, 10.0, 10.0),
                weight: 4.0,
            },
        ];

        let result = pack_objects(objects, single_blueprint((10.0, 10.0, 30.0), 100.0));
        assert!(result.unplaced.is_empty());
        assert_eq!(result.containers.len(), 1);

        let placements = &result.containers[0].placed;
        assert_eq!(placements.len(), 2);

        let config = PackingConfig::default();
        let bottom_weight = placements
            .iter()
            .filter(|p| (p.position.2 - 0.0).abs() < config.height_epsilon)
            .map(|p| p.object.weight)
            .sum::<f64>();
        let top_weight = placements
            .iter()
            .filter(|p| p.position.2 > config.height_epsilon)
            .map(|p| p.object.weight)
            .sum::<f64>();

        assert!(bottom_weight >= top_weight);
    }

    #[test]
    fn single_box_snaps_to_corner() {
        let config = PackingConfig::default();

        let objects = vec![Box3D {
            id: 1,
            dims: (10.0, 10.0, 10.0),
            weight: 10.0,
        }];

        let result = pack_objects(objects, single_blueprint((20.0, 20.0, 20.0), 100.0));
        assert!(result.unplaced.is_empty());
        let placement = &result.containers[0].placed[0];

        assert!(placement.position.0 <= config.grid_step + config.general_epsilon);
        assert!(placement.position.1 <= config.grid_step + config.general_epsilon);
        assert!((placement.position.2 - 0.0).abs() < config.general_epsilon * 10.0);
    }

    #[test]
    fn creates_additional_containers_when_weight_exceeded() {
        let objects = vec![
            Box3D {
                id: 1,
                dims: (10.0, 10.0, 10.0),
                weight: 300.0,
            },
            Box3D {
                id: 2,
                dims: (10.0, 10.0, 10.0),
                weight: 300.0,
            },
            Box3D {
                id: 3,
                dims: (10.0, 10.0, 10.0),
                weight: 300.0,
            },
        ];

        let result = pack_objects(objects, single_blueprint((20.0, 20.0, 20.0), 400.0));
        assert_eq!(result.containers.len(), 3);
        assert!(result.unplaced.is_empty());
        for cont in &result.containers {
            assert_eq!(cont.placed.len(), 1);
        }
    }

    #[test]
    fn reports_objects_too_large_for_container() {
        let objects = vec![Box3D {
            id: 1,
            dims: (12.0, 9.0, 8.0),
            weight: 5.0,
        }];

        let result = pack_objects(objects, single_blueprint((10.0, 10.0, 10.0), 100.0));
        assert!(result.containers.is_empty());
        assert_eq!(result.unplaced.len(), 1);
        assert_eq!(result.unplaced[0].object.id, 1);
        assert!(matches!(
            result.unplaced[0].reason,
            UnplacedReason::DimensionsExceedContainer
        ));
    }

    #[test]
    fn reports_objects_too_heavy_for_container() {
        let objects = vec![Box3D {
            id: 1,
            dims: (5.0, 5.0, 5.0),
            weight: 25.0,
        }];

        let result = pack_objects(objects, single_blueprint((10.0, 10.0, 10.0), 10.0));
        assert!(result.containers.is_empty());
        assert_eq!(result.unplaced.len(), 1);
        assert!(matches!(
            result.unplaced[0].reason,
            UnplacedReason::TooHeavyForContainer
        ));
    }

    #[test]
    fn selects_matching_container_type() {
        let templates = vec![
            ContainerBlueprint::new(0, Some("Small".to_string()), (12.0, 12.0, 12.0), 30.0)
                .unwrap(),
            ContainerBlueprint::new(1, Some("Large".to_string()), (40.0, 40.0, 40.0), 100.0)
                .unwrap(),
        ];

        let objects = vec![
            Box3D {
                id: 1,
                dims: (30.0, 30.0, 20.0),
                weight: 90.0,
            },
            Box3D {
                id: 2,
                dims: (10.0, 10.0, 10.0),
                weight: 15.0,
            },
            Box3D {
                id: 3,
                dims: (8.0, 8.0, 8.0),
                weight: 10.0,
            },
        ];

        let result = pack_objects(objects, templates);
        assert_eq!(result.container_count(), 2);
        let mut dims: Vec<_> = result.containers.iter().map(|c| c.dims).collect();
        dims.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        assert_eq!(dims[0], (12.0, 12.0, 12.0));
        assert_eq!(dims[1], (40.0, 40.0, 40.0));
    }

    #[test]
    fn rejects_when_no_template_fits_dimensions() {
        let templates = vec![
            ContainerBlueprint::new(0, None, (10.0, 10.0, 10.0), 100.0).unwrap(),
            ContainerBlueprint::new(1, None, (12.0, 12.0, 12.0), 120.0).unwrap(),
        ];

        let objects = vec![Box3D {
            id: 1,
            dims: (15.0, 12.0, 12.0),
            weight: 20.0,
        }];

        let result = pack_objects(objects, templates);
        assert!(result.containers.is_empty());
        assert_eq!(result.unplaced.len(), 1);
        assert!(matches!(
            result.unplaced[0].reason,
            UnplacedReason::DimensionsExceedContainer
        ));
    }

    #[test]
    fn reject_heavier_on_light_support() {
        let config = PackingConfig::default();
        let mut container = Container::new((10.0, 10.0, 30.0), 100.0).unwrap();

        container.placed.push(PlacedBox {
            object: Box3D {
                id: 1,
                dims: (10.0, 10.0, 10.0),
                weight: 5.0,
            },
            position: (0.0, 0.0, 0.0),
        });

        let heavy_box = Box3D {
            id: 2,
            dims: (10.0, 10.0, 10.0),
            weight: 9.0,
        };

        assert!(find_stable_position(&heavy_box, &container, &config).is_none());
    }

    #[test]
    fn rotation_toggle_controls_reorientation() {
        let object = Box3D {
            id: 1,
            dims: (80.0, 40.0, 60.0),
            weight: 10.0,
        };
        let templates = single_blueprint((60.0, 80.0, 40.0), 100.0);

        let mut config = PackingConfig::default();
        config.allow_item_rotation = false;
        let result_without_rotation =
            pack_objects_with_config(vec![object.clone()], templates.clone(), config);
        assert_eq!(result_without_rotation.unplaced.len(), 1);
        assert!(matches!(
            result_without_rotation.unplaced[0].reason,
            UnplacedReason::DimensionsExceedContainer
        ));

        config.allow_item_rotation = true;
        let result_with_rotation = pack_objects_with_config(vec![object], templates, config);
        assert!(result_with_rotation.unplaced.is_empty());
        assert_eq!(result_with_rotation.containers.len(), 1);
        let placed_dims = result_with_rotation.containers[0].placed[0].object.dims;
        assert_eq!(placed_dims, (60.0, 80.0, 40.0));
    }

    #[test]
    fn orientation_deduplication_handles_equal_dimensions() {
        // Test cube (all dimensions equal) - should produce only 1 unique orientation
        let cube = Box3D {
            id: 1,
            dims: (50.0, 50.0, 50.0),
            weight: 10.0,
        };
        let cube_orientations = orientations_for(&cube, true);
        assert_eq!(
            cube_orientations.len(),
            1,
            "Cube should produce only 1 unique orientation, got {}",
            cube_orientations.len()
        );
        assert_eq!(cube_orientations[0].dims, (50.0, 50.0, 50.0));

        // Test rectangular prism with two equal dimensions - should produce 3 unique orientations
        // (30, 30, 60), (30, 60, 30), and (60, 30, 30)
        let rect_prism = Box3D {
            id: 2,
            dims: (30.0, 30.0, 60.0),
            weight: 10.0,
        };
        let rect_orientations = orientations_for(&rect_prism, true);
        assert_eq!(
            rect_orientations.len(),
            3,
            "Rectangular prism with two equal dimensions should produce 3 unique orientations, got {}",
            rect_orientations.len()
        );

        // Verify all orientations are unique
        // Using O(n²) is acceptable here since we're checking only 3 items
        for i in 0..rect_orientations.len() {
            for j in (i + 1)..rect_orientations.len() {
                assert_ne!(
                    rect_orientations[i].dims, rect_orientations[j].dims,
                    "Orientations at indices {} and {} are duplicates: {:?}",
                    i, j, rect_orientations[i].dims
                );
            }
        }

        // Test fully distinct dimensions - should produce 6 unique orientations
        let distinct = Box3D {
            id: 3,
            dims: (20.0, 30.0, 40.0),
            weight: 10.0,
        };
        let distinct_orientations = orientations_for(&distinct, true);
        assert_eq!(
            distinct_orientations.len(),
            6,
            "Object with all distinct dimensions should produce 6 unique orientations, got {}",
            distinct_orientations.len()
        );

        // Verify all 6 orientations are unique
        // Using O(n²) is acceptable here since we're checking only 6 items
        for i in 0..distinct_orientations.len() {
            for j in (i + 1)..distinct_orientations.len() {
                assert_ne!(
                    distinct_orientations[i].dims, distinct_orientations[j].dims,
                    "Orientations at indices {} and {} are duplicates: {:?}",
                    i, j, distinct_orientations[i].dims
                );
            }
        }

        // Test with rotation disabled - should always return 1 orientation
        let no_rotation = orientations_for(&distinct, false);
        assert_eq!(
            no_rotation.len(),
            1,
            "With rotation disabled, should produce only 1 orientation"
        );
        assert_eq!(no_rotation[0].dims, distinct.dims);
    }

    #[test]
    fn sample_pack_respects_weight_order() {
        let config = PackingConfig::default();

        let objects = vec![
            Box3D {
                id: 1,
                dims: (30.0, 30.0, 20.0),
                weight: 50.0,
            },
            Box3D {
                id: 2,
                dims: (20.0, 40.0, 25.0),
                weight: 30.0,
            },
            Box3D {
                id: 3,
                dims: (10.0, 20.0, 10.0),
                weight: 10.0,
            },
            Box3D {
                id: 4,
                dims: (50.0, 40.0, 30.0),
                weight: 70.0,
            },
            Box3D {
                id: 5,
                dims: (60.0, 50.0, 40.0),
                weight: 90.0,
            },
        ];

        let results = pack_objects(objects, single_blueprint((100.0, 100.0, 100.0), 500.0));
        assert!(results.unplaced.is_empty());
        assert!(!results.containers.is_empty());
        for cont in &results.containers {
            assert_heavy_below(cont, &config);
        }

        let primary = &results.containers[0];
        let heavy = primary
            .placed
            .iter()
            .find(|p| p.object.id == 5)
            .expect("heaviest object missing");
        assert!(heavy.position.0 <= config.grid_step + config.general_epsilon);
        assert!(heavy.position.1 <= config.grid_step + config.general_epsilon);

        let second = primary
            .placed
            .iter()
            .find(|p| p.object.id == 4)
            .expect("second heaviest object missing");
        assert!(second.position.2 <= config.height_epsilon);
    }

    #[test]
    fn footprint_cluster_groups_similar_dimensions() {
        let strategy =
            FootprintClusterStrategy::new(PackingConfig::DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE);
        let mut objects = vec![
            Box3D {
                id: 1,
                dims: (20.0, 10.0, 10.0),
                weight: 30.0,
            },
            Box3D {
                id: 2,
                dims: (20.4, 10.1, 9.5),
                weight: 28.0,
            },
            Box3D {
                id: 3,
                dims: (5.0, 5.0, 5.0),
                weight: 12.0,
            },
        ];

        objects.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
        let reordered = strategy.reorder(objects.clone());

        assert_eq!(reordered.len(), objects.len());
        assert_eq!(reordered[0].id, 1);
        assert_eq!(reordered[1].id, 2);
        assert_eq!(reordered[2].id, 3);
    }

    #[test]
    fn diagnostics_capture_support_and_balance_metrics() {
        let config = PackingConfig::default();
        let mut container = Container::new((10.0, 10.0, 30.0), 200.0).unwrap();

        container.placed.push(PlacedBox {
            object: Box3D {
                id: 1,
                dims: (5.0, 10.0, 10.0),
                weight: 8.0,
            },
            position: (0.0, 0.0, 0.0),
        });

        container.placed.push(PlacedBox {
            object: Box3D {
                id: 2,
                dims: (10.0, 10.0, 8.0),
                weight: 5.0,
            },
            position: (0.0, 0.0, 10.0),
        });

        let diagnostics = compute_container_diagnostics(&container, &config);

        assert_eq!(diagnostics.support_samples.len(), 2);
        let min_support = diagnostics.minimum_support_percent;
        assert!((min_support - 50.0).abs() < 1e-6);

        let avg_support = diagnostics.average_support_percent;
        assert!((avg_support - 75.0).abs() < 1e-6);

        assert!(diagnostics.imbalance_ratio > 0.0);
        assert!(diagnostics.center_of_mass_offset > 0.0);

        let summary = summarize_diagnostics(std::iter::once(&diagnostics));
        assert!((summary.average_support_percent - 75.0).abs() < 1e-6);
        assert!((summary.worst_support_percent - 50.0).abs() < 1e-6);
        assert!((summary.max_imbalance_ratio - diagnostics.imbalance_ratio).abs() < 1e-6);
    }

    #[test]
    fn progress_emits_diagnostics_events() {
        let config = PackingConfig::default();
        let objects = vec![
            Box3D {
                id: 1,
                dims: (10.0, 10.0, 10.0),
                weight: 8.0,
            },
            Box3D {
                id: 2,
                dims: (5.0, 5.0, 5.0),
                weight: 3.0,
            },
        ];

        let mut diagnostics_events = 0usize;
        pack_objects_with_progress(
            objects,
            single_blueprint((20.0, 20.0, 30.0), 100.0),
            config,
            |evt| {
                if matches!(evt, PackEvent::ContainerDiagnostics { .. }) {
                    diagnostics_events += 1;
                }
            },
        );

        assert!(diagnostics_events >= 1);
    }
}
