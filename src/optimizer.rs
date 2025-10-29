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
}

impl PackingConfig {
    pub const DEFAULT_GRID_STEP: f64 = 5.0;
    pub const DEFAULT_SUPPORT_RATIO: f64 = 0.6;
    pub const DEFAULT_HEIGHT_EPSILON: f64 = 1e-3;
    pub const DEFAULT_GENERAL_EPSILON: f64 = 1e-6;
    pub const DEFAULT_BALANCE_LIMIT_RATIO: f64 = 0.45;

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

    /// Erstellt die finale Konfiguration.
    pub fn build(self) -> PackingConfig {
        self.config
    }
}

/// Ergebnis der Verpackungsberechnung.
#[derive(Clone, Debug)]
pub struct PackingResult {
    pub containers: Vec<Container>,
    pub unplaced: Vec<UnplacedBox>,
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
    /// Ein Objekt konnte nicht platziert werden.
    ObjectRejected {
        id: usize,
        weight: f64,
        dims: (f64, f64, f64),
        reason_code: String,
        reason_text: String,
    },
    /// Packen abgeschlossen.
    Finished { containers: usize, unplaced: usize },
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
        });
        return PackingResult {
            containers: Vec::new(),
            unplaced: Vec::new(),
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
        });
        return PackingResult {
            containers: Vec::new(),
            unplaced,
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

    let mut containers: Vec<Container> = Vec::new();
    let mut unplaced: Vec<UnplacedBox> = Vec::new();

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

    on_event(&PackEvent::Finished {
        containers: containers.len(),
        unplaced: unplaced.len(),
    });
    PackingResult {
        containers,
        unplaced,
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
fn has_sufficient_support(b: &PlacedBox, cont: &Container, config: &PackingConfig) -> bool {
    if b.position.2 <= config.height_epsilon {
        return true;
    }

    let mut support_area = 0.0;
    let (bx, by, bz) = b.position;
    let (bw, bd, _) = b.object.dims;

    for p in &cont.placed {
        let over_x = overlap_1d(bx, bx + bw, p.position.0, p.position.0 + p.object.dims.0);
        let over_y = overlap_1d(by, by + bd, p.position.1, p.position.1 + p.object.dims.1);
        let diff_z = bz - (p.position.2 + p.object.dims.2);

        if diff_z.abs() < config.height_epsilon && over_x > 0.0 && over_y > 0.0 {
            support_area += over_x * over_y;
        }
    }

    let base_area = bw * bd;
    if base_area <= config.general_epsilon {
        return false;
    }

    (support_area / base_area) >= config.support_ratio
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
    let mut total_w = new_box.object.weight;
    let mut x_c = (new_box.position.0 + new_box.object.dims.0 / 2.0) * new_box.object.weight;
    let mut y_c = (new_box.position.1 + new_box.object.dims.1 / 2.0) * new_box.object.weight;

    for p in &cont.placed {
        let w = p.object.weight;
        total_w += w;
        x_c += (p.position.0 + p.object.dims.0 / 2.0) * w;
        y_c += (p.position.1 + p.object.dims.1 / 2.0) * w;
    }

    let cm_x = x_c / total_w;
    let cm_y = y_c / total_w;

    let center_x = cont.dims.0 / 2.0;
    let center_y = cont.dims.1 / 2.0;

    ((cm_x - center_x).powi(2) + (cm_y - center_y).powi(2)).sqrt()
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
}
