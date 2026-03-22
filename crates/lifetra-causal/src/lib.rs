#[derive(Debug, Clone, PartialEq)]
pub struct CausalLink {
    pub source: String,
    pub influence: f32,
}

impl CausalLink {
    pub fn new(source: impl Into<String>, influence: f32) -> Self {
        Self {
            source: source.into(),
            influence,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CausalState {
    pub links: Vec<CausalLink>,
    pub influence_balance: f32,
}

impl CausalState {
    pub fn new(links: Vec<CausalLink>, influence_balance: f32) -> Self {
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
