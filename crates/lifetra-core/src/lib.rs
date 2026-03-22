/// Shared scalar type for normalized domain measurements.
pub type Scalar = f32;

/// Stable identifier for a modeled entity or idea.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

impl EntityId {
    /// Creates a new entity identifier from any string-like value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Timestamp represented as seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates a timestamp from epoch seconds.
    pub fn new(epoch_seconds: i64) -> Self {
        Self(epoch_seconds)
    }

    /// Returns the timestamp as epoch seconds.
    pub fn epoch_seconds(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_exposes_the_original_value() {
        let id = EntityId::new("entity-1");

        assert_eq!(id.as_str(), "entity-1");
    }

    #[test]
    fn timestamp_exposes_epoch_seconds() {
        let timestamp = Timestamp::new(123);

        assert_eq!(timestamp.epoch_seconds(), 123);
    }
}
