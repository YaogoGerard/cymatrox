//! Fluid module — liquid surface under vertical vibration (Phase 2).
//!
//! Contract: `docs/CONTRACT.md` § Fluid · Decision: ADR-0011.

mod config;
mod initial;
#[cfg(any(test, feature = "reference"))]
mod reference;
mod sim;
mod types;

pub use config::{
    DomainMask, DomainShape, Driving, FluidConfig, LiquidSpec, MAX_GRID_DIM, MIN_GRID_DIM,
    SolverParams, SurfaceGrid,
};
pub use sim::FluidSimulation;
pub use types::FluidSurfaceNode;

use thiserror::Error as ThisError;

/// Module-local failures wrapped by [`crate::Error`] (ADR-0005).
#[derive(Debug, ThisError)]
pub enum FluidError {
    /// A configuration precondition was violated (contract F1).
    /// The message names the exact contract clause and its bounds.
    #[error("invalid fluid configuration: {0}")]
    InvalidConfig(String),
}
