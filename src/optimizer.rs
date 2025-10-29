//! Optimierungslogik für die 3D-Verpackung von Objekten.
//!
//! Dieser Modul implementiert einen heuristischen Algorithmus zur effizienten Platzierung
//! von Objekten in Containern unter Berücksichtigung von:
//! - Gewichtsgrenzen und -verteilung
//! - Stabilität und Unterstützung
//! - Schwerpunkt-Balance
//! - Schichtung (schwere Objekte unten)

use std::cmp::Ordering;

use crate::geometry::{intersects, overlap_1d, point_inside};
use crate::model::{Box3D, Container, ContainerBlueprint, PlacedBox};
use utoipa::ToSchema;

/// Konfiguration für den Packing-Algorithmus.
///
/// Enthält alle Toleranzen und Grenzwerte zur Steuerung des Optimierungsverhaltens.
#[derive(Copy, Clone, Debug)]
pub struct PackingConfig {
    /// Schrittweite für Positionsraster (kleinere Werte = genauer, aber langsamer)
    pub grid_step: f64,
    /// Minimaler Anteil der Grundfläche, der unterstützt sein muss (0.0 bis 1.0)
    pub support_ratio: f64,
    /// Toleranz für Höhenvergleiche
    pub height_epsilon: f64,
    /// Allgemeine numerische Toleranz
    pub general_epsilon: f64,
    /// Maximale erlaubte Abweichung des Schwerpunkts vom Mittelpunkt (als Ratio der Diagonale)
    pub balance_limit_ratio: f64,
    /// Relative Toleranz bei der Vorgruppierung nach Grundfläche zur Reduktion von Backtracking
    pub footprint_cluster_tolerance: f64,
}

impl PackingConfig {
    pub const DEFAULT_GRID_STEP: f64 = 5.0;
    pub const DEFAULT_SUPPORT_RATIO: f64 = 0.6;
    pub const DEFAULT_HEIGHT_EPSILON: f64 = 1e-3;
    pub const DEFAULT_GENERAL_EPSILON: f64 = 1e-6;
    pub const DEFAULT_BALANCE_LIMIT_RATIO: f64 = 0.45;
    pub const DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE: f64 = 0.15;

    /// Erstellt einen Builder für benutzerdefinierte Konfiguration.
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
        }
    }
}

/// Builder-Pattern für PackingConfig (OOP-Prinzip).
#[derive(Clone, Debug)]
pub struct PackingConfigBuilder {
    config: PackingConfig,
}

impl Default for PackingConfigBuilder {
    fn default() -> Self {
        Self {
            config: PackingConfig::default(),
        }
    }
}

impl PackingConfigBuilder {
    /// Setzt die Raster-Schrittweite.
    pub fn grid_step(mut self, step: f64) -> Self {
        self.config.grid_step = step;
        self
    }

    /// Setzt die minimale Unterstützungsrate.
    pub fn support_ratio(mut self, ratio: f64) -> Self {
        self.config.support_ratio = ratio;
        self
    }

    /// Setzt die Höhentoleranz.
    pub fn height_epsilon(mut self, epsilon: f64) -> Self {
        self.config.height_epsilon = epsilon;
        self
    }

    /// Setzt die allgemeine Toleranz.
    pub fn general_epsilon(mut self, epsilon: f64) -> Self {
        self.config.general_epsilon = epsilon;
        self
    }

    /// Setzt das Balance-Limit als Ratio der Diagonale.
    pub fn balance_limit_ratio(mut self, ratio: f64) -> Self {
        self.config.balance_limit_ratio = ratio;
        self
    }

    /// Setzt die Toleranz für die Vorgruppierung basierend auf der Grundfläche.
    pub fn footprint_cluster_tolerance(mut self, tolerance: f64) -> Self {
        self.config.footprint_cluster_tolerance = tolerance;
        self
    }

    /// Erstellt die finale Konfiguration.
    pub fn build(self) -> PackingConfig {
        self.config
    }
}

/// Abstrakte Strategien zur Gruppierung/Neuordnung von Objekten vor dem Packen.
///
/// Diese interne Trait definiert die Schnittstelle für Strategien, die die Reihenfolge
/// (und ggf. Auswahl) von Objekten vor dem Packvorgang beeinflussen. Implementierungen
/// können die Reihenfolge der Objekte ändern, Gruppen bilden oder Objekte filtern, um
/// die Effizienz des Packens zu verbessern. Es wird garantiert, dass die Rückgabe
/// eine (ggf. gefilterte) Teilmenge der Eingabe ist; Objekte können entfernt, aber
/// nicht modifiziert werden. Die Trait ist absichtlich privat, da sie nur für interne
/// Optimierungsstrategien gedacht ist und keine stabile API garantiert.
trait ObjectClusterStrategy {
    fn reorder(&self, objects: Vec<Box3D>) -> Vec<Box3D>;
}

/// Gruppiert Objekte mit kompatibler Grundfläche, um Backtracking zu reduzieren.
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

/// Support-Kennzahlen pro Objekt.
#[derive(Clone, Debug, serde::Serialize, ToSchema)]
pub struct SupportDiagnostics {
    pub object_id: usize,
    pub support_percent: f64,
    pub rests_on_floor: bool,
}

/// Diagnostische Kennzahlen pro Container für Monitoring.
#[derive(Clone, Debug, serde::Serialize, ToSchema)]
pub struct ContainerDiagnostics {
    pub center_of_mass_offset: f64,
    pub balance_limit: f64,
    pub imbalance_ratio: f64,
    pub average_support_percent: f64,
    pub minimum_support_percent: f64,
    pub support_samples: Vec<SupportDiagnostics>,
}

/// Zusammenfassung wichtiger Kennzahlen über alle Container hinweg.
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

/// Ergebnis der Verpackungsberechnung.
#[derive(Clone, Debug)]
pub struct PackingResult {
    pub containers: Vec<Container>,
    pub unplaced: Vec<UnplacedBox>,
    pub container_diagnostics: Vec<ContainerDiagnostics>,
    pub diagnostics_summary: PackingDiagnosticsSummary,
}

impl PackingResult {
    /// Gibt an, ob alle Objekte verpackt wurden.
    pub fn is_complete(&self) -> bool {
        self.unplaced.is_empty()
    }

    /// Gibt die Gesamtanzahl der Container zurück.
    pub fn container_count(&self) -> usize {
        self.containers.len()
    }

    /// Gibt die Anzahl unverpackter Objekte zurück.
    pub fn unplaced_count(&self) -> usize {
        self.unplaced.len()
    }

    /// Berechnet die durchschnittliche Auslastung aller Container.
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

    /// Berechnet das Gesamtgewicht aller verpackten Objekte.
    pub fn total_packed_weight(&self) -> f64 {
        self.containers.iter().map(|c| c.total_weight()).sum()
    }

    /// Liefert die aggregierten Diagnosewerte.
    pub fn diagnostics_summary(&self) -> &PackingDiagnosticsSummary {
        &self.diagnostics_summary
    }
}

/// Objekt, das nicht platziert werden konnte.
#[derive(Clone, Debug)]
pub struct UnplacedBox {
    pub object: Box3D,
    pub reason: UnplacedReason,
}

/// Gründe, warum ein Objekt nicht platziert werden konnte.
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
                write!(f, "Objekt überschreitet das zulässige Gesamtgewicht")
            }
            UnplacedReason::DimensionsExceedContainer => {
                write!(
                    f,
                    "Objekt passt in mindestens einer Dimension nicht in den Container"
                )
            }
            UnplacedReason::NoStablePosition => {
                write!(
                    f,
                    "Keine stabile Position innerhalb des Containers gefunden"
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

    let dimension_blocked = templates.iter().all(|tpl| {
        object.dims.0 > tpl.dims.0 + config.general_epsilon
            || object.dims.1 > tpl.dims.1 + config.general_epsilon
            || object.dims.2 > tpl.dims.2 + config.general_epsilon
    });
    if dimension_blocked {
        return UnplacedReason::DimensionsExceedContainer;
    }

    UnplacedReason::NoStablePosition
}

/// Hauptfunktion zur Verpackung von Objekten in Container.
///
/// Sortiert Objekte nach Gewicht und Volumen (schwere/große zuerst) und platziert
/// sie nacheinander in Container. Erstellt neue Container, wenn nötig.
///
/// # Parameter
/// * `objects` - Liste der zu verpackenden Objekte
/// * `container_templates` - Mögliche Container-Typen
///
/// # Rückgabewert
/// `PackingResult` mit platzierten Containern und ggf. unverpackten Objekten
pub fn pack_objects(
    objects: Vec<Box3D>,
    container_templates: Vec<ContainerBlueprint>,
) -> PackingResult {
    pack_objects_with_config(objects, container_templates, PackingConfig::default())
}

/// Verpackung mit benutzerdefinierter Konfiguration.
///
/// Wie `pack_objects`, aber mit anpassbaren Parametern.
///
/// # Parameter
/// * `objects` - Liste der zu verpackenden Objekte
/// * `container_templates` - Mögliche Container-Typen
/// * `config` - Konfigurationsparameter für den Algorithmus
pub fn pack_objects_with_config(
    objects: Vec<Box3D>,
    container_templates: Vec<ContainerBlueprint>,
    config: PackingConfig,
) -> PackingResult {
    pack_objects_with_progress(objects, container_templates, config, |_| {})
}

/// Ereignisse, die während des Packens auftreten, um Live-Visualisierung zu ermöglichen.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum PackEvent {
    /// Ein neuer Container wird begonnen.
    ContainerStarted {
        id: usize,
        dims: (f64, f64, f64),
        max_weight: f64,
        label: Option<String>,
        template_id: Option<usize>,
    },
    /// Ein Objekt wurde platziert.
    ObjectPlaced {
        container_id: usize,
        id: usize,
        pos: (f64, f64, f64),
        weight: f64,
        dims: (f64, f64, f64),
        total_weight: f64,
    },
    /// Aktualisierte Diagnostik eines Containers.
    ContainerDiagnostics {
        container_id: usize,
        diagnostics: ContainerDiagnostics,
    },
    /// Ein Objekt konnte nicht platziert werden.
    ObjectRejected {
        id: usize,
        weight: f64,
        dims: (f64, f64, f64),
        reason_code: String,
        reason_text: String,
    },
    /// Packen abgeschlossen.
    Finished {
        containers: usize,
        unplaced: usize,
        diagnostics_summary: PackingDiagnosticsSummary,
    },
}

/// Verpackung mit benutzerdefinierter Konfiguration und Live-Progress Callback.
///
/// Ruft für jeden wichtigen Schritt ein Callback auf (geeignet für SSE/WebSocket).
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

    // Sortierung: Schwere und große Objekte zuerst (Stabilitätsprinzip)
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
    for obj in objects {
        let mut target: Option<(usize, (f64, f64, f64))> = None;

        // Versuche, in bestehenden Container zu platzieren
        for idx in 0..containers.len() {
            if !containers[idx].can_fit(&obj) {
                continue;
            }
            if let Some(position) = find_stable_position(&obj, &containers[idx], &config) {
                target = Some((idx, position));
                break;
            }
        }

        // Platziere in gefundenem Container oder erstelle neuen
        if let Some((idx, position)) = target {
            containers[idx].placed.push(PlacedBox {
                object: obj,
                position,
            });
            let total_w = containers[idx].total_weight();
            on_event(&PackEvent::ObjectPlaced {
                container_id: idx + 1,
                id: containers[idx].placed.last().unwrap().object.id,
                pos: position,
                weight: containers[idx].placed.last().unwrap().object.weight,
                dims: containers[idx].placed.last().unwrap().object.dims,
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
        } else {
            match templates.iter().position(|tpl| tpl.can_fit(&obj)) {
                Some(template_index) => {
                    let template = &templates[template_index];
                    let mut new_container = template.instantiate();
                    let new_id = containers.len() + 1;
                    match find_stable_position(&obj, &new_container, &config) {
                        Some(position) => {
                            new_container.placed.push(PlacedBox {
                                object: obj,
                                position,
                            });
                            let total_w = new_container.total_weight();
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
                        }
                        None => {
                            let reason = UnplacedReason::NoStablePosition;
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
                    }
                }
                None => {
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
            }
        }
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

/// Findet eine stabile Position für ein Objekt in einem Container.
///
/// Durchsucht verschiedene Z-Ebenen, Y- und X-Positionen und bewertet jede
/// Position nach Stabilität, Unterstützung, Gewichtsverteilung und Balance.
///
/// # Parameter
/// * `b` - Das zu platzierende Objekt
/// * `cont` - Der Container
/// * `config` - Konfigurationsparameter
///
/// # Rückgabewert
/// `Some((x, y, z))` bei erfolgreicher Platzierung, sonst `None`
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

    // Sammle alle relevanten Z-Ebenen (Boden + Oberseiten aller platzierten Objekte)
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

                // Prüfe auf Kollisionen
                if cont.placed.iter().any(|p| intersects(p, &candidate)) {
                    continue;
                }

                // Bei Platzierung über dem Boden: Prüfe Stabilität
                if z > 0.0 {
                    if !has_sufficient_support(&candidate, cont, config) {
                        continue;
                    }
                    if !supports_weight_correctly(&candidate, cont, config) {
                        continue;
                    }
                    if !is_center_supported(&candidate, cont, config) {
                        // Verhindert Überhänge, bei denen der Schwerpunkt nicht abgestützt ist
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

/// Generiert mögliche Positionen entlang einer Achse.
///
/// Erstellt ein Raster von Positionen mit der angegebenen Schrittweite.
///
/// # Parameter
/// * `container_len` - Länge des Containers in dieser Dimension
/// * `object_len` - Länge des Objekts in dieser Dimension
/// * `step` - Schrittweite des Rasters
/// * `epsilon` - Numerische Toleranz
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

/// Prüft, ob ein Objekt korrekt nach Gewicht unterstützt wird.
///
/// Stellt sicher, dass keine schwereren Objekte auf leichteren liegen.
///
/// # Parameter
/// * `b` - Das zu prüfende platzierte Objekt
/// * `cont` - Der Container
/// * `config` - Konfigurationsparameter
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

        // Schwereres Objekt darf nicht auf leichterem liegen
        if p.object.weight + config.general_epsilon < b.object.weight {
            return false;
        }
    }

    has_support
}

/// Prüft, ob ein Objekt ausreichend unterstützt wird.
///
/// Berechnet den Anteil der Grundfläche, der auf anderen Objekten aufliegt.
///
/// # Parameters
/// * `b` - Das zu prüfende platzierte Objekt
/// * `cont` - Der Container
/// * `config` - Konfigurationsparameter
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

/// Prüft, ob der Schwerpunkt des Objekts (Projektion auf XY) von der Auflagefläche getragen wird.
///
/// Eine einfache, robuste Stabilitätsheuristik: Es muss mindestens eine tragende Box direkt unter
/// dem projizierten Mittelpunkt liegen (gleiche Z-Ebene, XY enthält Center-Punkt).
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

/// Berechnet die Balance/Schwerpunktabweichung nach Hinzufügen eines Objekts.
///
/// Berechnet den gewichteten Schwerpunkt aller Objekte und dessen Distanz
/// zum geometrischen Mittelpunkt des Containers.
///
/// # Parameter
/// * `cont` - Der Container
/// * `new_box` - Das hinzuzufügende Objekt
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

/// Bewertung einer Platzierungsposition.
///
/// Niedrigere Werte sind besser (z zuerst, dann y, dann x, dann balance).
#[derive(Clone, Copy)]
struct PlacementScore {
    z: f64,
    y: f64,
    x: f64,
    balance: f64,
}

/// Aktualisiert die beste gefundene Position.
///
/// # Parameters
/// * `best` - Aktuell beste Position
/// * `position` - Neue Kandidaten-Position
/// * `score` - Score der neuen Position
/// * `config` - Konfigurationsparameter
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

/// Vergleicht zwei Platzierungsscores.
///
/// Priorität: z (niedrig) > y (niedrig) > x (niedrig) > balance (niedrig)
///
/// # Parameters
/// * `new` - Neuer Score
/// * `current` - Aktueller Score
/// * `config` - Konfigurationsparameter
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

/// Vergleicht zwei Werte mit Toleranz.
///
/// # Parameters
/// * `a` - Erster Wert
/// * `b` - Zweiter Wert
/// * `eps` - Toleranz
fn compare_with_epsilon(a: f64, b: f64, eps: f64) -> Ordering {
    if (a - b).abs() <= eps {
        Ordering::Equal
    } else if a < b {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Berechnet die maximale erlaubte Balance-Abweichung.
///
/// # Parameters
/// * `cont` - Der Container
/// * `config` - Konfigurationsparameter
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

/// Berechnet diagnostische Kennzahlen für einen Container.
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

/// Aggregiert Diagnosen über mehrere Container.
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
                    "Objekt {} ({}kg) unter Objekt {} ({}kg) verletzt Gewichtssortierung",
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
            .expect("schwerstes Objekt fehlt");
        assert!(heavy.position.0 <= config.grid_step + config.general_epsilon);
        assert!(heavy.position.1 <= config.grid_step + config.general_epsilon);

        let second = primary
            .placed
            .iter()
            .find(|p| p.object.id == 4)
            .expect("zweit schwerstes Objekt fehlt");
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
