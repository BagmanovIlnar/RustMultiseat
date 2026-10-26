//! Hot-plug rematch of configured [`DeviceId`]s against a fresh [`HardwareTree`].
//!
//! When Windows reassigns instance ids after unplug/replug, seat assignments
//! are preserved by matching [`DeviceId::same_device`] (instance / interface
//! path) and, as a fallback, [`DeviceId::same_container`] + input kind.

use crate::hardware::{HardwareTree, InputDeviceKind};
use crate::ids::DeviceId;
use crate::seat::MultiseatConfig;

/// One successful device id rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRemap {
    /// Role label (`emergency_keyboard`, `seat_1_keyboard`, …).
    pub role: String,
    /// Id as stored in the previous config.
    pub from: DeviceId,
    /// Id taken from the current hardware snapshot.
    pub to: DeviceId,
}

/// Result of rematching a config to hardware.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemapReport {
    /// Rewritten configuration (identical to input when nothing changed).
    pub config: MultiseatConfig,
    /// Individual device rewrites.
    pub remaps: Vec<DeviceRemap>,
    /// Configured devices that could not be resolved in the snapshot.
    pub unresolved: Vec<(String, DeviceId)>,
}

impl RemapReport {
    /// True when at least one device id was rewritten.
    pub fn changed(&self) -> bool {
        !self.remaps.is_empty()
    }
}

/// Rewrites device ids in `config` to the current hardware snapshot ids.
///
/// Unresolved devices are left as-is and listed in [`RemapReport::unresolved`]
/// so validation can still report `unknown_device`.
pub fn remap_config(config: &MultiseatConfig, hardware: &HardwareTree) -> RemapReport {
    let mut out = config.clone();
    let mut remaps = Vec::new();
    let mut unresolved = Vec::new();
    let mut claimed = vec![false; hardware.input_devices.len()];

    if let Some(kbd) = out.emergency_keyboard.take() {
        match claim_device(hardware, &kbd, InputDeviceKind::Keyboard, &mut claimed) {
            Some(to) => {
                if to != kbd {
                    remaps.push(DeviceRemap {
                        role: "emergency_keyboard".into(),
                        from: kbd,
                        to: to.clone(),
                    });
                }
                out.emergency_keyboard = Some(to);
            }
            None => {
                unresolved.push(("emergency_keyboard".into(), kbd.clone()));
                out.emergency_keyboard = Some(kbd);
            }
        }
    }

    if let Some(ptr) = out.emergency_pointer.take() {
        match claim_device(hardware, &ptr, InputDeviceKind::Pointer, &mut claimed) {
            Some(to) => {
                if to != ptr {
                    remaps.push(DeviceRemap {
                        role: "emergency_pointer".into(),
                        from: ptr,
                        to: to.clone(),
                    });
                }
                out.emergency_pointer = Some(to);
            }
            None => {
                unresolved.push(("emergency_pointer".into(), ptr.clone()));
                out.emergency_pointer = Some(ptr);
            }
        }
    }

    for seat in &mut out.seats {
        let seat_no = seat.id.0;
        remap_list(
            &mut seat.keyboard_ids,
            hardware,
            InputDeviceKind::Keyboard,
            &mut claimed,
            &mut remaps,
            &mut unresolved,
            &format!("seat_{seat_no}_keyboard"),
        );
        remap_list(
            &mut seat.pointer_ids,
            hardware,
            InputDeviceKind::Pointer,
            &mut claimed,
            &mut remaps,
            &mut unresolved,
            &format!("seat_{seat_no}_pointer"),
        );
    }

    RemapReport {
        config: out,
        remaps,
        unresolved,
    }
}

fn remap_list(
    ids: &mut [DeviceId],
    hardware: &HardwareTree,
    kind: InputDeviceKind,
    claimed: &mut [bool],
    remaps: &mut Vec<DeviceRemap>,
    unresolved: &mut Vec<(String, DeviceId)>,
    role_prefix: &str,
) {
    for (idx, configured) in ids.iter_mut().enumerate() {
        let role = format!("{role_prefix}_{idx}");
        match claim_device(hardware, configured, kind, claimed) {
            Some(to) => {
                if &to != configured {
                    remaps.push(DeviceRemap {
                        role,
                        from: configured.clone(),
                        to: to.clone(),
                    });
                    *configured = to;
                }
            }
            None => unresolved.push((role, configured.clone())),
        }
    }
}

fn claim_device(
    hardware: &HardwareTree,
    configured: &DeviceId,
    kind: InputDeviceKind,
    claimed: &mut [bool],
) -> Option<DeviceId> {
    // Pass 1: same HID collection (instance or interface path).
    for (i, dev) in hardware.input_devices.iter().enumerate() {
        if claimed[i] || dev.kind != kind {
            continue;
        }
        if configured.same_device(&dev.id) {
            claimed[i] = true;
            return Some(dev.id.clone());
        }
    }
    // Pass 2: same physical container + kind (composite kit after path churn).
    for (i, dev) in hardware.input_devices.iter().enumerate() {
        if claimed[i] || dev.kind != kind {
            continue;
        }
        if configured.same_container(&dev.id) {
            claimed[i] = true;
            return Some(dev.id.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{DisplayInfo, InputDeviceInfo, PlatformCapabilities};
    use crate::ids::{DisplayId, SeatId, UserSid};
    use crate::seat::SeatConfig;

    fn hw_with(
        kbd: DeviceId,
        ptr: DeviceId,
        host_kbd: DeviceId,
        host_ptr: DeviceId,
    ) -> HardwareTree {
        HardwareTree {
            revision: 2,
            gpus: vec![],
            displays: vec![DisplayInfo {
                id: DisplayId("d1".into()),
                gpu_id: "g".into(),
                width: Some(1920),
                height: Some(1080),
                attached: true,
                virtual_display: false,
            }],
            usb_roots: vec![],
            input_devices: vec![
                InputDeviceInfo {
                    id: host_kbd,
                    kind: InputDeviceKind::Keyboard,
                    name: "Host Kbd".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: host_ptr,
                    kind: InputDeviceKind::Pointer,
                    name: "Host Ptr".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: kbd,
                    kind: InputDeviceKind::Keyboard,
                    name: "Seat Kbd".into(),
                    vendor_id: None,
                    product_id: None,
                },
                InputDeviceInfo {
                    id: ptr,
                    kind: InputDeviceKind::Pointer,
                    name: "Seat Ptr".into(),
                    vendor_id: None,
                    product_id: None,
                },
            ],
            capabilities: PlatformCapabilities::default(),
        }
    }

    #[test]
    fn remaps_by_interface_path_when_instance_changes() {
        let container = "bbbbbbbb-0000-0000-0000-000000000001";
        let old_kbd = DeviceId::new(
            "HID\\OLD_KBD",
            Some(r"\\?\hid#vid_bbbb#seat1_kbd".into()),
            Some(container.into()),
        );
        let new_kbd = DeviceId::new(
            "HID\\NEW_KBD",
            Some(r"\\?\HID#VID_BBBB#SEAT1_KBD".into()),
            Some(container.into()),
        );
        let old_ptr = DeviceId::new(
            "HID\\OLD_PTR",
            Some(r"\\?\hid#vid_bbbb#seat1_ptr".into()),
            Some(container.into()),
        );
        let new_ptr = DeviceId::new(
            "HID\\NEW_PTR",
            Some(r"\\?\hid#vid_bbbb#seat1_ptr".into()),
            Some(container.into()),
        );
        let host_kbd = DeviceId::from_instance("HOST_KBD");
        let host_ptr = DeviceId::from_instance("HOST_PTR");

        let config = MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(1),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![DisplayId("d1".into())],
                keyboard_ids: vec![old_kbd.clone()],
                pointer_ids: vec![old_ptr.clone()],
                enabled: true,
            }],
            emergency_keyboard: Some(host_kbd.clone()),
            emergency_pointer: Some(host_ptr.clone()),
        };
        let hardware = hw_with(new_kbd.clone(), new_ptr.clone(), host_kbd, host_ptr);
        let report = remap_config(&config, &hardware);
        assert!(report.changed());
        assert_eq!(report.config.seats[0].keyboard_ids[0], new_kbd);
        assert_eq!(report.config.seats[0].pointer_ids[0], new_ptr);
        assert!(report.unresolved.is_empty());
    }

    #[test]
    fn remaps_by_container_when_paths_diverge() {
        let container = "cccccccc-0000-0000-0000-000000000001";
        let configured = DeviceId::new(
            "HID\\GONE",
            Some(r"\\?\hid#old_path".into()),
            Some(container.into()),
        );
        let live = DeviceId::new(
            "HID\\LIVE",
            Some(r"\\?\hid#brand_new_path".into()),
            Some(container.into()),
        );
        let host_kbd = DeviceId::from_instance("HOST_KBD");
        let host_ptr = DeviceId::from_instance("HOST_PTR");
        let ptr = DeviceId::new(
            "HID\\PTR",
            Some(r"\\?\hid#ptr".into()),
            Some(container.into()),
        );

        let config = MultiseatConfig {
            seats: vec![SeatConfig {
                id: SeatId(1),
                user_sid: UserSid("S-1-5-21-1".into()),
                display_ids: vec![DisplayId("d1".into())],
                keyboard_ids: vec![configured],
                pointer_ids: vec![ptr.clone()],
                enabled: true,
            }],
            emergency_keyboard: Some(host_kbd.clone()),
            emergency_pointer: Some(host_ptr.clone()),
        };
        let hardware = hw_with(live.clone(), ptr, host_kbd, host_ptr);
        let report = remap_config(&config, &hardware);
        assert_eq!(report.config.seats[0].keyboard_ids[0], live);
        assert!(report.remaps.iter().any(|r| r.role.starts_with("seat_1_keyboard")));
    }
}
