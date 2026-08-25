//! Acoustic module — standing-wave levitation field (Phase 3).
//!
//! Contract: `docs/CONTRACT.md` § Acoustic · Decision: ADR-0012.

mod config;
mod initial;
#[cfg(any(test, feature = "reference"))]
mod reference;
mod sim;
mod types;

pub use config::{
    AcousticConfig, Axis, Driving, MAX_GRID_DIM, MIN_GRID_DIM, MediumSpec, ParticleSpec, Side,
    SolverParams, TransducerSpec, VolumeGrid,
};
pub use sim::AcousticSimulation;
pub use types::AcousticPressureNode;

use thiserror::Error as ThisError;

/// Module-local failures wrapped by [`crate::Error`] (ADR-0005).
#[derive(Debug, ThisError)]
pub enum AcousticError {
    /// A configuration precondition was violated (contract F1).
    /// The message names the exact contract clause and its bounds.
    #[error("invalid acoustic configuration: {0}")]
    InvalidConfig(String),
}
