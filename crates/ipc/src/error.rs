//! Framing and codec errors.

use thiserror::Error;

/// Result alias for frame operations.
pub type FrameResult<T> = Result<T, FrameError>;

/// Errors while encoding/decoding frames.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// Buffer is shorter than the fixed header.
    #[error("truncated frame header")]
    TruncatedHeader,

    /// Magic bytes do not match.
    #[error("invalid magic")]
    InvalidMagic,

    /// Protocol version is unsupported.
    #[error("unsupported protocol version {0:#010x}")]
    UnsupportedVersion(u32),

    /// Payload length exceeds configured maximum.
    #[error("payload too large: {length} > {max}")]
    PayloadTooLarge {
        /// Observed length.
        length: u32,
        /// Configured max.
        max: u32,
    },

    /// Buffer does not contain the full payload.
    #[error("truncated payload")]
    TruncatedPayload,

    /// Unknown message kind discriminant.
    #[error("unknown message kind {0}")]
    UnknownKind(u8),

    /// Payload JSON/serde decode failure.
    #[error("payload decode error: {0}")]
    PayloadDecode(String),

    /// Payload encode failure.
    #[error("payload encode error: {0}")]
    PayloadEncode(String),
}
