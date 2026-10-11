//! Binary frame header encode/decode.

use bytes::{Buf, BufMut, BytesMut};
use multiseat_core::{PROTOCOL_MAGIC, PROTOCOL_VERSION};

use crate::error::{FrameError, FrameResult};

/// Maximum accepted payload size (1 MiB).
pub const MAX_FRAME_PAYLOAD: u32 = 1_048_576;

/// Soft limit for outbound event queue depth.
pub const MAX_QUEUE_DEPTH: usize = 256;

/// Fixed header size in bytes.
pub const HEADER_LEN: usize = 24;

/// Message class discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageKind {
    /// Client request.
    Request = 1,
    /// Server response.
    Response = 2,
    /// Unsolicited daemon event.
    Event = 3,
}

impl MessageKind {
    fn from_u8(value: u8) -> FrameResult<Self> {
        match value {
            1 => Ok(Self::Request),
            2 => Ok(Self::Response),
            3 => Ok(Self::Event),
            other => Err(FrameError::UnknownKind(other)),
        }
    }
}

/// Decoded frame header + payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// Protocol version from the frame.
    pub protocol_version: u32,
    /// Message class.
    pub kind: MessageKind,
    /// Reserved flags.
    pub flags: u8,
    /// Correlation id.
    pub request_id: u64,
    /// Raw payload.
    pub payload: Vec<u8>,
}

/// Encodes a complete frame into `out`.
pub fn encode_frame(
    kind: MessageKind,
    flags: u8,
    request_id: u64,
    payload: &[u8],
    out: &mut BytesMut,
) -> FrameResult<()> {
    if payload.len() as u32 > MAX_FRAME_PAYLOAD {
        return Err(FrameError::PayloadTooLarge {
            length: payload.len() as u32,
            max: MAX_FRAME_PAYLOAD,
        });
    }

    out.reserve(HEADER_LEN + payload.len());
    out.extend_from_slice(&PROTOCOL_MAGIC);
    out.put_u32_le(PROTOCOL_VERSION);
    out.put_u8(kind as u8);
    out.put_u8(flags);
    out.put_u16_le(0);
    out.put_u64_le(request_id);
    out.put_u32_le(payload.len() as u32);
    out.extend_from_slice(payload);
    Ok(())
}

/// Decodes one frame from the front of `buf`, advancing the cursor on success.
///
/// Returns `Ok(None)` when more bytes are needed.
pub fn decode_frame(buf: &mut BytesMut) -> FrameResult<Option<FrameHeader>> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }

    let mut cursor = &buf[..HEADER_LEN];
    let mut magic = [0u8; 4];
    cursor.copy_to_slice(&mut magic);
    if magic != PROTOCOL_MAGIC {
        return Err(FrameError::InvalidMagic);
    }

    let protocol_version = cursor.get_u32_le();
    if protocol_version != PROTOCOL_VERSION {
        return Err(FrameError::UnsupportedVersion(protocol_version));
    }

    let kind = MessageKind::from_u8(cursor.get_u8())?;
    let flags = cursor.get_u8();
    let _reserved = cursor.get_u16_le();
    let request_id = cursor.get_u64_le();
    let payload_length = cursor.get_u32_le();

    if payload_length > MAX_FRAME_PAYLOAD {
        return Err(FrameError::PayloadTooLarge {
            length: payload_length,
            max: MAX_FRAME_PAYLOAD,
        });
    }

    let total = HEADER_LEN + payload_length as usize;
    if buf.len() < total {
        return Ok(None);
    }

    let _ = buf.split_to(HEADER_LEN);
    let payload = buf.split_to(payload_length as usize).to_vec();

    Ok(Some(FrameHeader {
        protocol_version,
        kind,
        flags,
        request_id,
        payload,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_frame() {
        let mut out = BytesMut::new();
        encode_frame(MessageKind::Request, 0, 42, b"{\"x\":1}", &mut out).unwrap();

        let mut buf = out;
        let frame = decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(frame.request_id, 42);
        assert_eq!(frame.kind, MessageKind::Request);
        assert_eq!(frame.payload, b"{\"x\":1}");
        assert!(buf.is_empty());
    }

    #[test]
    fn needs_more_bytes() {
        let mut buf = BytesMut::from(&b"RMS1"[..]);
        assert!(decode_frame(&mut buf).unwrap().is_none());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = BytesMut::from(&b"XXXX\x00\x00\x01\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"[..]);
        assert_eq!(
            decode_frame(&mut buf).unwrap_err(),
            FrameError::InvalidMagic
        );
    }

    #[test]
    fn rejects_oversized_payload_length() {
        let mut out = BytesMut::new();
        out.extend_from_slice(&PROTOCOL_MAGIC);
        out.put_u32_le(PROTOCOL_VERSION);
        out.put_u8(1);
        out.put_u8(0);
        out.put_u16_le(0);
        out.put_u64_le(1);
        out.put_u32_le(MAX_FRAME_PAYLOAD + 1);
        assert!(matches!(
            decode_frame(&mut out),
            Err(FrameError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn random_noise_never_panics() {
        // Lightweight fuzz-style soak: decoder must stay panic-free.
        let mut seed = 0xC0FFEE_u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let len = ((seed >> 33) as usize % 64) + 1;
            let mut bytes = vec![0u8; len];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((seed >> ((i % 8) * 8)) as u8).wrapping_add(i as u8);
            }
            let mut buf = BytesMut::from(bytes.as_slice());
            let _ = decode_frame(&mut buf);
        }
    }
}
