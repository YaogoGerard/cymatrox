//! Granular module — solids on Chladni plates (Phase 1).
//!
//! Contract: `docs/CONTRACT.md` § Granular · Decision: ADR-0009.

mod config;
mod placement;
#[cfg(any(test, feature = "reference"))]
mod reference;
mod sim;
mod types;

pub use config::{
    Driving, EigenPair, GrainBed, GranularConfig, InitialDistribution, ModeSelection, PlateSpec,
    SolverParams,
};
pub use sim::GranularSimulation;
pub use types::GranularData;

use thiserror::Error as ThisError;

/// Module-local failures wrapped by [`crate::Error`] (ADR-0005).
#[derive(Debug, ThisError)]
pub enum GranularError {
    /// A configuration precondition was violated (contract F1).
    /// The message names the exact contract clause and its bounds.
    #[error("invalid granular configuration: {0}")]
    InvalidConfig(String),
}
