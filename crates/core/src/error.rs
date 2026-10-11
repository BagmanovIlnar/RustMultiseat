//! Error types shared across crates.

use thiserror::Error;

use crate::validation::ValidationReport;

/// Convenient result alias for core operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Errors produced by validation, orchestration, and backends.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Configuration failed validation.
    #[error("configuration validation failed")]
    Validation(ValidationReport),

    /// Backend rejected an operation.
    #[error("backend error: {0}")]
    Backend(String),

    /// Orchestrator is in the wrong state for the requested operation.
    #[error("invalid state transition: {0}")]
    InvalidState(String),

    /// Durable journal is missing or corrupted.
    #[error("journal error: {0}")]
    Journal(String),

    /// Recovery is required before accepting new start requests.
    #[error("recovery required: {0}")]
    RecoveryRequired(String),
}
