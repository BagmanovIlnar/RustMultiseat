//! User-mode control plane for `drivers/input-filter` (IOCTL ABI stub).
//!
//! Compiles on all hosts. On Windows, tries a real `CreateFile`/`DeviceIoControl`
//! path; if the KMDF device is missing, behaves as absent (fail-open).
//! Soft-present (`MULTISEAT_INPUT_FILTER=1`) keeps bookkeeping without IOCTL.
//! Codes/structs: [`super::ioctl`] ↔ `drivers/input-filter/include/rms_if_ioctl.h`.

use std::fmt;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::ioctl::{
    RmsIfFailOpenReq, RmsIfRouteTable, RmsIfStats, RmsIfVersionInfo, IOCTL_RMS_IF_GET_VERSION,
    IOCTL_RMS_IF_QUERY_STATS, IOCTL_RMS_IF_SET_FAIL_OPEN, IOCTL_RMS_IF_SET_ROUTES,
    RMS_IF_ABI_VERSION,
};

/// Lab / production symbolic link hint (see `drivers/input-filter/INTERFACE.md`).
pub const INPUT_FILTER_CONTROL_DEVICE: &str = r"\\.\RustMultiseatInputFilter";

/// IOCTL ABI major version advertised by a future driver.
pub const INPUT_FILTER_ABI_VERSION: u32 = RMS_IF_ABI_VERSION;

/// Why the control device cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputFilterError {
    /// Filter control device is not installed / not reachable.
    NotPresent,
    /// Open or IOCTL failed (Windows).
    Io(String),
    /// Driver ABI newer/older than this user-mode stub.
    IncompatibleVersion {
        /// Version reported by the driver.
        driver: u32,
        /// Version expected by this crate.
        expected: u32,
    },
}

impl fmt::Display for InputFilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPresent => write!(
                f,
                "input filter control device not present ({INPUT_FILTER_CONTROL_DEVICE})"
            ),
            Self::Io(msg) => write!(f, "input filter IO: {msg}"),
            Self::IncompatibleVersion { driver, expected } => {
                write!(f, "input filter ABI driver={driver} expected={expected}")
            }
        }
    }
}

impl std::error::Error for InputFilterError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Absent,
    Soft,
    #[cfg_attr(not(windows), allow(dead_code))]
    Native,
}

/// Handle to the filter control device (or a documented absence).
#[derive(Debug)]
pub struct InputFilterControl {
    presence: Presence,
    #[cfg(windows)]
    native: Option<super::native_ioctl::NativeFilterDevice>,
}

impl InputFilterControl {
    /// Attempts to open the control device.
    pub fn open() -> Result<Self, InputFilterError> {
        #[cfg(windows)]
        {
            match super::native_ioctl::NativeFilterDevice::open() {
                Ok(dev) => {
                    info!(
                        device = INPUT_FILTER_CONTROL_DEVICE,
                        "opened native input filter control device"
                    );
                    return Ok(Self {
                        presence: Presence::Native,
                        native: Some(dev),
                    });
                }
                Err(InputFilterError::NotPresent) | Err(InputFilterError::Io(_)) => {
                    debug!(
                        device = INPUT_FILTER_CONTROL_DEVICE,
                        "native input filter control device unavailable"
                    );
                }
                Err(other) => return Err(other),
            }
            if std::env::var_os("MULTISEAT_INPUT_FILTER").is_some_and(|v| v == "1") {
                debug!(
                    device = INPUT_FILTER_CONTROL_DEVICE,
                    "MULTISEAT_INPUT_FILTER=1; control device treated as soft-present"
                );
                return Ok(Self {
                    presence: Presence::Soft,
                    native: None,
                });
            }
            Ok(Self {
                presence: Presence::Absent,
                native: None,
            })
        }

        #[cfg(not(windows))]
        {
            if std::env::var_os("MULTISEAT_INPUT_FILTER").is_some_and(|v| v == "1") {
                return Ok(Self {
                    presence: Presence::Soft,
                });
            }
            Ok(Self {
                presence: Presence::Absent,
            })
        }
    }

    /// True when a control path is available (real device or lab soft-present).
    pub fn is_present(&self) -> bool {
        !matches!(self.presence, Presence::Absent)
    }

    /// True when using soft bookkeeping only (no DeviceIoControl).
    pub fn is_soft(&self) -> bool {
        matches!(self.presence, Presence::Soft)
    }

    /// IOCTL codes this build will issue (for lab diagnostics).
    pub fn ioctl_table(&self) -> [(&'static str, u32); 4] {
        [
            ("IOCTL_RMS_IF_GET_VERSION", IOCTL_RMS_IF_GET_VERSION),
            ("IOCTL_RMS_IF_SET_FAIL_OPEN", IOCTL_RMS_IF_SET_FAIL_OPEN),
            ("IOCTL_RMS_IF_SET_ROUTES", IOCTL_RMS_IF_SET_ROUTES),
            ("IOCTL_RMS_IF_QUERY_STATS", IOCTL_RMS_IF_QUERY_STATS),
        ]
    }

    /// Queries driver ABI version when present.
    pub fn get_version(&self) -> Result<RmsIfVersionInfo, InputFilterError> {
        match self.presence {
            Presence::Absent => Err(InputFilterError::NotPresent),
            Presence::Soft => Ok(RmsIfVersionInfo {
                abi_version: INPUT_FILTER_ABI_VERSION,
                driver_build: 0,
                caps_flags: 0b11,
            }),
            #[cfg(windows)]
            Presence::Native => self
                .native
                .as_ref()
                .ok_or(InputFilterError::NotPresent)?
                .get_version(),
            #[cfg(not(windows))]
            Presence::Native => unreachable!(),
        }
    }

    /// Requests fail-open (pass-through) on the filter.
    pub fn set_fail_open(&self, enabled: bool) -> Result<(), InputFilterError> {
        match self.presence {
            Presence::Absent => {
                debug!(
                    enabled,
                    "set_fail_open: no filter device; already fail-open"
                );
                Ok(())
            }
            Presence::Soft => {
                let req = RmsIfFailOpenReq {
                    enabled: u32::from(enabled),
                };
                info!(
                    enabled = req.enabled,
                    ioctl = IOCTL_RMS_IF_SET_FAIL_OPEN,
                    device = INPUT_FILTER_CONTROL_DEVICE,
                    "SOFT set_fail_open (DeviceIoControl pending; bookkeeping only)"
                );
                Ok(())
            }
            #[cfg(windows)]
            Presence::Native => self
                .native
                .as_ref()
                .ok_or(InputFilterError::NotPresent)?
                .set_fail_open(enabled),
            #[cfg(not(windows))]
            Presence::Native => unreachable!(),
        }
    }

    /// Atomically replace/clear routes (`IOCTL_RMS_IF_SET_ROUTES`).
    pub fn set_routes(&self, table: &RmsIfRouteTable) -> Result<(), InputFilterError> {
        table
            .validate()
            .map_err(|m| InputFilterError::Io(m.into()))?;
        match self.presence {
            Presence::Absent => Err(InputFilterError::NotPresent),
            Presence::Soft => {
                info!(
                    entries = table.entry_count,
                    flags = table.flags,
                    ioctl = IOCTL_RMS_IF_SET_ROUTES,
                    "SOFT set_routes (DeviceIoControl pending; bookkeeping only)"
                );
                Ok(())
            }
            #[cfg(windows)]
            Presence::Native => self
                .native
                .as_ref()
                .ok_or(InputFilterError::NotPresent)?
                .set_routes(table),
            #[cfg(not(windows))]
            Presence::Native => unreachable!(),
        }
    }

    /// Clears all routes (replace-all empty) then fail-open — safe emergency helper.
    pub fn clear_routes_and_fail_open(&self) -> Result<(), InputFilterError> {
        if self.is_present() {
            if let Err(err) = self.set_routes(&RmsIfRouteTable::empty_replace()) {
                warn!(error = %err, "clear_routes before fail-open");
            }
        }
        self.set_fail_open(true)
    }

    /// Push a validated route table; optionally arm filtering (`fail_open=0`).
    ///
    /// Requires a present control path (native or `MULTISEAT_INPUT_FILTER=1` soft).
    /// Lab tip: apply with `arm=false` first, confirm stats, then arm.
    pub fn apply_routes(&self, table: &RmsIfRouteTable, arm: bool) -> Result<(), InputFilterError> {
        if !self.is_present() {
            return Err(InputFilterError::NotPresent);
        }
        self.set_routes(table)?;
        if arm {
            self.set_fail_open(false)?;
            info!(
                entries = table.entry_count,
                soft = self.is_soft(),
                "input filter routes applied and armed (fail_open=0)"
            );
        } else {
            info!(
                entries = table.entry_count,
                soft = self.is_soft(),
                "input filter routes applied; still fail-open (not armed)"
            );
        }
        Ok(())
    }

    /// Queries counters when present; absent → empty fail-open stats.
    pub fn query_stats(&self) -> Result<RmsIfStats, InputFilterError> {
        match self.presence {
            Presence::Absent => Ok(RmsIfStats::empty_fail_open()),
            Presence::Soft => Ok(RmsIfStats::empty_fail_open()),
            #[cfg(windows)]
            Presence::Native => self
                .native
                .as_ref()
                .ok_or(InputFilterError::NotPresent)?
                .query_stats(),
            #[cfg(not(windows))]
            Presence::Native => unreachable!(),
        }
    }
}

/// Snapshot from a control-device health ping (daemon/ctl heartbeat helper).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFilterHealth {
    /// Control path available (native or soft).
    pub present: bool,
    /// Soft bookkeeping only (no DeviceIoControl).
    pub soft: bool,
    /// Driver/soft ABI version when readable.
    pub abi_version: Option<u32>,
    /// Current fail-open flag (`1` = pass-through).
    pub fail_open: u32,
    /// True when absent/soft/native path looks safe for host input.
    pub healthy: bool,
    /// Whether this ping forced `emergency_fail_open`.
    pub forced_fail_open: bool,
    /// Operator-facing detail.
    pub detail: String,
}

impl InputFilterControl {
    /// Queries version + stats without changing routes (lab/daemon heartbeat).
    pub fn health_ping(&self) -> InputFilterHealth {
        if !self.is_present() {
            return InputFilterHealth {
                present: false,
                soft: false,
                abi_version: None,
                fail_open: 1,
                healthy: true,
                forced_fail_open: false,
                detail: format!(
                    "control device absent ({INPUT_FILTER_CONTROL_DEVICE}); host already fail-open"
                ),
            };
        }

        let soft = self.is_soft();
        let version = self.get_version();
        let stats = self.query_stats();
        match (version, stats) {
            (Ok(v), Ok(s)) => InputFilterHealth {
                present: true,
                soft,
                abi_version: Some(v.abi_version),
                fail_open: s.fail_open,
                healthy: true,
                forced_fail_open: false,
                detail: if soft {
                    "soft-present: version/stats bookkeeping ok".into()
                } else {
                    format!(
                        "native ok: abi={} build={} fail_open={}",
                        v.abi_version, v.driver_build, s.fail_open
                    )
                },
            },
            (Err(err), _) | (_, Err(err)) => InputFilterHealth {
                present: true,
                soft,
                abi_version: None,
                fail_open: 1,
                healthy: false,
                forced_fail_open: false,
                detail: format!("health query failed: {err}"),
            },
        }
    }
}

/// Opens the control device, pings health; on unhealthy present path forces fail-open.
pub fn filter_heartbeat() -> Result<InputFilterHealth, InputFilterError> {
    let ctl = InputFilterControl::open()?;
    let mut health = ctl.health_ping();
    if health.present && !health.healthy {
        warn!(detail = %health.detail, "input filter heartbeat unhealthy; forcing fail-open");
        ctl.clear_routes_and_fail_open()?;
        health.forced_fail_open = true;
        health.fail_open = 1;
        health.detail = format!("{}; forced emergency_fail_open", health.detail);
    }
    Ok(health)
}

/// Best-effort emergency: open control device (if any), clear routes, fail-open.
pub fn emergency_fail_open() -> Result<(), InputFilterError> {
    let ctl = InputFilterControl::open()?;
    ctl.clear_routes_and_fail_open()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ioctl::{
        RmsIfRouteEntry, RMS_IF_ROUTE_F_KEYBOARD, RMS_IF_TABLE_F_REPLACE_ALL,
    };

    #[test]
    fn open_absent_on_dev_host_without_env() {
        let _g = crate::test_env::lock();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        let ctl = InputFilterControl::open().unwrap();
        assert!(!ctl.is_present());
        assert!(ctl.set_fail_open(true).is_ok());
        assert!(matches!(
            ctl.get_version(),
            Err(InputFilterError::NotPresent)
        ));
        let stats = ctl.query_stats().unwrap();
        assert_eq!(stats.fail_open, 1);
        assert!(matches!(
            ctl.set_routes(&RmsIfRouteTable::empty_replace()),
            Err(InputFilterError::NotPresent)
        ));
    }

    #[test]
    fn soft_set_routes_when_filter_env() {
        let _g = crate::test_env::lock();
        std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
        let ctl = InputFilterControl::open().unwrap();
        assert!(ctl.is_present());
        assert!(ctl.is_soft());
        let entry = RmsIfRouteEntry::new(1, RMS_IF_ROUTE_F_KEYBOARD, "HID\\TEST");
        let table = RmsIfRouteTable::from_entries(&[entry], RMS_IF_TABLE_F_REPLACE_ALL).unwrap();
        ctl.set_routes(&table).unwrap();
        ctl.clear_routes_and_fail_open().unwrap();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
    }

    #[test]
    fn emergency_fail_open_ok_when_absent() {
        let _g = crate::test_env::lock();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        assert!(emergency_fail_open().is_ok());
    }

    #[test]
    fn ioctl_table_lists_four_codes() {
        let ctl = InputFilterControl::open().unwrap();
        assert_eq!(ctl.ioctl_table().len(), 4);
        assert_eq!(INPUT_FILTER_ABI_VERSION, RMS_IF_ABI_VERSION);
    }

    #[test]
    fn heartbeat_healthy_when_absent() {
        let _g = crate::test_env::lock();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        let health = filter_heartbeat().unwrap();
        assert!(!health.present);
        assert!(health.healthy);
        assert!(!health.forced_fail_open);
        assert_eq!(health.fail_open, 1);
    }

    #[test]
    fn heartbeat_healthy_when_soft() {
        let _g = crate::test_env::lock();
        std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
        let health = filter_heartbeat().unwrap();
        assert!(health.present);
        assert!(health.soft);
        assert!(health.healthy);
        assert_eq!(health.abi_version, Some(INPUT_FILTER_ABI_VERSION));
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
    }

    #[test]
    fn apply_routes_requires_present() {
        let _g = crate::test_env::lock();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
        let ctl = InputFilterControl::open().unwrap();
        let table = RmsIfRouteTable::empty_replace();
        assert!(matches!(
            ctl.apply_routes(&table, true),
            Err(InputFilterError::NotPresent)
        ));
    }

    #[test]
    fn soft_apply_routes_and_arm() {
        let _g = crate::test_env::lock();
        std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
        let ctl = InputFilterControl::open().unwrap();
        let entry = RmsIfRouteEntry::new(1, RMS_IF_ROUTE_F_KEYBOARD, "HID\\TEST");
        let table = RmsIfRouteTable::from_entries(&[entry], RMS_IF_TABLE_F_REPLACE_ALL).unwrap();
        ctl.apply_routes(&table, true).unwrap();
        ctl.clear_routes_and_fail_open().unwrap();
        std::env::remove_var("MULTISEAT_INPUT_FILTER");
    }
}
