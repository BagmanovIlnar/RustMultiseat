//! USB hub port facts via `IOCTL_USB_GET_NODE_*` on `GUID_DEVINTERFACE_USB_HUB`.

use std::collections::HashMap;

use multiseat_core::{CoreError, CoreResult};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE,
    DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Usb::{
    DeviceConnected, GUID_DEVINTERFACE_USB_HUB, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
    IOCTL_USB_GET_NODE_INFORMATION, USB_NODE_CONNECTION_INFORMATION_EX, USB_NODE_INFORMATION,
};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, GENERIC_READ,
    GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

/// Per-port facts from hub IOCTL.
#[derive(Debug, Clone)]
pub struct HubPortInfo {
    /// True when `ConnectionStatus == DeviceConnected`.
    pub connected: bool,
    /// USB device address on the bus.
    pub device_address: u16,
    /// True when the attached device is itself a hub.
    pub is_hub: bool,
    /// USB speed byte from the connection info.
    pub speed: u8,
}

/// Snapshot of one hub interface.
#[derive(Debug, Clone)]
pub struct HubSnapshot {
    /// Device instance id for the hub.
    pub instance_id: String,
    /// `\\?\` device interface path used for IOCTL.
    pub interface_path: String,
    /// Number of ports reported by the hub descriptor.
    pub port_count: u32,
    /// Port index (1-based) → connection info.
    pub ports: HashMap<u32, HubPortInfo>,
}

/// Enumerates USB hubs and queries each port via hub IOCTL.
///
/// Failures are soft: callers should fall back to CM-only topology.
pub fn enumerate_hub_ports() -> CoreResult<Vec<HubSnapshot>> {
    unsafe { enumerate_hub_ports_inner() }
}

unsafe fn enumerate_hub_ports_inner() -> CoreResult<Vec<HubSnapshot>> {
    let list = SetupDiGetClassDevsW(
        Some(&GUID_DEVINTERFACE_USB_HUB),
        None,
        None,
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    )
    .map_err(|e| CoreError::Backend(format!("SetupDiGetClassDevsW(USB_HUB) failed: {e}")))?;

    let mut hubs = Vec::new();
    let mut index = 0u32;
    loop {
        let mut iface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            InterfaceClassGuid: GUID::zeroed(),
            Flags: 0,
            Reserved: 0,
        };
        if SetupDiEnumDeviceInterfaces(list, None, &GUID_DEVINTERFACE_USB_HUB, index, &mut iface)
            .is_err()
        {
            let err = GetLastError();
            if err == ERROR_NO_MORE_ITEMS {
                break;
            }
            let _ = SetupDiDestroyDeviceInfoList(list);
            return Err(CoreError::Backend(format!(
                "SetupDiEnumDeviceInterfaces failed: {err:?}"
            )));
        }

        match hub_from_interface(list, &iface) {
            Ok(hub) => hubs.push(hub),
            Err(err) => {
                tracing::debug!(error = %err, "skipping USB hub interface");
            }
        }
        index += 1;
    }

    let _ = SetupDiDestroyDeviceInfoList(list);
    Ok(hubs)
}

unsafe fn hub_from_interface(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface: &SP_DEVICE_INTERFACE_DATA,
) -> CoreResult<HubSnapshot> {
    let (path, instance_id) = interface_path_and_instance(list, iface)?;
    let handle = open_hub(&path)?;
    let result = query_hub(handle, instance_id, path);
    let _ = CloseHandle(handle);
    result
}

unsafe fn interface_path_and_instance(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface: &SP_DEVICE_INTERFACE_DATA,
) -> CoreResult<(String, String)> {
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
    (*detail).cbSize = if cfg!(target_pointer_width = "64") {
        8
    } else {
        6
    };

    SetupDiGetDeviceInterfaceDetailW(list, iface, Some(detail), required, None, Some(&mut info))
        .map_err(|e| CoreError::Backend(format!("SetupDiGetDeviceInterfaceDetailW failed: {e}")))?;

    let path = read_device_path(detail);
    let instance_id = device_instance_id(list, &info)?;
    Ok((path, instance_id))
}

unsafe fn read_device_path(detail: *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W) -> String {
    // DevicePath is a flexible WCHAR array immediately after cbSize.
    let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
    let mut len = 0usize;
    while *path_ptr.add(len) != 0 {
        len += 1;
        if len > 1024 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, len))
}

unsafe fn device_instance_id(
    list: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    info: &SP_DEVINFO_DATA,
) -> CoreResult<String> {
    let mut needed = 0u32;
    let probe = SetupDiGetDeviceInstanceIdW(list, info, None, Some(&mut needed));
    if probe.is_err() {
        let err = GetLastError();
        if err != ERROR_INSUFFICIENT_BUFFER {
            return Err(CoreError::Backend(format!(
                "SetupDiGetDeviceInstanceIdW size failed: {err:?}"
            )));
        }
    }
    let mut buf = vec![0u16; needed as usize];
    SetupDiGetDeviceInstanceIdW(list, info, Some(buf.as_mut_slice()), Some(&mut needed))
        .map_err(|e| CoreError::Backend(format!("SetupDiGetDeviceInstanceIdW failed: {e}")))?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..len]))
}

unsafe fn open_hub(path: &str) -> CoreResult<HANDLE> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    CreateFileW(
        PCWSTR(wide.as_ptr()),
        (GENERIC_READ.0 | GENERIC_WRITE.0) as u32,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        None,
    )
    .or_else(|_| {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    })
    .map_err(|e| CoreError::Backend(format!("CreateFileW({path}) failed: {e}")))
}

unsafe fn query_hub(
    handle: HANDLE,
    instance_id: String,
    interface_path: String,
) -> CoreResult<HubSnapshot> {
    let mut node = USB_NODE_INFORMATION::default();
    let mut returned = 0u32;
    DeviceIoControl(
        handle,
        IOCTL_USB_GET_NODE_INFORMATION,
        None,
        0,
        Some((&mut node as *mut USB_NODE_INFORMATION).cast()),
        std::mem::size_of::<USB_NODE_INFORMATION>() as u32,
        Some(&mut returned),
        None,
    )
    .map_err(|e| CoreError::Backend(format!("IOCTL_USB_GET_NODE_INFORMATION failed: {e}")))?;

    let port_count = {
        let hub_info = node.u.HubInformation;
        u32::from(hub_info.HubDescriptor.bNumberOfPorts)
    };
    let mut ports = HashMap::new();
    for port in 1..=port_count.max(1) {
        if let Ok(info) = query_port(handle, port) {
            ports.insert(port, info);
        }
    }

    Ok(HubSnapshot {
        instance_id,
        interface_path,
        port_count,
        ports,
    })
}

unsafe fn query_port(handle: HANDLE, port: u32) -> CoreResult<HubPortInfo> {
    let mut info = USB_NODE_CONNECTION_INFORMATION_EX::default();
    info.ConnectionIndex = port;
    let mut returned = 0u32;
    DeviceIoControl(
        handle,
        IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
        Some((&info as *const USB_NODE_CONNECTION_INFORMATION_EX).cast()),
        std::mem::size_of::<USB_NODE_CONNECTION_INFORMATION_EX>() as u32,
        Some((&mut info as *mut USB_NODE_CONNECTION_INFORMATION_EX).cast()),
        std::mem::size_of::<USB_NODE_CONNECTION_INFORMATION_EX>() as u32,
        Some(&mut returned),
        None,
    )
    .map_err(|e| {
        CoreError::Backend(format!(
            "IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX(port={port}) failed: {e}"
        ))
    })?;

    let status = info.ConnectionStatus;
    let device_address = info.DeviceAddress;
    let is_hub = info.DeviceIsHub;
    let speed = info.Speed;
    Ok(HubPortInfo {
        connected: status == DeviceConnected,
        device_address,
        is_hub,
        speed,
    })
}

/// Looks up a hub snapshot by instance id (case-insensitive).
pub fn find_hub<'a>(hubs: &'a [HubSnapshot], instance_id: &str) -> Option<&'a HubSnapshot> {
    let needle = instance_id.to_ascii_uppercase();
    hubs.iter().find(|h| {
        h.instance_id.eq_ignore_ascii_case(instance_id) || {
            h.instance_id.to_ascii_uppercase().contains(&needle)
                || needle.contains(&h.instance_id.to_ascii_uppercase())
        }
    })
}

/// Index connected IOCTL ports by USB device address for CM cross-check.
pub fn address_to_port(hub: &HubSnapshot) -> HashMap<u16, u32> {
    hub.ports
        .iter()
        .filter(|(_, info)| info.connected)
        .map(|(port, info)| (info.device_address, *port))
        .collect()
}
