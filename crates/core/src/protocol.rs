//! Wire-protocol constants shared by framing and clients.

/// Magic bytes for every framed message (`RMS1`).
pub const PROTOCOL_MAGIC: [u8; 4] = *b"RMS1";

/// Current protocol major.minor encoded as `0xMMMMmmmm`.
pub const PROTOCOL_VERSION: u32 = 0x0001_0000;
