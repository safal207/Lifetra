# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Bead-thread trajectory model for representing multi-scale temporal state as beads on a persistent orientation thread.
- Sector-local directed graphs inside each bead for spatial, causal, visual, evidence, interaction, or other domain decompositions.
- Explicit bead-to-bead transition semantics for observation, refinement, divergence, recovery, branch, merge, and custom transitions.
- Causal transition fields for cause, effect, spatial scope, confidence, and evidence continuity.
- `examples/dream_loop_beads.rs` showing a visual-refinement loop as a Lifetra trajectory.
- `docs/bead-thread-trajectory.md` describing the metaphor, the graph model, and the gaps it fills in linear refinement loops.

## [0.1.0] - 2026-03-22

### Added

- GitHub Actions CI for `cargo check`, `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`.
- First-pass domain helper methods across causality, orientation, trajectory, reflection, resonance, and synergy.
- Aggregate analysis helpers on `EntityState`: `summary()`, `health_score()`, and `is_coherent()`.
- `examples/idea_evolution.rs` as the first end-to-end walkthrough for a modeled idea.

### Documentation

- Clarified that `CausalState::total_influence()` is cumulative and may exceed `1.0`.
- Documented the current v0.1 coherence semantics used by `EntityState::is_coherent()`.
