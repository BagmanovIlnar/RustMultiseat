//! Payload codec using a fixed JSON representation for version 1.
//!
//! JSON is chosen for the initial protocol so payloads remain inspectable in
//! tests and logs. A future protocol version may switch the payload encoding
//! without changing the outer frame layout.

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{FrameError, FrameResult};

/// Encodes a serializable payload.
pub fn encode_payload<T: Serialize>(value: &T) -> FrameResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| FrameError::PayloadEncode(e.to_string()))
}

/// Decodes a payload into `T`.
pub fn decode_payload<T: DeserializeOwned>(bytes: &[u8]) -> FrameResult<T> {
    serde_json::from_slice(bytes).map_err(|e| FrameError::PayloadDecode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::{ClientRequest, PROTOCOL_VERSION};

    #[test]
    fn request_roundtrip() {
        let req = ClientRequest::Hello {
            client_version: PROTOCOL_VERSION,
        };
        let bytes = encode_payload(&req).unwrap();
        let decoded: ClientRequest = decode_payload(&bytes).unwrap();
        assert_eq!(decoded, req);
    }
}
