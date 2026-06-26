//! Packaging-material (void-fill) estimation for packed containers.
//!
//! Once the optimizer has placed every object, the volume that is *not* occupied by an object
//! remains as empty space inside the rigid container. To stop the load from shifting during
//! transport this void space has to be filled with cushioning material — air pillows, foam flakes,
//! packing paper, and so on. Knowing how much material a shipment needs is a first-class packing
//! result, so this module models it as a small set of reusable, serializable value objects:
//!
//! - [`PackagingFill`] — the void-fill requirement for a single container.
//! - [`PackagingSummary`] — the aggregated requirement across every opened container.
//! - [`PackagingAccumulator`] — folds many [`PackagingFill`]s into a [`PackagingSummary`].
//!
//! All figures are expressed in the same cubic length unit as the container and object dimensions
//! (e.g. cm³ when dimensions are given in centimetres). The types deliberately re-sanitize their
//! inputs so that a `void_volume` is never negative and percentages always fall in `0.0..=100.0`,
//! regardless of how the caller obtained the raw volumes.

use serde::Serialize;
use utoipa::ToSchema;

/// Void-space / packaging-material requirement for a single container.
///
/// The four figures are derived from just two inputs — the container's interior volume and the
/// volume occupied by the packed objects — but exposing them explicitly keeps every consumer
/// (HTTP clients, the CLI, the live visualization) free of duplicated arithmetic (DRY).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, ToSchema)]
pub struct PackagingFill {
    /// Total interior volume of the container (cubic units).
    pub container_volume: f64,
    /// Volume occupied by the packed objects (cubic units).
    pub used_volume: f64,
    /// Empty volume that must be filled with packaging material (cubic units).
    pub void_volume: f64,
    /// Void volume as a percentage of the container volume (`0.0..=100.0`).
    pub void_volume_percent: f64,
}

impl PackagingFill {
    /// Builds a fill report from a container volume and the volume already used by objects.
    ///
    /// Inputs are sanitized defensively: non-finite or negative volumes are treated as `0`, and the
    /// used volume is clamped to never exceed the container volume. This guarantees
    /// `void_volume >= 0` and `void_volume_percent` within `0.0..=100.0` for any caller-supplied
    /// values.
    pub fn from_volumes(container_volume: f64, used_volume: f64) -> Self {
        let container_volume = sanitize_volume(container_volume);
        let used_volume = sanitize_volume(used_volume).min(container_volume);
        let void_volume = (container_volume - used_volume).max(0.0);
        let void_volume_percent = if container_volume > 0.0 {
            (void_volume / container_volume * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        Self {
            container_volume,
            used_volume,
            void_volume,
            void_volume_percent,
        }
    }

    /// Returns the fill report for an empty (zero-volume) container.
    pub const fn empty() -> Self {
        Self {
            container_volume: 0.0,
            used_volume: 0.0,
            void_volume: 0.0,
            void_volume_percent: 0.0,
        }
    }
}

impl Default for PackagingFill {
    fn default() -> Self {
        Self::empty()
    }
}

/// Aggregated packaging-material requirement across every opened container.
///
/// `total_void_volume` is the headline figure: the total amount of cushioning material a shipment
/// needs across all of its containers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, ToSchema)]
pub struct PackagingSummary {
    /// Combined interior volume of all opened containers (cubic units).
    pub total_container_volume: f64,
    /// Combined volume occupied by all packed objects (cubic units).
    pub total_used_volume: f64,
    /// Total void volume that must be filled with packaging material (cubic units).
    pub total_void_volume: f64,
    /// Mean per-container void volume as a percentage of container volume (`0.0..=100.0`).
    pub average_void_volume_percent: f64,
}

impl PackagingSummary {
    /// Returns the summary for a packing run that opened no containers.
    pub const fn empty() -> Self {
        Self {
            total_container_volume: 0.0,
            total_used_volume: 0.0,
            total_void_volume: 0.0,
            average_void_volume_percent: 0.0,
        }
    }
}

impl Default for PackagingSummary {
    fn default() -> Self {
        Self::empty()
    }
}

/// Folds per-container [`PackagingFill`]s into a single [`PackagingSummary`].
///
/// Using a dedicated accumulator keeps the aggregation logic in one place, so the optimizer's
/// summary builder and any future consumers stay DRY.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackagingAccumulator {
    total_container_volume: f64,
    total_used_volume: f64,
    total_void_volume: f64,
    void_percent_sum: f64,
    container_count: usize,
}

impl PackagingAccumulator {
    /// Creates an empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a single container's fill report to the running totals.
    pub fn record(&mut self, fill: &PackagingFill) {
        self.total_container_volume += fill.container_volume;
        self.total_used_volume += fill.used_volume;
        self.total_void_volume += fill.void_volume;
        self.void_percent_sum += fill.void_volume_percent;
        self.container_count += 1;
    }

    /// Consumes the accumulator and produces the aggregated [`PackagingSummary`].
    pub fn finish(self) -> PackagingSummary {
        let average_void_volume_percent = if self.container_count > 0 {
            self.void_percent_sum / self.container_count as f64
        } else {
            0.0
        };

        PackagingSummary {
            total_container_volume: self.total_container_volume,
            total_used_volume: self.total_used_volume,
            total_void_volume: self.total_void_volume,
            average_void_volume_percent,
        }
    }
}

impl<'a> FromIterator<&'a PackagingFill> for PackagingSummary {
    fn from_iter<I: IntoIterator<Item = &'a PackagingFill>>(iter: I) -> Self {
        let mut acc = PackagingAccumulator::new();
        for fill in iter {
            acc.record(fill);
        }
        acc.finish()
    }
}

/// Normalizes a raw volume input to a finite, non-negative value.
fn sanitize_volume(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn fill_computes_void_volume_and_percentage() {
        let fill = PackagingFill::from_volumes(1000.0, 250.0);
        assert!((fill.void_volume - 750.0).abs() < EPS);
        assert!((fill.void_volume_percent - 75.0).abs() < EPS);
        assert!((fill.used_volume - 250.0).abs() < EPS);
        assert!((fill.container_volume - 1000.0).abs() < EPS);
    }

    #[test]
    fn fill_clamps_overfilled_container() {
        // A used volume larger than the container (e.g. from rounding) must not yield a negative
        // void volume.
        let fill = PackagingFill::from_volumes(100.0, 150.0);
        assert!((fill.void_volume - 0.0).abs() < EPS);
        assert!((fill.void_volume_percent - 0.0).abs() < EPS);
        assert!((fill.used_volume - 100.0).abs() < EPS);
    }

    #[test]
    fn fill_handles_non_finite_inputs() {
        let fill = PackagingFill::from_volumes(f64::NAN, f64::INFINITY);
        assert_eq!(fill, PackagingFill::empty());
    }

    #[test]
    fn fill_of_empty_container_is_all_void() {
        let fill = PackagingFill::from_volumes(500.0, 0.0);
        assert!((fill.void_volume - 500.0).abs() < EPS);
        assert!((fill.void_volume_percent - 100.0).abs() < EPS);
    }

    #[test]
    fn accumulator_sums_void_volume_and_averages_percentage() {
        let fills = [
            PackagingFill::from_volumes(1000.0, 250.0), // 750 void, 75%
            PackagingFill::from_volumes(1000.0, 750.0), // 250 void, 25%
        ];

        let summary: PackagingSummary = fills.iter().collect();
        assert!((summary.total_void_volume - 1000.0).abs() < EPS);
        assert!((summary.total_container_volume - 2000.0).abs() < EPS);
        assert!((summary.total_used_volume - 1000.0).abs() < EPS);
        assert!((summary.average_void_volume_percent - 50.0).abs() < EPS);
    }

    #[test]
    fn empty_summary_reports_zeroes() {
        let summary: PackagingSummary = std::iter::empty::<&PackagingFill>().collect();
        assert_eq!(summary, PackagingSummary::empty());
    }
}
