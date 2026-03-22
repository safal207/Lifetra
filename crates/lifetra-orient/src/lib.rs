use lifetra_core::Scalar;

/// Named axis within the orientation vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationAxis {
    Growth,
    Stability,
    Truth,
    Connection,
}

/// Directional tendencies that describe where an entity is trying to move.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OrientationVector {
    pub toward_growth: Scalar,
    pub toward_stability: Scalar,
    pub toward_truth: Scalar,
    pub toward_connection: Scalar,
}

impl OrientationVector {
    /// Creates an orientation vector across the core conceptual directions.
    pub fn new(
        toward_growth: Scalar,
        toward_stability: Scalar,
        toward_truth: Scalar,
        toward_connection: Scalar,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&toward_growth));
        debug_assert!((0.0..=1.0).contains(&toward_stability));
        debug_assert!((0.0..=1.0).contains(&toward_truth));
        debug_assert!((0.0..=1.0).contains(&toward_connection));

        Self {
            toward_growth,
            toward_stability,
            toward_truth,
            toward_connection,
        }
    }

    /// Returns the dominant orientation axis and its score.
    pub fn dominant_axis(&self) -> (OrientationAxis, Scalar) {
        let axes = [
            (OrientationAxis::Growth, self.toward_growth),
            (OrientationAxis::Stability, self.toward_stability),
            (OrientationAxis::Truth, self.toward_truth),
            (OrientationAxis::Connection, self.toward_connection),
        ];

        axes.into_iter()
            .max_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or((OrientationAxis::Growth, 0.0))
    }

    /// Returns the average directional intensity across all four axes.
    pub fn average_intensity(&self) -> Scalar {
        (self.toward_growth + self.toward_stability + self.toward_truth + self.toward_connection)
            / 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_directional_tendencies() {
        let orientation = OrientationVector::new(0.8, 0.6, 0.9, 0.7);

        assert!(orientation.toward_truth > orientation.toward_stability);
    }

    #[test]
    fn detects_dominant_axis() {
        let orientation = OrientationVector::new(0.71, 0.66, 0.91, 0.84);

        assert_eq!(orientation.dominant_axis(), (OrientationAxis::Truth, 0.91));
        assert!((orientation.average_intensity() - 0.78).abs() < 0.000_1);
    }
}
