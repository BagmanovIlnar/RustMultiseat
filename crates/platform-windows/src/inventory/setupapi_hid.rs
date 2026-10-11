//! SetupAPI enumeration for HID keyboards/pointers and USB devices.

use multiseat_core::{CoreError, CoreResult, DeviceId, InputDeviceInfo, InputDeviceKind, UsbNode};
use windows::core::GUID;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW, DIGCF_PRESENT,
    SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};
use windows::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS};

/// HID class GUID `{745a17a0-74d3-11d0-b6fe-00a0c90f57da}`.
const GUID_DEVCLASS_HIDCLASS: GUID = GUID::from_u128(0x745a_17a0_74d3_11d0_b6fe_00a0_c90f_57da);

/// USB device class GUID `{36fc9e60-c465-11cf-8056-444553540000}`.
const GUID_DEVCLASS_USB: GUID = GUID::from_u128(0x36fc_9e60_c465_11cf_8056_4445_5354_0000);

/// Enumerates present HID devices and classifies keyboard/pointer when possible.
pub fn enumerate_input_devices() -> CoreResult<Vec<InputDeviceInfo>> {
    let rows = unsafe { enumerate_class_rows(&GUID_DEVCLASS_HIDCLASS) }?;
    Ok(rows
        .into_iter()
        .filter_map(|row| classify_hid(row.instance_id, row.friendly_name))
        .collect())
}

/// Enumerates USB devices as a shallow root list.
pub fn enumerate_usb_roots() -> CoreResult<Vec<UsbNode>> {
    let rows = unsafe { enumerate_class_rows(&GUID_DEVCLASS_USB) }?;
    Ok(rows
        .into_iter()
        .map(|row| UsbNode {
            device_id: DeviceId::from_instance(row.instance_id),
            description: row.friendly_name,
            port: None,
            location_path: None,
            children: Vec::new(),
        })
        .collect())
}

struct DeviceRow {
    instance_id: String,
    friendly_name: String,
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

        out.push(DeviceRow {
            instance_id,
            friendly_name,
        });
        index += 1;
    }

    let _ = SetupDiDestroyDeviceInfoList(list);
    Ok(out)
}

fn classify_hid(instance: String, name: String) -> Option<InputDeviceInfo> {
    let blob = format!("{name} {instance}").to_ascii_uppercase();
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

    let (vendor_id, product_id) = parse_vid_pid(&instance);
    Some(InputDeviceInfo {
        id: DeviceId::from_instance(instance),
        kind,
        name,
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
