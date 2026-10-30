//! IOCTL codes and structs mirroring `drivers/input-filter/include/rms_if_ioctl.h`.
//!
//! Keep numeric values identical to the C header (CTL_CODE formula).

/// Vendor device type (user-defined). Must match `RMS_IF_DEVICE_TYPE`.
pub const RMS_IF_DEVICE_TYPE: u32 = 0x8A50;

/// ABI major. Must match `RMS_IF_ABI_VERSION` / [`super::INPUT_FILTER_ABI_VERSION`].
pub const RMS_IF_ABI_VERSION: u32 = 1;

const METHOD_BUFFERED: u32 = 0;
const FILE_READ_ACCESS: u32 = 0x0001;
const FILE_WRITE_ACCESS: u32 = 0x0002;

/// Windows `CTL_CODE` macro (buffered I/O).
pub const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

/// `IOCTL_RMS_IF_GET_VERSION`
pub const IOCTL_RMS_IF_GET_VERSION: u32 =
    ctl_code(RMS_IF_DEVICE_TYPE, 0x800, METHOD_BUFFERED, FILE_READ_ACCESS);

/// `IOCTL_RMS_IF_SET_FAIL_OPEN`
pub const IOCTL_RMS_IF_SET_FAIL_OPEN: u32 = ctl_code(
    RMS_IF_DEVICE_TYPE,
    0x801,
    METHOD_BUFFERED,
    FILE_WRITE_ACCESS,
);

/// `IOCTL_RMS_IF_SET_ROUTES`
pub const IOCTL_RMS_IF_SET_ROUTES: u32 = ctl_code(
    RMS_IF_DEVICE_TYPE,
    0x802,
    METHOD_BUFFERED,
    FILE_WRITE_ACCESS,
);

/// `IOCTL_RMS_IF_QUERY_STATS`
pub const IOCTL_RMS_IF_QUERY_STATS: u32 =
    ctl_code(RMS_IF_DEVICE_TYPE, 0x803, METHOD_BUFFERED, FILE_READ_ACCESS);

/// Output of `IOCTL_RMS_IF_GET_VERSION` (`RMS_IF_VERSION_INFO`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RmsIfVersionInfo {
    /// ABI major (`RMS_IF_ABI_VERSION`).
    pub abi_version: u32,
    /// Driver build stamp.
    pub driver_build: u32,
    /// Capability flags (bit0 fail-open default, bit1 routes).
    pub caps_flags: u32,
}

/// Input of `IOCTL_RMS_IF_SET_FAIL_OPEN` (`RMS_IF_FAIL_OPEN_REQ`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RmsIfFailOpenReq {
    /// 1 = fail-open, 0 = armed.
    pub enabled: u32,
}

/// Output of `IOCTL_RMS_IF_QUERY_STATS` (`RMS_IF_STATS`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RmsIfStats {
    /// Packets observed.
    pub packets_seen: u64,
    /// Packets passed to class driver / host.
    pub packets_passed: u64,
    /// Packets suppressed for seat routing.
    pub packets_suppressed: u64,
    /// Queue overflow events (must trip fail-open).
    pub queue_overflows: u64,
    /// 1 if currently fail-open.
    pub fail_open: u32,
    /// Reserved.
    pub reserved: u32,
}

impl RmsIfStats {
    /// Empty counters (soft / absent device).
    pub const fn empty_fail_open() -> Self {
        Self {
            packets_seen: 0,
            packets_passed: 0,
            packets_suppressed: 0,
            queue_overflows: 0,
            fail_open: 1,
            reserved: 0,
        }
    }
}

/// `RMS_IF_MAX_ROUTES`
pub const RMS_IF_MAX_ROUTES: usize = 32;
/// `RMS_IF_INSTANCE_ID_LEN`
pub const RMS_IF_INSTANCE_ID_LEN: usize = 128;

/// `RMS_IF_ROUTE_F_KEYBOARD`
pub const RMS_IF_ROUTE_F_KEYBOARD: u32 = 0x1;
/// `RMS_IF_ROUTE_F_POINTER`
pub const RMS_IF_ROUTE_F_POINTER: u32 = 0x2;
/// `RMS_IF_ROUTE_F_SUPPRESS_HOST`
pub const RMS_IF_ROUTE_F_SUPPRESS_HOST: u32 = 0x4;
/// `RMS_IF_TABLE_F_REPLACE_ALL`
pub const RMS_IF_TABLE_F_REPLACE_ALL: u32 = 0x1;

/// One route row (`RMS_IF_ROUTE_ENTRY`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RmsIfRouteEntry {
    /// Seat id (`0` = host/emergency).
    pub seat_id: u32,
    /// `RMS_IF_ROUTE_F_*` flags.
    pub flags: u32,
    /// UTF-8 instance id, NUL-padded.
    pub instance_id: [u8; RMS_IF_INSTANCE_ID_LEN],
}

impl std::fmt::Debug for RmsIfRouteEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id = std::str::from_utf8(&self.instance_id)
            .unwrap_or("")
            .trim_end_matches('\0');
        f.debug_struct("RmsIfRouteEntry")
            .field("seat_id", &self.seat_id)
            .field("flags", &self.flags)
            .field("instance_id", &id)
            .finish()
    }
}

impl PartialEq for RmsIfRouteEntry {
    fn eq(&self, other: &Self) -> bool {
        self.seat_id == other.seat_id
            && self.flags == other.flags
            && self.instance_id == other.instance_id
    }
}

impl Eq for RmsIfRouteEntry {}

impl RmsIfRouteEntry {
    /// Builds an entry from a UTF-8 instance id (truncated/padded to 128 bytes).
    pub fn new(seat_id: u32, flags: u32, instance_id: &str) -> Self {
        let mut buf = [0u8; RMS_IF_INSTANCE_ID_LEN];
        let bytes = instance_id.as_bytes();
        let n = bytes.len().min(RMS_IF_INSTANCE_ID_LEN);
        buf[..n].copy_from_slice(&bytes[..n]);
        Self {
            seat_id,
            flags,
            instance_id: buf,
        }
    }
}

/// Full SET_ROUTES payload (`RMS_IF_ROUTE_TABLE`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RmsIfRouteTable {
    /// Must be [`RMS_IF_ABI_VERSION`].
    pub abi_version: u32,
    /// Number of valid entries in `entries`.
    pub entry_count: u32,
    /// `RMS_IF_TABLE_F_*`.
    pub flags: u32,
    /// Reserved (0).
    pub reserved: u32,
    /// Fixed-capacity route rows.
    pub entries: [RmsIfRouteEntry; RMS_IF_MAX_ROUTES],
}

impl std::fmt::Debug for RmsIfRouteTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.entry_count as usize;
        f.debug_struct("RmsIfRouteTable")
            .field("abi_version", &self.abi_version)
            .field("entry_count", &self.entry_count)
            .field("flags", &self.flags)
            .field("entries", &&self.entries[..n.min(RMS_IF_MAX_ROUTES)])
            .finish()
    }
}

impl PartialEq for RmsIfRouteTable {
    fn eq(&self, other: &Self) -> bool {
        self.abi_version == other.abi_version
            && self.entry_count == other.entry_count
            && self.flags == other.flags
            && self.reserved == other.reserved
            && self.entries[..self.entry_count as usize]
                == other.entries[..other.entry_count as usize]
    }
}

impl Eq for RmsIfRouteTable {}

impl RmsIfRouteTable {
    /// Empty replace-all table (fail-open friendly: no suppress routes).
    pub fn empty_replace() -> Self {
        Self {
            abi_version: RMS_IF_ABI_VERSION,
            entry_count: 0,
            flags: RMS_IF_TABLE_F_REPLACE_ALL,
            reserved: 0,
            entries: [RmsIfRouteEntry {
                seat_id: 0,
                flags: 0,
                instance_id: [0; RMS_IF_INSTANCE_ID_LEN],
            }; RMS_IF_MAX_ROUTES],
        }
    }

    /// Builds a table from entries (errors if too many).
    pub fn from_entries(entries: &[RmsIfRouteEntry], flags: u32) -> Result<Self, &'static str> {
        if entries.len() > RMS_IF_MAX_ROUTES {
            return Err("too many route entries");
        }
        let mut table = Self::empty_replace();
        table.flags = flags;
        table.entry_count = entries.len() as u32;
        table.entries[..entries.len()].copy_from_slice(entries);
        Ok(table)
    }

    /// Validates ABI + count before IOCTL.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.abi_version != RMS_IF_ABI_VERSION {
            return Err("route table abi mismatch");
        }
        if self.entry_count as usize > RMS_IF_MAX_ROUTES {
            return Err("route table entry_count out of range");
        }
        Ok(())
    }

    /// Byte length of the METHOD_BUFFERED input (full fixed struct).
    pub const fn ioctl_buffer_len() -> usize {
        std::mem::size_of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctl_codes_match_documented_formula() {
        // Precomputed with CTL_CODE(0x8A50, fn, METHOD_BUFFERED, access).
        assert_eq!(IOCTL_RMS_IF_GET_VERSION, 0x8A50_4000 | (0x800 << 2));
        assert_eq!(IOCTL_RMS_IF_SET_FAIL_OPEN, 0x8A50_8000 | (0x801 << 2));
        assert_eq!(IOCTL_RMS_IF_SET_ROUTES, 0x8A50_8000 | (0x802 << 2));
        assert_eq!(IOCTL_RMS_IF_QUERY_STATS, 0x8A50_4000 | (0x803 << 2));
    }

    #[test]
    fn struct_sizes_match_c_layout() {
        assert_eq!(std::mem::size_of::<RmsIfVersionInfo>(), 12);
        assert_eq!(std::mem::size_of::<RmsIfFailOpenReq>(), 4);
        assert_eq!(std::mem::size_of::<RmsIfStats>(), 40);
        assert_eq!(std::mem::size_of::<RmsIfRouteEntry>(), 136);
        // 16 header + 32 * 136
        assert_eq!(std::mem::size_of::<RmsIfRouteTable>(), 16 + 32 * 136);
        assert_eq!(RmsIfRouteTable::ioctl_buffer_len(), 16 + 32 * 136);
    }

    #[test]
    fn route_table_roundtrip_builder() {
        let e = RmsIfRouteEntry::new(
            1,
            RMS_IF_ROUTE_F_KEYBOARD | RMS_IF_ROUTE_F_SUPPRESS_HOST,
            "HID\\VID_1",
        );
        let table = RmsIfRouteTable::from_entries(&[e], RMS_IF_TABLE_F_REPLACE_ALL).unwrap();
        table.validate().unwrap();
        assert_eq!(table.entry_count, 1);
        assert_eq!(table.entries[0], e);
        assert!(RmsIfRouteTable::from_entries(&[e; RMS_IF_MAX_ROUTES + 1], 0).is_err());
    }

    #[test]
    fn ioctl_codes_are_distinct() {
        let codes = [
            IOCTL_RMS_IF_GET_VERSION,
            IOCTL_RMS_IF_SET_FAIL_OPEN,
            IOCTL_RMS_IF_SET_ROUTES,
            IOCTL_RMS_IF_QUERY_STATS,
        ];
        for (i, a) in codes.iter().enumerate() {
            for (j, b) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
