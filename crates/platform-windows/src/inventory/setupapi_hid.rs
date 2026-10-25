//! SetupAPI enumeration for HID keyboards/pointers and USB devices.
//!
//! HID inventory prefers device-interface enumeration (`GUID_DEVINTERFACE_HID`)
//! so each [`DeviceId`] carries a stable `interface_path` for CreateFile and a
//! `container_id` (`SPDRP_BASE_CONTAINERID`) for composite-device matching
//! across hot-plug rescans.

use multiseat_core::{CoreError, CoreResult, DeviceId, InputDeviceInfo, InputDeviceKind, UsbNode};
use windows::core::GUID;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceInterfaceDetailW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SPDRP_BASE_CONTAINERID,
    SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::HumanInterfaceDevice::GUID_DEVINTERFACE_HID;
use windows::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS};

/// USB device class GUID `{36fc9e60-c465-11cf-8056-444553540000}`.
const GUID_DEVCLASS_USB: GUID = GUID::from_u128(0x36fc_9e60_c465_11cf_8056_4445_5354_0000);

/// Enumerates present HID devices and classifies keyboard/pointer when possible.
pub fn enumerate_input_devices() -> CoreResult<Vec<InputDeviceInfo>> {
    let rows = unsafe { enumerate_hid_interfaces() }?;
    Ok(rows
        .into_iter()
        .filter_map(|row| classify_hid(row))
        .collect())
}

/// Enumerates USB devices as a shallow root list (class enum; no interface path).
pub fn enumerate_usb_roots() -> CoreResult<Vec<UsbNode>> {
    let rows = unsafe { enumerate_class_rows(&GUID_DEVCLASS_USB) }?;
    Ok(rows
        .into_iter()
        .map(|row| UsbNode {
            device_id: DeviceId::new(row.instance_id, None, row.container_id),
            description: row.friendly_name,
            port: None,
            location_path: None,
            children: Vec::new(),
        })
        .collect())
}

struct HidRow {
    instance_id: String,
    friendly_name: String,
    interface_path: String,
    container_id: Option<String>,
}

struct DeviceRow {
    instance_id: String,
    friendly_name: String,
    container_id: Option<String>,
}

unsafe fn enumerate_hid_interfaces() -> CoreResult<Vec<HidRow>> {
    let list = SetupDiGetClassDevsW(
        Some(&GUID_DEVINTERFACE_HID),
        None,
        None,
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    )
    .map_err(|e| CoreError::Backend(format!("SetupDiGetClassDevsW(HID) failed: {e}")))?;

    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut iface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            InterfaceClassGuid: GUID::zeroed(),
            Flags: 0,
            Reserved: 0,
        };
        if SetupDiEnumDeviceInterfaces(list, None, &GUID_DEVINTERFACE_HID, index, &mut iface)
            .is_err()
        {
            let err = GetLastError();
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            let _ = SetupDiDestroyDeviceInfoList(list);
            return Err(CoreError::Backend(format!(
                "SetupDiEnumDeviceInterfaces(HID) failed: {err:?}"
            )));
        }

        match hid_from_interface(list, &iface) {
            Ok(row) => out.push(row),
            Err(err) => {
                tracing::debug!(error = %err, "skipping HID interface");
            }
        }
        index += 1;
    }

    let _ = SetupDiDestroyDeviceInfoList(list);
    Ok(out)
}

unsafe fn hid_from_interface(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface: &SP_DEVICE_INTERFACE_DATA,
) -> CoreResult<HidRow> {
    let (path, info) = interface_path_and_devinfo(list, iface)?;
    let instance_id = device_instance_id(list, &info)?;
    let friendly_name = registry_string(list, &info, SPDRP_FRIENDLYNAME)
        .or_else(|_| registry_string(list, &info, SPDRP_DEVICEDESC))
        .unwrap_or_else(|_| instance_id.clone());
    let container_id = registry_string(list, &info, SPDRP_BASE_CONTAINERID).ok();
    Ok(HidRow {
        instance_id,
        friendly_name,
        interface_path: path,
        container_id,
    })
}

unsafe fn enumerate_class_rows(class: &GUID) -> CoreResult<Vec<DeviceRow>> {
    let list = SetupDiGetClassDevsW(Some(class), None, None, DIGCF_PRESENT)
        .map_err(|e| CoreError::Backend(format!("SetupDiGetClassDevsW failed: {e}")))?;

    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut info = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ClassGuid: GUID::zeroed(),
            DevInst: 0,
            Reserved: 0,
        };

        if SetupDiEnumDeviceInfo(list, index, &mut info).is_err() {
            let err = GetLastError();
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            let _ = SetupDiDestroyDeviceInfoList(list);
            return Err(CoreError::Backend(format!(
                "SetupDiEnumDeviceInfo failed: {err:?}"
            )));
        }

        let instance_id = match device_instance_id(list, &info) {
            Ok(id) => id,
            Err(_) => {
                index += 1;
                continue;
            }
        };
        let friendly_name = registry_string(list, &info, SPDRP_FRIENDLYNAME)
            .or_else(|_| registry_string(list, &info, SPDRP_DEVICEDESC))
            .unwrap_or_else(|_| instance_id.clone());
        let container_id = registry_string(list, &info, SPDRP_BASE_CONTAINERID).ok();

        out.push(DeviceRow {
            instance_id,
            friendly_name,
            container_id,
        });
        index += 1;
    }

    let _ = SetupDiDestroyDeviceInfoList(list);
    Ok(out)
}

unsafe fn interface_path_and_devinfo(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface: &SP_DEVICE_INTERFACE_DATA,
) -> CoreResult<(String, SP_DEVINFO_DATA)> {
    let mut required = 0u32;
    let mut info = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ClassGuid: GUID::zeroed(),
        DevInst: 0,
        Reserved: 0,
    };
    let first = SetupDiGetDeviceInterfaceDetailW(
        list,
        iface,
        None,
        0,
        Some(&mut required),
        Some(&mut info),
    );
    if first.is_err() {
        let err = GetLastError();
        if err != ERROR_INSUFFICIENT_BUFFER {
            return Err(CoreError::Backend(format!(
                "SetupDiGetDeviceInterfaceDetailW size failed: {err:?}"
            )));
        }
    }

    let mut buf = vec![0u8; required.max(8) as usize];
    let detail = buf.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    (*detail).cbSize = if cfg!(target_pointer_width = "64") { 8 } else { 6 };

    SetupDiGetDeviceInterfaceDetailW(
        list,
        iface,
        Some(detail),
        required,
        None,
        Some(&mut info),
    )
    .map_err(|e| CoreError::Backend(format!("SetupDiGetDeviceInterfaceDetailW failed: {e}")))?;

    Ok((read_device_path(detail), info))
}

unsafe fn read_device_path(detail: *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W) -> String {
    let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
    let mut len = 0usize;
    while *path_ptr.add(len) != 0 {
        len += 1;
        if len > 2048 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, len))
}

fn classify_hid(row: HidRow) -> Option<InputDeviceInfo> {
    let blob = format!("{} {}", row.friendly_name, row.instance_id).to_ascii_uppercase();
    let kind = if blob.contains("KEYBOARD") || blob.contains("HID_DEVICE_SYSTEM_KEYBOARD") {
        InputDeviceKind::Keyboard
    } else if blob.contains("MOUSE")
        || blob.contains("POINTER")
        || blob.contains("HID_DEVICE_SYSTEM_MOUSE")
    {
        InputDeviceKind::Pointer
    } else {
        InputDeviceKind::Other
    };

    let (vendor_id, product_id) = parse_vid_pid(&row.instance_id);
    Some(InputDeviceInfo {
        id: DeviceId::new(
            row.instance_id,
            Some(row.interface_path),
            row.container_id,
        ),
        kind,
        name: row.friendly_name,
        vendor_id,
        product_id,
    })
}

fn parse_vid_pid(instance: &str) -> (Option<u16>, Option<u16>) {
    let upper = instance.to_ascii_uppercase();
    (
        extract_hex_after(&upper, "VID_"),
        extract_hex_after(&upper, "PID_"),
    )
}

fn extract_hex_after(s: &str, marker: &str) -> Option<u16> {
    let idx = s.find(marker)?;
    let start = idx + marker.len();
    let end = (start + 4).min(s.len());
    u16::from_str_radix(&s[start..end], 16).ok()
}

unsafe fn device_instance_id(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    info: &SP_DEVINFO_DATA,
) -> CoreResult<String> {
    let mut needed = 0u32;
    let _ = SetupDiGetDeviceInstanceIdW(list, info, None, Some(&mut needed));
    if GetLastError() != ERROR_INSUFFICIENT_BUFFER && needed == 0 {
        return Err(CoreError::Backend(
            "SetupDiGetDeviceInstanceIdW could not size buffer".into(),
        ));
    }
    let mut buf = vec![0u16; needed as usize];
    SetupDiGetDeviceInstanceIdW(list, info, Some(&mut buf), Some(&mut needed))
        .map_err(|e| CoreError::Backend(format!("SetupDiGetDeviceInstanceIdW failed: {e}")))?;
    Ok(utf16_z(&buf))
}

unsafe fn registry_string(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    info: &SP_DEVINFO_DATA,
    prop: windows::Win32::Devices::DeviceAndDriverInstallation::SETUP_DI_REGISTRY_PROPERTY,
) -> CoreResult<String> {
    let mut property_type = 0u32;
    let mut needed = 0u32;
    let _ = SetupDiGetDeviceRegistryPropertyW(
        list,
        info,
        prop,
        Some(&mut property_type),
        None,
        Some(&mut needed),
    );
    if needed == 0 {
        return Err(CoreError::Backend("empty registry property".into()));
    }
    let mut buf = vec![0u8; needed as usize];
    SetupDiGetDeviceRegistryPropertyW(
        list,
        info,
        prop,
        Some(&mut property_type),
        Some(&mut buf),
        Some(&mut needed),
    )
    .map_err(|e| CoreError::Backend(e.to_string()))?;

    let words = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect::<Vec<_>>();
    Ok(utf16_z(&words))
}

fn utf16_z(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_keyboard_gets_vid_pid() {
        let info = classify_hid(HidRow {
            instance_id: "HID\\VID_BBBB&PID_0001\\SEAT1_KBD".into(),
            friendly_name: "Seat 1 Keyboard".into(),
            interface_path: r"\\?\hid#vid_bbbb&pid_0001#seat1_kbd".into(),
            container_id: Some("{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}".into()),
        })
        .unwrap();
        assert_eq!(info.kind, InputDeviceKind::Keyboard);
        assert_eq!(info.vendor_id, Some(0xBBBB));
        assert_eq!(info.product_id, Some(0x0001));
        assert!(info.id.interface_path.is_some());
        assert_eq!(
            info.id.container_id.as_deref(),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
    }
}
