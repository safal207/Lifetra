use lifetra_core::Scalar;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_directional_tendencies() {
        let orientation = OrientationVector::new(0.8, 0.6, 0.9, 0.7);

        assert!(orientation.toward_truth > orientation.toward_stability);
    }
}
