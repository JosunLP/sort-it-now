//! Geometric helper functions for 3D collision detection and spatial planning.
//!
//! This module provides functions for checking intersections between
//! placed objects and for calculating overlaps in various dimensions.
//!
//! ## Design Principles
//!
//! - **Backward compatibility**: All existing functions are preserved
//! - **OOP extensions**: New functions use traits from `types.rs`
//! - **Performance**: Critical paths are inline-optimized

use crate::model::PlacedBox;
use crate::types::{BoundingBox, EPSILON_GENERAL, Vec3};

/// Checks if two placed objects spatially intersect.
///
/// Uses Axis-Aligned Bounding Box (AABB) collision detection.
/// Two boxes do NOT intersect if they are separated in at least one axis.
///
/// # Algorithm
/// Implements the Separating Axis Theorem (SAT) for AABBs:
/// Two convex objects do NOT intersect if and only if there exists an axis
/// on which their projections are separated.
///
/// # Parameters
/// * `a` - First placed object
/// * `b` - Second placed object
///
/// # Returns
/// `true` if the objects intersect, otherwise `false`
///
/// # Complexity
/// O(1) - Constant time
///
/// # Example
/// ```ignore
/// let box1 = PlacedBox::new(box_a, (0.0, 0.0, 0.0));
/// let box2 = PlacedBox::new(box_b, (5.0, 0.0, 0.0));
/// let collision = intersects(&box1, &box2);
/// ```
#[inline]
pub fn intersects(a: &PlacedBox, b: &PlacedBox) -> bool {
    let (ax, ay, az) = a.position;
    let (aw, ad, ah) = a.object.dims;
    let (bx, by, bz) = b.position;
    let (bw, bd, bh) = b.object.dims;

    // Separating Axis Theorem: Objects do NOT intersect if
    // they are completely separated in any axis
    !(ax + aw <= bx
        || bx + bw <= ax
        || ay + ad <= by
        || by + bd <= ay
        || az + ah <= bz
        || bz + bh <= az)
}

/// Alternative collision check with BoundingBox types (OOP version).
///
/// Uses the `BoundingBox` structure from the `types` module for better type safety.
///
/// # Parameters
/// * `a` - First bounding box
/// * `b` - Second bounding box
///
/// # Returns
/// `true` if the boxes intersect
#[inline]
#[allow(dead_code)]
pub fn bounding_boxes_intersect(a: &BoundingBox, b: &BoundingBox) -> bool {
    a.intersects(b)
}

/// Calculates the overlap of two intervals in one dimension.
///
/// # Parameters
/// * `a1` - Start of the first interval
/// * `a2` - End of the first interval
/// * `b1` - Start of the second interval
/// * `b2` - End of the second interval
///
/// # Returns
/// Length of the overlap, at least 0.0
///
/// # Example
/// ```ignore
/// let overlap = overlap_1d(0.0, 5.0, 3.0, 8.0); // Result: 2.0
/// let no_overlap = overlap_1d(0.0, 3.0, 5.0, 8.0); // Result: 0.0
/// ```
#[inline]
pub fn overlap_1d(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    (a2.min(b2) - a1.max(b1)).max(0.0)
}

/// Calculates the overlap area of two rectangles in the XY plane.
///
/// # Parameters
/// * `a` - First placed object
/// * `b` - Second placed object
///
/// # Returns
/// Area of overlap in the XY plane
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

/// Checks if a point is inside an object.
///
/// # Parameters
/// * `point` - The point to check (x, y, z)
/// * `placed_box` - The placed object
///
/// # Returns
/// `true` if the point is inside the object
#[inline]
pub fn point_inside(point: (f64, f64, f64), placed_box: &PlacedBox) -> bool {
    let (px, py, pz) = point;
    let (bx, by, bz) = placed_box.position;
    let (bw, bd, bh) = placed_box.object.dims;

    px >= bx && px <= bx + bw && py >= by && py <= by + bd && pz >= bz && pz <= bz + bh
}

/// Checks if a Vec3 point is inside a BoundingBox (OOP version).
///
/// # Parameters
/// * `point` - The point to check
/// * `bounds` - The bounding box
///
/// # Returns
/// `true` if the point is inside the box
#[inline]
#[allow(dead_code)]
pub fn point_inside_bounds(point: &Vec3, bounds: &BoundingBox) -> bool {
    bounds.contains_point(point)
}

/// Checks if a box rests on another box (e.g., for stability checking).
///
/// A box rests on another if:
/// 1. The bottom of the upper box touches the top of the lower (within tolerance)
/// 2. There is XY overlap
///
/// # Parameters
/// * `upper` - The upper box
/// * `lower` - The lower (supporting) box
/// * `height_epsilon` - Tolerance for height comparisons
///
/// # Returns
/// `true` if `upper` rests on `lower`
#[inline]
#[allow(dead_code)]
pub fn rests_on(upper: &PlacedBox, lower: &PlacedBox, height_epsilon: f64) -> bool {
    let upper_bottom = upper.position.2;
    let lower_top = lower.position.2 + lower.object.dims.2;

    // Check if heights match
    if (upper_bottom - lower_top).abs() > height_epsilon {
        return false;
    }

    // Check XY overlap
    let overlap_x = overlap_1d(
        upper.position.0,
        upper.position.0 + upper.object.dims.0,
        lower.position.0,
        lower.position.0 + lower.object.dims.0,
    );
    let overlap_y = overlap_1d(
        upper.position.1,
        upper.position.1 + upper.object.dims.1,
        lower.position.1,
        lower.position.1 + lower.object.dims.1,
    );

    overlap_x > EPSILON_GENERAL && overlap_y > EPSILON_GENERAL
}

/// Calculates the support area between two boxes.
///
/// Returns the area with which `upper` rests on `lower`.
/// Returns 0.0 if the boxes are not in contact.
///
/// # Parameters
/// * `upper` - The upper box
/// * `lower` - The lower box
/// * `height_epsilon` - Tolerance for height comparisons
///
/// # Returns
/// Overlap area in the XY plane
#[inline]
#[allow(dead_code)]
pub fn support_area(upper: &PlacedBox, lower: &PlacedBox, height_epsilon: f64) -> f64 {
    let upper_bottom = upper.position.2;
    let lower_top = lower.position.2 + lower.object.dims.2;

    // Check if heights match
    if (upper_bottom - lower_top).abs() > height_epsilon {
        return 0.0;
    }

    overlap_area_xy(upper, lower)
}

/// Calculates the center of mass of a set of points in the XY plane.
///
/// Useful for stability calculations and balance checks.
///
/// # Parameters
/// * `points` - Iterator over (x, y, weight) tuples
///
/// # Returns
/// `Some((cx, cy))` if total weight > 0, otherwise `None`
#[allow(dead_code)]
pub fn center_of_mass_xy<I>(points: I) -> Option<(f64, f64)>
where
    I: IntoIterator<Item = (f64, f64, f64)>,
{
    let mut total_weight = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for (x, y, weight) in points {
        total_weight += weight;
        weighted_x += x * weight;
        weighted_y += y * weight;
    }

    if total_weight <= 0.0 {
        None
    } else {
        Some((weighted_x / total_weight, weighted_y / total_weight))
    }
}

/// Calculates the Euclidean 2D distance between two points.
///
/// # Parameters
/// * `a` - First point (x, y)
/// * `b` - Second point (x, y)
///
/// # Returns
/// Euclidean distance
#[inline]
#[allow(dead_code)]
pub fn distance_2d(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Box3D;

    fn make_placed_box(id: usize, pos: (f64, f64, f64), dims: (f64, f64, f64)) -> PlacedBox {
        PlacedBox {
            object: Box3D {
                id,
                dims,
                weight: 1.0,
            },
            position: pos,
        }
    }

    #[test]
    fn test_intersects_overlapping_boxes() {
        let a = make_placed_box(1, (0.0, 0.0, 0.0), (10.0, 10.0, 10.0));
        let b = make_placed_box(2, (5.0, 5.0, 5.0), (10.0, 10.0, 10.0));
        assert!(intersects(&a, &b));
    }

    #[test]
    fn test_intersects_separated_boxes() {
        let a = make_placed_box(1, (0.0, 0.0, 0.0), (10.0, 10.0, 10.0));
        let b = make_placed_box(2, (20.0, 0.0, 0.0), (10.0, 10.0, 10.0));
        assert!(!intersects(&a, &b));
    }

    #[test]
    fn test_overlap_1d() {
        assert!((overlap_1d(0.0, 5.0, 3.0, 8.0) - 2.0).abs() < EPSILON_GENERAL);
        assert!((overlap_1d(0.0, 3.0, 5.0, 8.0) - 0.0).abs() < EPSILON_GENERAL);
        assert!((overlap_1d(0.0, 10.0, 2.0, 8.0) - 6.0).abs() < EPSILON_GENERAL);
    }

    #[test]
    fn test_point_inside() {
        let box_ = make_placed_box(1, (0.0, 0.0, 0.0), (10.0, 10.0, 10.0));
        assert!(point_inside((5.0, 5.0, 5.0), &box_));
        assert!(!point_inside((15.0, 5.0, 5.0), &box_));
    }

    #[test]
    fn test_rests_on() {
        let lower = make_placed_box(1, (0.0, 0.0, 0.0), (10.0, 10.0, 10.0));
        let upper = make_placed_box(2, (0.0, 0.0, 10.0), (10.0, 10.0, 10.0));
        let separate = make_placed_box(3, (20.0, 0.0, 10.0), (10.0, 10.0, 10.0));

        assert!(rests_on(&upper, &lower, 1e-3));
        assert!(!rests_on(&separate, &lower, 1e-3));
    }

    #[test]
    fn test_center_of_mass_xy() {
        let points = vec![(0.0, 0.0, 10.0), (10.0, 0.0, 10.0)];
        let center = center_of_mass_xy(points).unwrap();
        assert!((center.0 - 5.0).abs() < EPSILON_GENERAL);
        assert!((center.1 - 0.0).abs() < EPSILON_GENERAL);
    }

    #[test]
    fn test_distance_2d() {
        assert!((distance_2d((0.0, 0.0), (3.0, 4.0)) - 5.0).abs() < EPSILON_GENERAL);
    }
}
