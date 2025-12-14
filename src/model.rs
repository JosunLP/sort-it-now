//! Data models for the box packing simulation.
//!
//! This module defines the fundamental data structures for 3D packing optimization:
//! - `Box3D`: Represents an object to be packed with dimensions and weight
//! - `PlacedBox`: An object with its position in the container
//! - `Container`: The packing container with capacity limits
//!
//! All structures implement the traits from the `types` module for OOP compliance.

use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use serde_json::json;
use utoipa::ToSchema;

use crate::types::{BoundingBox, Dimensional, EPSILON_GENERAL, Positioned, Vec3, Weighted};

/// Validation error for object data.
#[derive(Debug, Clone)]
pub enum ValidationError {
    InvalidDimension(String),
    InvalidWeight(String),
    #[allow(dead_code)]
    InvalidConfiguration(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidDimension(msg) => write!(f, "Invalid dimension: {}", msg),
            ValidationError::InvalidWeight(msg) => write!(f, "Invalid weight: {}", msg),
            ValidationError::InvalidConfiguration(msg) => {
                write!(f, "Invalid configuration: {}", msg)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Helper function to validate a single dimension (DRY principle).
fn validate_dimension(value: f64, name: &str) -> Result<(), ValidationError> {
    if value <= 0.0 || value.is_nan() || value.is_infinite() {
        return Err(ValidationError::InvalidDimension(format!(
            "{} must be positive, got: {}",
            name, value
        )));
    }
    Ok(())
}

/// Helper function to validate weight (DRY principle).
fn validate_weight_value(value: f64) -> Result<(), ValidationError> {
    if value <= 0.0 || value.is_nan() || value.is_infinite() {
        return Err(ValidationError::InvalidWeight(format!(
            "Weight must be positive, got: {}",
            value
        )));
    }
    Ok(())
}

/// Validates dimensions and weight together (DRY principle).
fn validate_box_params(dims: (f64, f64, f64), weight: f64) -> Result<(), ValidationError> {
    validate_dimension(dims.0, "Width")?;
    validate_dimension(dims.1, "Depth")?;
    validate_dimension(dims.2, "Height")?;
    validate_weight_value(weight)?;
    Ok(())
}

/// Validates container dimensions (DRY principle).
fn validate_container_dims(dims: (f64, f64, f64)) -> Result<(), ValidationError> {
    validate_dimension(dims.0, "Container width")?;
    validate_dimension(dims.1, "Container depth")?;
    validate_dimension(dims.2, "Container height")?;
    Ok(())
}

/// Represents a 3D object to be packed.
///
/// Implements the `Dimensional` and `Weighted` traits for OOP compliance.
///
/// # Fields
/// * `id` - Unique identification number of the object
/// * `dims` - Dimensions (width, depth, height) in units
/// * `weight` - Weight of the object in kg
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Box3D {
    pub id: usize,
    #[schema(value_type = [f64; 3], example = json!([30.0, 40.0, 20.0]))]
    pub dims: (f64, f64, f64),
    pub weight: f64,
}

impl Box3D {
    /// Creates a new Box3D object with validation.
    ///
    /// # Parameters
    /// * `id` - Unique ID
    /// * `dims` - Dimensions (width, depth, height)
    /// * `weight` - Weight in kg
    ///
    /// # Returns
    /// `Ok(Box3D)` for valid values, otherwise `Err(ValidationError)`
    ///
    /// # Examples
    /// ```
    /// use sort_it_now::model::Box3D;
    ///
    /// let box_ok = Box3D::new(1, (10.0, 20.0, 30.0), 5.0);
    /// assert!(box_ok.is_ok());
    ///
    /// let box_invalid = Box3D::new(1, (-10.0, 20.0, 30.0), 5.0);
    /// assert!(box_invalid.is_err());
    /// ```
    pub fn new(id: usize, dims: (f64, f64, f64), weight: f64) -> Result<Self, ValidationError> {
        validate_box_params(dims, weight)?;
        Ok(Self { id, dims, weight })
    }

    /// Calculates the volume of the object.
    ///
    /// # Returns
    /// The volume as the product of width × depth × height
    pub fn volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }

    /// Returns the base area of the object.
    ///
    /// # Returns
    /// The base area as the product of width × depth
    #[allow(dead_code)]
    pub fn base_area(&self) -> f64 {
        let (w, d, _) = self.dims;
        w * d
    }

    /// Converts the dimensions to a Vec3.
    #[inline]
    pub fn dims_as_vec3(&self) -> Vec3 {
        Vec3::from_tuple(self.dims)
    }
}

/// Implementation of the Dimensional trait for Box3D.
impl Dimensional for Box3D {
    fn dimensions(&self) -> Vec3 {
        self.dims_as_vec3()
    }
}

/// Implementation of the Weighted trait for Box3D.
impl Weighted for Box3D {
    fn weight(&self) -> f64 {
        self.weight
    }
}

/// A placed object with its position in the container.
///
/// Implements `Positioned`, `Dimensional` and `Weighted` traits for OOP compliance.
///
/// # Fields
/// * `object` - The original Box3D object
/// * `position` - Position (x, y, z) of the lower left corner in the container
#[derive(Clone, Debug)]
pub struct PlacedBox {
    pub object: Box3D,
    pub position: (f64, f64, f64),
}

impl PlacedBox {
    /// Creates a new PlacedBox object.
    ///
    /// # Parameters
    /// * `object` - The Box3D object to place
    /// * `position` - Position (x, y, z) in the container
    #[allow(dead_code)]
    pub fn new(object: Box3D, position: (f64, f64, f64)) -> Self {
        Self { object, position }
    }

    /// Returns the top Z coordinate of the placed object.
    ///
    /// # Returns
    /// Z position + height of the object
    #[allow(dead_code)]
    pub fn top_z(&self) -> f64 {
        self.position.2 + self.object.dims.2
    }

    /// Returns the center of mass of the placed object.
    ///
    /// # Returns
    /// Tuple with (center_x, center_y, center_z)
    #[allow(dead_code)]
    pub fn center(&self) -> (f64, f64, f64) {
        (
            self.position.0 + self.object.dims.0 / 2.0,
            self.position.1 + self.object.dims.1 / 2.0,
            self.position.2 + self.object.dims.2 / 2.0,
        )
    }

    /// Returns the center of mass as Vec3.
    #[inline]
    #[allow(dead_code)]
    pub fn center_vec3(&self) -> Vec3 {
        Vec3::new(
            self.position.0 + self.object.dims.0 / 2.0,
            self.position.1 + self.object.dims.1 / 2.0,
            self.position.2 + self.object.dims.2 / 2.0,
        )
    }

    /// Calculates the bounding box of the placed object.
    ///
    /// Useful for collision detection and overlap calculation.
    #[inline]
    #[allow(dead_code)]
    pub fn bounding_box(&self) -> BoundingBox {
        BoundingBox::from_position_and_dims(
            Vec3::from_tuple(self.position),
            self.object.dims_as_vec3(),
        )
    }

    /// Converts the position to a Vec3.
    #[inline]
    pub fn position_vec3(&self) -> Vec3 {
        Vec3::from_tuple(self.position)
    }
}

/// Implementation of the Positioned trait for PlacedBox.
impl Positioned for PlacedBox {
    fn position(&self) -> Vec3 {
        self.position_vec3()
    }
}

/// Implementation of the Dimensional trait for PlacedBox.
impl Dimensional for PlacedBox {
    fn dimensions(&self) -> Vec3 {
        self.object.dims_as_vec3()
    }
}

/// Implementation of the Weighted trait for PlacedBox.
impl Weighted for PlacedBox {
    fn weight(&self) -> f64 {
        self.object.weight
    }
}

/// Represents a packing container with capacity limits.
///
/// # Fields
/// * `dims` - Dimensions (width, depth, height) of the container
/// * `max_weight` - Maximum total weight in kg
/// * `placed` - List of already placed objects
#[derive(Clone, Debug)]
pub struct Container {
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
    pub placed: Vec<PlacedBox>,
    pub template_id: Option<usize>,
    pub label: Option<String>,
}

impl Container {
    /// Creates a new empty container with validation.
    ///
    /// Uses the shared validation logic (DRY principle).
    ///
    /// # Parameters
    /// * `dims` - Dimensions (width, depth, height)
    /// * `max_weight` - Maximum weight
    ///
    /// # Returns
    /// `Ok(Container)` for valid values, otherwise `Err(ValidationError)`
    #[allow(dead_code)]
    pub fn new(dims: (f64, f64, f64), max_weight: f64) -> Result<Self, ValidationError> {
        // Use shared validation logic (DRY)
        validate_container_dims(dims)?;
        validate_weight_value(max_weight)?;

        Ok(Self {
            dims,
            max_weight,
            placed: Vec::new(),
            template_id: None,
            label: None,
        })
    }

    /// Calculates the total weight of all placed objects.
    ///
    /// # Returns
    /// Sum of the weights of all objects
    pub fn total_weight(&self) -> f64 {
        self.placed.iter().map(|b| b.object.weight).sum()
    }

    /// Calculates the remaining available weight.
    ///
    /// # Returns
    /// Difference between maximum and current weight
    pub fn remaining_weight(&self) -> f64 {
        self.max_weight - self.total_weight()
    }

    /// Calculates the used volume in the container.
    ///
    /// # Returns
    /// Sum of the volumes of all placed objects
    #[allow(dead_code)]
    pub fn used_volume(&self) -> f64 {
        self.placed.iter().map(|b| b.object.volume()).sum()
    }

    /// Calculates the total volume of the container.
    ///
    /// # Returns
    /// Volume of the container
    #[allow(dead_code)]
    pub fn total_volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }

    /// Calculates the utilization of the container in percent.
    ///
    /// # Returns
    /// Percentage value of volume usage (0.0 to 100.0)
    #[allow(dead_code)]
    pub fn utilization_percent(&self) -> f64 {
        let total = self.total_volume();
        if total <= 0.0 {
            return 0.0;
        }
        (self.used_volume() / total) * 100.0
    }

    /// Checks if an object can basically fit in the container.
    ///
    /// Considers weight and dimensions with tolerance.
    /// Uses the global tolerance constant (DRY principle).
    ///
    /// # Parameters
    /// * `b` - The object to check
    ///
    /// # Returns
    /// `true` if the object theoretically fits, otherwise `false`
    pub fn can_fit(&self, b: &Box3D) -> bool {
        self.remaining_weight() + EPSILON_GENERAL >= b.weight
            && b.dims.0 <= self.dims.0 + EPSILON_GENERAL
            && b.dims.1 <= self.dims.1 + EPSILON_GENERAL
            && b.dims.2 <= self.dims.2 + EPSILON_GENERAL
    }

    /// Converts the container dimensions to a Vec3.
    #[inline]
    #[allow(dead_code)]
    pub fn dims_as_vec3(&self) -> Vec3 {
        Vec3::from_tuple(self.dims)
    }

    /// Calculates the geometric center of the container (XY plane).
    #[inline]
    #[allow(dead_code)]
    pub fn center_xy(&self) -> (f64, f64) {
        (self.dims.0 / 2.0, self.dims.1 / 2.0)
    }

    /// Creates a new empty container with the same properties.
    ///
    /// # Returns
    /// A new container with the same dimensions and weight limit
    #[allow(dead_code)]
    pub fn empty_like(&self) -> Self {
        Self {
            dims: self.dims,
            max_weight: self.max_weight,
            placed: Vec::new(),
            template_id: self.template_id,
            label: self.label.clone(),
        }
    }

    /// Stores metadata for the container type (Builder pattern light).
    #[allow(dead_code)]
    pub fn with_meta(mut self, template_id: usize, label: Option<String>) -> Self {
        self.template_id = Some(template_id);
        self.label = label;
        self
    }
}

/// Template for a container type.
#[derive(Clone, Debug)]
pub struct ContainerBlueprint {
    pub id: usize,
    pub label: Option<String>,
    pub dims: (f64, f64, f64),
    pub max_weight: f64,
}

impl ContainerBlueprint {
    /// Creates a new container template after validating the parameters.
    ///
    /// Uses the same validation logic as Container (DRY principle).
    pub fn new(
        id: usize,
        label: Option<String>,
        dims: (f64, f64, f64),
        max_weight: f64,
    ) -> Result<Self, ValidationError> {
        // Validation via shared functions (DRY)
        validate_container_dims(dims)?;
        validate_weight_value(max_weight)?;
        Ok(Self {
            id,
            label,
            dims,
            max_weight,
        })
    }

    /// Instantiates an empty container based on this template.
    pub fn instantiate(&self) -> Container {
        Container {
            dims: self.dims,
            max_weight: self.max_weight,
            placed: Vec::new(),
            template_id: Some(self.id),
            label: self.label.clone(),
        }
    }

    /// Checks if the object can basically fit based on dimensions and weight.
    ///
    /// Uses the global tolerance constant (DRY principle).
    pub fn can_fit(&self, object: &Box3D) -> bool {
        object.weight <= self.max_weight + EPSILON_GENERAL
            && object.dims.0 <= self.dims.0 + EPSILON_GENERAL
            && object.dims.1 <= self.dims.1 + EPSILON_GENERAL
            && object.dims.2 <= self.dims.2 + EPSILON_GENERAL
    }

    /// Returns the volume of the template.
    pub fn volume(&self) -> f64 {
        let (w, d, h) = self.dims;
        w * d * h
    }

    /// Converts the blueprint dimensions to a Vec3.
    #[inline]
    #[allow(dead_code)]
    pub fn dims_as_vec3(&self) -> Vec3 {
        Vec3::from_tuple(self.dims)
    }
}

/// Implementation of the Dimensional trait for Container.
impl Dimensional for Container {
    fn dimensions(&self) -> Vec3 {
        Vec3::from_tuple(self.dims)
    }
}

/// Implementation of the Dimensional trait for ContainerBlueprint.
impl Dimensional for ContainerBlueprint {
    fn dimensions(&self) -> Vec3 {
        Vec3::from_tuple(self.dims)
    }
}
