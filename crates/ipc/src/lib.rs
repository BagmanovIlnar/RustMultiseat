//! Length-prefixed IPC framing for RustMultiseat.
//!
//! Frame layout (little-endian):
//! ```text
//! magic[4] | protocol_version:u32 | message_kind:u8 | flags:u8 | reserved:u16
//! | request_id:u64 | payload_length:u32 | payload[payload_length]
//! ```

#![deny(missing_docs)]

mod codec;
mod error;
mod frame;
pub mod transport;

pub use codec::{decode_payload, encode_payload};
pub use error::{FrameError, FrameResult};
pub use frame::{
    decode_frame, encode_frame, FrameHeader, MessageKind, MAX_FRAME_PAYLOAD, MAX_QUEUE_DEPTH,
};
pub use transport::{
    default_endpoint_str, is_named_pipe_path, Endpoint, DEFAULT_TCP_ENDPOINT, WINDOWS_PIPE_NAME,
};
