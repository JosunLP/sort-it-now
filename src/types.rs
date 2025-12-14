//! Common types and traits for 3D geometry.
//!
//! This module defines reusable types and trait abstractions
//! that promote DRY principles and OOP design patterns.

use std::ops::{Add, Mul, Sub};

/// Global numerical tolerance for floating-point comparisons.
///
/// Used for general numerical operations such as dimension and weight comparisons.
pub const EPSILON_GENERAL: f64 = 1e-6;

/// Tolerance for height comparisons in the Z-plane.
///
/// Slightly larger tolerance for height matching during stacking.
pub const EPSILON_HEIGHT: f64 = 1e-3;

/// Represents a 3D vector or point in space.
///
/// Used for positions, dimensions, and calculations in 3D space.
///
/// # Examples
/// ```
/// use sort_it_now::types::Vec3;
///
/// let position = Vec3::new(1.0, 2.0, 3.0);
/// let dimensions = Vec3::new(10.0, 20.0, 30.0);
/// let center = position + dimensions * 0.5;
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    /// Creates a new 3D vector.
    ///
    /// # Parameters
    /// * `x` - X component (width)
    /// * `y` - Y component (depth)
    /// * `z` - Z component (height)
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Creates a zero vector (origin).
    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }

    /// Converts to tuple format for API compatibility.
    #[inline]
    pub const fn as_tuple(&self) -> (f64, f64, f64) {
        (self.x, self.y, self.z)
    }

    /// Creates from tuple format.
    #[inline]
    pub const fn from_tuple(tuple: (f64, f64, f64)) -> Self {
        Self::new(tuple.0, tuple.1, tuple.2)
    }

    /// Calculates the volume (product of all components).
    ///
    /// Useful for dimension vectors.
    #[inline]
    pub fn volume(&self) -> f64 {
        self.x * self.y * self.z
    }

    /// Calculates the base area (X × Y product).
    #[inline]
    pub fn base_area(&self) -> f64 {
        self.x * self.y
    }

    /// Calculates the Euclidean distance to another point.
    #[inline]
    pub fn distance_to(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Calculates the 2D distance (XY plane only).
    #[inline]
    pub fn distance_2d(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Checks if all components are positive and finite.
    #[inline]
    pub fn is_valid_dimension(&self) -> bool {
        self.x > 0.0
            && self.y > 0.0
            && self.z > 0.0
            && self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
    }

    /// Checks if the vector fits within another vector (component-wise <=).
    ///
    /// # Parameters
    /// * `container` - The outer vector (e.g., container dimensions)
    /// * `tolerance` - Numerical tolerance for the comparison
    #[inline]
    pub fn fits_within(&self, container: &Self, tolerance: f64) -> bool {
        self.x <= container.x + tolerance
            && self.y <= container.y + tolerance
            && self.z <= container.z + tolerance
    }

    /// Returns the midpoint between the origin and this point.
    #[inline]
    pub fn center(&self) -> Self {
        Self::new(self.x / 2.0, self.y / 2.0, self.z / 2.0)
    }
}

impl Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;

    #[inline]
    fn mul(self, scalar: f64) -> Self::Output {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

impl From<(f64, f64, f64)> for Vec3 {
    #[inline]
    fn from(tuple: (f64, f64, f64)) -> Self {
        Self::from_tuple(tuple)
    }
}

impl From<Vec3> for (f64, f64, f64) {
    #[inline]
    fn from(vec: Vec3) -> Self {
        vec.as_tuple()
    }
}

/// Trait for objects with 3D dimensions.
///
/// Provides a common interface for all objects with spatial extent.
/// Follows the Interface Segregation Principle (ISP).
pub trait Dimensional {
    /// Returns the dimensions of the object.
    fn dimensions(&self) -> Vec3;

    /// Calculates the volume.
    fn volume(&self) -> f64 {
        self.dimensions().volume()
    }

    /// Calculates the base area.
    fn base_area(&self) -> f64 {
        self.dimensions().base_area()
    }

    /// Checks if this object fits in a container with the given dimensions.
    fn fits_in(&self, container_dims: &Vec3, tolerance: f64) -> bool {
        self.dimensions().fits_within(container_dims, tolerance)
    }
}

/// Trait for objects with a position in 3D space.
///
/// Enables querying position and bounding box calculations.
pub trait Positioned {
    /// Returns the position (lower left front corner).
    fn position(&self) -> Vec3;
}

/// Trait for objects with weight.
///
/// Provides a common interface for weight operations.
pub trait Weighted {
    /// Returns the weight in kg.
    fn weight(&self) -> f64;
}

/// Represents an Axis-Aligned Bounding Box (AABB).
///
/// Used for efficient collision detection and overlap calculation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox {
    /// Minimum corner (position)
    pub min: Vec3,
    /// Maximum corner (position + dimensions)
    pub max: Vec3,
}

impl BoundingBox {
    /// Creates a new bounding box.
    #[inline]
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// Creates a bounding box from position and dimensions.
    #[inline]
    pub fn from_position_and_dims(position: Vec3, dims: Vec3) -> Self {
        Self {
            min: position,
            max: position + dims,
        }
    }

    /// Checks if two bounding boxes intersect.
    ///
    /// Implements the Separating Axis Theorem (SAT) for AABBs.
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        !(self.max.x <= other.min.x
            || other.max.x <= self.min.x
            || self.max.y <= other.min.y
            || other.max.y <= self.min.y
            || self.max.z <= other.min.z
            || other.max.z <= self.min.z)
    }

    /// Calculates the overlap length in one dimension.
    #[inline]
    fn overlap_1d(a_min: f64, a_max: f64, b_min: f64, b_max: f64) -> f64 {
        (a_max.min(b_max) - a_min.max(b_min)).max(0.0)
    }

    /// Calculates the overlap area in the XY plane.
    #[inline]
    pub fn overlap_area_xy(&self, other: &Self) -> f64 {
        let overlap_x = Self::overlap_1d(self.min.x, self.max.x, other.min.x, other.max.x);
        let overlap_y = Self::overlap_1d(self.min.y, self.max.y, other.min.y, other.max.y);
        overlap_x * overlap_y
    }

    /// Checks if a point is inside the bounding box.
    #[inline]
    pub fn contains_point(&self, point: &Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    /// Returns the top (Z maximum).
    #[inline]
    pub fn top_z(&self) -> f64 {
        self.max.z
    }

    /// Returns the center point.
    #[inline]
    pub fn center(&self) -> Vec3 {
        Vec3::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
            (self.min.z + self.max.z) / 2.0,
        )
    }

    /// Returns the dimensions (width, depth, height).
    #[inline]
    pub fn dimensions(&self) -> Vec3 {
        self.max - self.min
    }
}

/// Validation functions for DRY principle.
#[allow(dead_code)]
pub mod validation {

    /// Validates a single dimension.
    ///
    /// # Parameters
    /// * `value` - The value to validate
    /// * `name` - Name of the dimension for error messages
    ///
    /// # Returns
    /// `Ok(())` for valid values, otherwise error text
    pub fn validate_dimension(value: f64, name: &str) -> Result<(), String> {
        if value <= 0.0 {
            return Err(format!("{} must be positive, got: {}", name, value));
        }
        if value.is_nan() {
            return Err(format!("{} must not be NaN", name));
        }
        if value.is_infinite() {
            return Err(format!("{} must not be infinite", name));
        }
        Ok(())
    }

    /// Validates a weight.
    ///
    /// # Parameters
    /// * `value` - The value to validate
    ///
    /// # Returns
    /// `Ok(())` for valid values, otherwise error text
    pub fn validate_weight(value: f64) -> Result<(), String> {
        if value <= 0.0 {
            return Err(format!("Weight must be positive, got: {}", value));
        }
        if value.is_nan() {
            return Err("Weight must not be NaN".to_string());
        }
        if value.is_infinite() {
            return Err("Weight must not be infinite".to_string());
        }
        Ok(())
    }

    /// Validates all three dimensions of a 3D object.
    ///
    /// # Parameters
    /// * `dims` - The dimensions to validate (width, depth, height)
    ///
    /// # Returns
    /// `Ok(())` for valid values, otherwise error text
    pub fn validate_dimensions_3d(dims: (f64, f64, f64)) -> Result<(), String> {
        validate_dimension(dims.0, "Width")?;
        validate_dimension(dims.1, "Depth")?;
        validate_dimension(dims.2, "Height")?;
        Ok(())
    }
}

/// Center of mass calculation helper.
///
/// Accumulates weighted positions for center of mass calculation.
#[derive(Clone, Debug, Default)]
pub struct CenterOfMassCalculator {
    weighted_x: f64,
    weighted_y: f64,
    total_weight: f64,
}

impl CenterOfMassCalculator {
    /// Creates a new calculator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a weighted point.
    ///
    /// # Parameters
    /// * `x` - X position of the point
    /// * `y` - Y position of the point
    /// * `weight` - Weight of the point
    pub fn add_point(&mut self, x: f64, y: f64, weight: f64) {
        self.weighted_x += x * weight;
        self.weighted_y += y * weight;
        self.total_weight += weight;
    }

    /// Calculates the center of mass.
    ///
    /// # Returns
    /// `Some((x, y))` for valid center of mass, `None` if no weight present
    pub fn compute(&self) -> Option<(f64, f64)> {
        if self.total_weight <= 0.0 {
            None
        } else {
            Some((
                self.weighted_x / self.total_weight,
                self.weighted_y / self.total_weight,
            ))
        }
    }

    /// Calculates the distance of the center of mass to a reference point.
    ///
    /// # Parameters
    /// * `reference` - The reference point (e.g., container center)
    pub fn distance_to(&self, reference: (f64, f64)) -> f64 {
        match self.compute() {
            Some((cx, cy)) => {
                let dx = cx - reference.0;
                let dy = cy - reference.1;
                (dx * dx + dy * dy).sqrt()
            }
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec3_operations() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);

        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn test_vec3_volume_and_area() {
        let dims = Vec3::new(10.0, 20.0, 30.0);
        assert!((dims.volume() - 6000.0).abs() < EPSILON_GENERAL);
        assert!((dims.base_area() - 200.0).abs() < EPSILON_GENERAL);
    }

    #[test]
    fn test_vec3_fits_within() {
        let small = Vec3::new(5.0, 5.0, 5.0);
        let large = Vec3::new(10.0, 10.0, 10.0);

        assert!(small.fits_within(&large, EPSILON_GENERAL));
        assert!(!large.fits_within(&small, EPSILON_GENERAL));
    }

    #[test]
    fn test_bounding_box_intersects() {
        let a = BoundingBox::from_position_and_dims(Vec3::zero(), Vec3::new(10.0, 10.0, 10.0));
        let b = BoundingBox::from_position_and_dims(
            Vec3::new(5.0, 5.0, 5.0),
            Vec3::new(10.0, 10.0, 10.0),
        );
        let c = BoundingBox::from_position_and_dims(
            Vec3::new(20.0, 20.0, 20.0),
            Vec3::new(10.0, 10.0, 10.0),
        );

        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn test_bounding_box_overlap_area() {
        let a = BoundingBox::from_position_and_dims(Vec3::zero(), Vec3::new(10.0, 10.0, 10.0));
        let b = BoundingBox::from_position_and_dims(
            Vec3::new(5.0, 5.0, 0.0),
            Vec3::new(10.0, 10.0, 10.0),
        );

        let overlap = a.overlap_area_xy(&b);
        assert!((overlap - 25.0).abs() < EPSILON_GENERAL); // 5x5 overlap
    }

    #[test]
    fn test_center_of_mass_calculator() {
        let mut calc = CenterOfMassCalculator::new();
        calc.add_point(0.0, 0.0, 10.0);
        calc.add_point(10.0, 0.0, 10.0);

        let center = calc.compute().unwrap();
        assert!((center.0 - 5.0).abs() < EPSILON_GENERAL);
        assert!((center.1 - 0.0).abs() < EPSILON_GENERAL);
    }

    #[test]
    fn test_validation_dimension() {
        assert!(validation::validate_dimension(10.0, "Width").is_ok());
        assert!(validation::validate_dimension(0.0, "Width").is_err());
        assert!(validation::validate_dimension(-1.0, "Width").is_err());
        assert!(validation::validate_dimension(f64::NAN, "Width").is_err());
        assert!(validation::validate_dimension(f64::INFINITY, "Width").is_err());
    }

    #[test]
    fn test_validation_weight() {
        assert!(validation::validate_weight(10.0).is_ok());
        assert!(validation::validate_weight(0.0).is_err());
        assert!(validation::validate_weight(-1.0).is_err());
    }
}
