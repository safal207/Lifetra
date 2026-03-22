/// Directional tendencies that describe where an entity is trying to move.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OrientationVector {
    pub toward_growth: f32,
    pub toward_stability: f32,
    pub toward_truth: f32,
    pub toward_connection: f32,
}

impl OrientationVector {
    /// Creates an orientation vector across the core conceptual directions.
    pub fn new(
        toward_growth: f32,
        toward_stability: f32,
        toward_truth: f32,
        toward_connection: f32,
    ) -> Self {
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
