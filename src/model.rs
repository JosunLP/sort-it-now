//! Datenmodelle für die Box-Packing-Simulation.
//!
//! Dieses Modul definiert die grundlegenden Datenstrukturen für die 3D-Verpackungsoptimierung:
//! - `Box3D`: Repräsentiert ein zu verpackendes Objekt mit Abmessungen und Gewicht
//! - `PlacedBox`: Ein Objekt mit seiner Position im Container
//! - `Container`: Der Verpackungsbehälter mit Kapazitätsgrenzen

use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use serde_json::json;
use utoipa::ToSchema;

/// Validierungsfehler für Objektdaten.
#[derive(Debug, Clone)]
pub enum ValidationError {
    InvalidDimension(String),
    InvalidWeight(String),
    InvalidConfiguration(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidDimension(msg) => write!(f, "Ungültige Dimension: {}", msg),
            ValidationError::InvalidWeight(msg) => write!(f, "Ungültiges Gewicht: {}", msg),
            ValidationError::InvalidConfiguration(msg) => {
                write!(f, "Ungültige Konfiguration: {}", msg)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Repräsentiert ein 3D-Objekt, das verpackt werden soll.
///
/// # Felder
/// * `id` - Eindeutige Identifikationsnummer des Objekts
/// * `dims` - Dimensionen (Breite, Tiefe, Höhe) in Einheiten
/// * `weight` - Gewicht des Objekts in kg
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Box3D {
    pub id: usize,
    #[schema(value_type = [f64; 3], example = json!([30.0, 40.0, 20.0]))]
    pub dims: (f64, f64, f64),
    pub weight: f64,
}

impl Box3D {
    /// Erstellt ein neues Box3D-Objekt mit Validierung.
    ///
    /// # Parameter
    /// * `id` - Eindeutige ID
    /// * `dims` - Dimensionen (Breite, Tiefe, Höhe)
    /// * `weight` - Gewicht in kg
    ///
    /// # Rückgabewert
    /// `Ok(Box3D)` bei gültigen Werten, sonst `Err(ValidationError)`
    pub fn new(id: usize, dims: (f64, f64, f64), weight: f64) -> Result<Self, ValidationError> {
        let (w, d, h) = dims;

        if w <= 0.0 || w.is_nan() || w.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Breite muss positiv sein, erhalten: {}",
                w
            )));
        }
        if d <= 0.0 || d.is_nan() || d.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Tiefe muss positiv sein, erhalten: {}",
                d
            )));
        }
        if h <= 0.0 || h.is_nan() || h.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Höhe muss positiv sein, erhalten: {}",
                h
            )));
        }
        if weight <= 0.0 || weight.is_nan() || weight.is_infinite() {
            return Err(ValidationError::InvalidWeight(format!(
                "Gewicht muss positiv sein, erhalten: {}",
                weight
            )));
        }

        Ok(Self { id, dims, weight })
    }

    /// Berechnet das Volumen des Objekts.
    ///
    /// # Rückgabewert
    /// Das Volumen als Produkt von Breite × Tiefe × Höhe
    pub fn volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }

    /// Gibt die Grundfläche des Objekts zurück.
    ///
    /// # Rückgabewert
    /// Die Grundfläche als Produkt von Breite × Tiefe
    pub fn base_area(&self) -> f64 {
        let (w, d, _) = self.dims;
        w * d
    }
}

/// Ein platziertes Objekt mit seiner Position im Container.
///
/// # Felder
/// * `object` - Das ursprüngliche Box3D-Objekt
/// * `position` - Position (x, y, z) der unteren linken Ecke im Container
#[derive(Clone, Debug)]
pub struct PlacedBox {
    pub object: Box3D,
    pub position: (f64, f64, f64),
}

impl PlacedBox {
    /// Gibt die obere Z-Koordinate des platzierten Objekts zurück.
    ///
    /// # Rückgabewert
    /// Z-Position + Höhe des Objekts
    pub fn top_z(&self) -> f64 {
        self.position.2 + self.object.dims.2
    }

    /// Gibt den Schwerpunkt des platzierten Objekts zurück.
    ///
    /// # Rückgabewert
    /// Tuple mit (center_x, center_y, center_z)
    pub fn center(&self) -> (f64, f64, f64) {
        (
            self.position.0 + self.object.dims.0 / 2.0,
            self.position.1 + self.object.dims.1 / 2.0,
            self.position.2 + self.object.dims.2 / 2.0,
        )
    }
}

/// Repräsentiert einen Verpackungsbehälter mit Kapazitätsgrenzen.
///
/// # Felder
/// * `dims` - Dimensionen (Breite, Tiefe, Höhe) des Containers
/// * `max_weight` - Maximales Gesamtgewicht in kg
/// * `placed` - Liste der bereits platzierten Objekte
#[derive(Clone, Debug)]
pub struct Container {
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
    pub placed: Vec<PlacedBox>,
    pub template_id: Option<usize>,
    pub label: Option<String>,
}

impl Container {
    /// Erstellt einen neuen leeren Container mit Validierung.
    ///
    /// # Parameter
    /// * `dims` - Dimensionen (Breite, Tiefe, Höhe)
    /// * `max_weight` - Maximales Gewicht
    ///
    /// # Rückgabewert
    /// `Ok(Container)` bei gültigen Werten, sonst `Err(ValidationError)`
    pub fn new(dims: (f64, f64, f64), max_weight: f64) -> Result<Self, ValidationError> {
        let (w, d, h) = dims;

        if w <= 0.0 || w.is_nan() || w.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Container-Breite muss positiv sein, erhalten: {}",
                w
            )));
        }
        if d <= 0.0 || d.is_nan() || d.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Container-Tiefe muss positiv sein, erhalten: {}",
                d
            )));
        }
        if h <= 0.0 || h.is_nan() || h.is_infinite() {
            return Err(ValidationError::InvalidDimension(format!(
                "Container-Höhe muss positiv sein, erhalten: {}",
                h
            )));
        }
        if max_weight <= 0.0 || max_weight.is_nan() || max_weight.is_infinite() {
            return Err(ValidationError::InvalidWeight(format!(
                "Maximales Gewicht muss positiv sein, erhalten: {}",
                max_weight
            )));
        }

        Ok(Self {
            dims,
            max_weight,
            placed: Vec::new(),
            template_id: None,
            label: None,
        })
    }

    /// Berechnet das Gesamtgewicht aller platzierten Objekte.
    ///
    /// # Rückgabewert
    /// Summe der Gewichte aller Objekte
    pub fn total_weight(&self) -> f64 {
        self.placed.iter().map(|b| b.object.weight).sum()
    }

    /// Berechnet das verbleibende verfügbare Gewicht.
    ///
    /// # Rückgabewert
    /// Differenz zwischen maximalem und aktuellem Gewicht
    pub fn remaining_weight(&self) -> f64 {
        self.max_weight - self.total_weight()
    }

    /// Berechnet das genutzte Volumen im Container.
    ///
    /// # Rückgabewert
    /// Summe der Volumina aller platzierten Objekte
    pub fn used_volume(&self) -> f64 {
        self.placed.iter().map(|b| b.object.volume()).sum()
    }

    /// Berechnet das Gesamtvolumen des Containers.
    ///
    /// # Rückgabewert
    /// Volumen des Containers
    pub fn total_volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }

    /// Berechnet die Auslastung des Containers in Prozent.
    ///
    /// # Rückgabewert
    /// Prozentwert der Volumenbelegung (0.0 bis 100.0)
    pub fn utilization_percent(&self) -> f64 {
        let total = self.total_volume();
        if total <= 0.0 {
            return 0.0;
        }
        (self.used_volume() / total) * 100.0
    }

    /// Prüft, ob ein Objekt grundsätzlich in den Container passt.
    ///
    /// Berücksichtigt Gewicht und Dimensionen mit Toleranz.
    ///
    /// # Parameter
    /// * `b` - Das zu prüfende Objekt
    ///
    /// # Rückgabewert
    /// `true` wenn das Objekt theoretisch passt, sonst `false`
    pub fn can_fit(&self, b: &Box3D) -> bool {
        let tolerance = 1e-6;
        self.remaining_weight() + tolerance >= b.weight
            && b.dims.0 <= self.dims.0 + tolerance
            && b.dims.1 <= self.dims.1 + tolerance
            && b.dims.2 <= self.dims.2 + tolerance
    }

    /// Erstellt einen neuen leeren Container mit gleichen Eigenschaften.
    ///
    /// # Rückgabewert
    /// Ein neuer Container mit gleichen Dimensionen und Gewichtslimit
    pub fn empty_like(&self) -> Self {
        Self {
            dims: self.dims,
            max_weight: self.max_weight,
            placed: Vec::new(),
            template_id: self.template_id,
            label: self.label.clone(),
        }
    }

    /// Hinterlegt Metadaten zum Container-Typ (Builder-Pattern light).
    pub fn with_meta(mut self, template_id: usize, label: Option<String>) -> Self {
        self.template_id = Some(template_id);
        self.label = label;
        self
    }
}

/// Vorlage für einen Container-Typ.
#[derive(Clone, Debug)]
pub struct ContainerBlueprint {
    pub id: usize,
    pub label: Option<String>,
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
}

impl ContainerBlueprint {
    /// Erstellt eine neue Container-Vorlage nach Validierung der Parameter.
    pub fn new(
        id: usize,
        label: Option<String>,
        dims: (f64, f64, f64),
        max_weight: f64,
    ) -> Result<Self, ValidationError> {
        // Validierung wird über Container::new sichergestellt.
        let _ = Container::new(dims, max_weight)?;
        Ok(Self {
            id,
            label,
            dims,
            max_weight,
        })
    }

    /// Instanziiert einen leeren Container basierend auf dieser Vorlage.
    pub fn instantiate(&self) -> Container {
        Container {
            dims: self.dims,
            max_weight: self.max_weight,
            placed: Vec::new(),
            template_id: Some(self.id),
            label: self.label.clone(),
        }
    }

    /// Prüft, ob das Objekt aufgrund von Dimensionen und Gewicht grundsätzlich passt.
    pub fn can_fit(&self, object: &Box3D) -> bool {
        let tolerance = 1e-6;
        object.weight <= self.max_weight + tolerance
            && object.dims.0 <= self.dims.0 + tolerance
            && object.dims.1 <= self.dims.1 + tolerance
            && object.dims.2 <= self.dims.2 + tolerance
    }

    /// Liefert das Volumen der Vorlage.
    pub fn volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }
}
