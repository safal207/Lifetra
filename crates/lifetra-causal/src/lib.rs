use lifetra_core::Scalar;

/// A directed causal influence that contributes to an entity's present state.
#[derive(Debug, Clone, PartialEq)]
pub struct CausalLink {
    pub source: String,
    pub influence: Scalar,
}

impl CausalLink {
    /// Creates a causal link from a named source and an influence strength.
    pub fn new(source: impl Into<String>, influence: Scalar) -> Self {
        debug_assert!((0.0..=1.0).contains(&influence));

        Self {
            source: source.into(),
            influence,
        }
    }
}

/// Aggregated causal context for an entity.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CausalState {
    pub links: Vec<CausalLink>,
    pub influence_balance: Scalar,
}

impl CausalState {
    /// Creates causal state from a set of links and a coarse balance score.
    pub fn new(links: Vec<CausalLink>, influence_balance: Scalar) -> Self {
        debug_assert!((0.0..=1.0).contains(&influence_balance));

        Self {
            links,
            influence_balance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_causal_links_and_balance() {
        let state = CausalState::new(vec![CausalLink::new("origin", 0.9)], 0.75);

        assert_eq!(state.links[0].source, "origin");
        assert_eq!(state.influence_balance, 0.75);
    }
}
