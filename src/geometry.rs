//! Geometrische Hilfsfunktionen für 3D-Kollisionserkennung und Raumplanung.
//!
//! Dieses Modul bietet Funktionen zur Überprüfung von Überschneidungen zwischen
//! platzierten Objekten und zur Berechnung von Überlappungen in verschiedenen Dimensionen.

use crate::model::PlacedBox;

/// Prüft, ob zwei platzierte Objekte sich räumlich überschneiden.
///
/// Verwendet Axis-Aligned Bounding Box (AABB) Kollisionserkennung.
/// Zwei Boxen überschneiden sich NICHT, wenn sie in mindestens einer Achse getrennt sind.
///
/// # Parameter
/// * `a` - Erstes platziertes Objekt
/// * `b` - Zweites platziertes Objekt
///
/// # Rückgabewert
/// `true` wenn sich die Objekte überschneiden, sonst `false`
///
/// # Beispiel
/// ```
/// let box1 = PlacedBox { position: (0.0, 0.0, 0.0), ... };
/// let box2 = PlacedBox { position: (5.0, 0.0, 0.0), ... };
/// let collision = intersects(&box1, &box2);
/// ```
pub fn intersects(a: &PlacedBox, b: &PlacedBox) -> bool {
    let (ax, ay, az) = a.position;
    let (aw, ad, ah) = a.object.dims;
    let (bx, by, bz) = b.position;
    let (bw, bd, bh) = b.object.dims;

    // Separating Axis Theorem: Objekte überschneiden sich NICHT, wenn
    // sie in irgendeiner Achse vollständig getrennt sind
    !(ax + aw <= bx
        || bx + bw <= ax
        || ay + ad <= by
        || by + bd <= ay
        || az + ah <= bz
        || bz + bh <= az)
}

/// Berechnet die Überlappung zweier Intervalle in einer Dimension.
///
/// # Parameter
/// * `a1` - Start des ersten Intervalls
/// * `a2` - Ende des ersten Intervalls
/// * `b1` - Start des zweiten Intervalls
/// * `b2` - Ende des zweiten Intervalls
///
/// # Rückgabewert
/// Länge der Überlappung, mindestens 0.0
///
/// # Beispiel
/// ```
/// let overlap = overlap_1d(0.0, 5.0, 3.0, 8.0); // Ergebnis: 2.0
/// ```
pub fn overlap_1d(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    (a2.min(b2) - a1.max(b1)).max(0.0)
}

/// Berechnet die Überlappungsfläche zweier Rechtecke in der XY-Ebene.
///
/// # Parameter
/// * `a` - Erstes platziertes Objekt
/// * `b` - Zweites platziertes Objekt
///
/// # Rückgabewert
/// Fläche der Überlappung in der XY-Ebene
#[allow(dead_code)]
pub fn overlap_area_xy(a: &PlacedBox, b: &PlacedBox) -> f64 {
    let overlap_x = overlap_1d(
        a.position.0,
        a.position.0 + a.object.dims.0,
        b.position.0,
        b.position.0 + b.object.dims.0,
    );
    let overlap_y = overlap_1d(
        a.position.1,
        a.position.1 + a.object.dims.1,
        b.position.1,
        b.position.1 + b.object.dims.1,
    );
    overlap_x * overlap_y
}

/// Prüft, ob ein Punkt innerhalb eines Objekts liegt.
///
/// # Parameter
/// * `point` - Der zu prüfende Punkt (x, y, z)
/// * `placed_box` - Das platzierte Objekt
///
/// # Rückgabewert
/// `true` wenn der Punkt innerhalb des Objekts liegt
pub fn point_inside(point: (f64, f64, f64), placed_box: &PlacedBox) -> bool {
    let (px, py, pz) = point;
    let (bx, by, bz) = placed_box.position;
    let (bw, bd, bh) = placed_box.object.dims;

    px >= bx && px <= bx + bw && py >= by && py <= by + bd && pz >= bz && pz <= bz + bh
}
