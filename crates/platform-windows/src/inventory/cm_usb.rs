//! USB topology via Configuration Manager + optional hub IOCTL enrichment.

use multiseat_core::{CoreError, CoreResult, DeviceId, UsbNode};
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Child, CM_Get_DevNode_Registry_PropertyW, CM_Get_Device_IDW, CM_Get_Device_ID_Size,
    CM_Get_Sibling, CM_Locate_DevNodeW, CM_DRP_ADDRESS, CM_DRP_DEVICEDESC, CM_DRP_LOCATION_PATHS,
    CM_LOCATE_DEVNODE_NORMAL, CR_SUCCESS,
};

use super::hub_ioctl::{self, HubSnapshot};
use super::setupapi_hid;

/// Builds a forest of USB hubs/devices rooted at USB root hubs.
pub fn build_usb_topology() -> CoreResult<Vec<UsbNode>> {
    unsafe { build_usb_topology_inner() }
}

unsafe fn build_usb_topology_inner() -> CoreResult<Vec<UsbNode>> {
    let flat = setupapi_hid::enumerate_usb_roots()?;
    let mut roots = Vec::new();

    for device in flat {
        let instance = device.device_id.instance_id.clone();
        if !instance.to_ascii_uppercase().contains("ROOT_HUB") {
            continue;
        }
        match build_subtree_from_instance(&instance) {
            Ok(node) => roots.push(node),
            Err(err) => {
                tracing::warn!(%instance, error = %err, "CM USB walk failed; using shallow node");
                roots.push(device);
            }
        }
    }

    if roots.is_empty() {
        return setupapi_hid::enumerate_usb_roots();
    }

    match hub_ioctl::enumerate_hub_ports() {
        Ok(hubs) => {
            tracing::info!(hubs = hubs.len(), "hub IOCTL snapshots collected");
            for root in &mut roots {
                enrich_node(root, &hubs);
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "hub IOCTL enumeration failed; keeping CM ports");
        }
    }

    Ok(roots)
}

unsafe fn build_subtree_from_instance(instance_id: &str) -> CoreResult<UsbNode> {
    let wide = to_wide(instance_id);
    let mut devinst = 0u32;
    let status = CM_Locate_DevNodeW(
        &mut devinst,
        PCWSTR(wide.as_ptr()),
        CM_LOCATE_DEVNODE_NORMAL,
    );
    if status != CR_SUCCESS {
        return Err(CoreError::Backend(format!(
            "CM_Locate_DevNodeW({instance_id}) failed: {status:?}"
        )));
    }
    build_subtree(devinst, None)
}

unsafe fn build_subtree(devinst: u32, port: Option<u32>) -> CoreResult<UsbNode> {
    let instance_id = device_id(devinst)?;
    let description = device_description(devinst).unwrap_or_else(|| instance_id.clone());
    let location_path = device_location_path(devinst);

    let mut children = Vec::new();
    let mut child = 0u32;
    if CM_Get_Child(&mut child, devinst, 0) == CR_SUCCESS {
        let mut current = child;
        let mut index = 1u32;
        loop {
            let child_port = device_address(current).or(Some(index));
            children.push(build_subtree(current, child_port)?);
            let mut sibling = 0u32;
            if CM_Get_Sibling(&mut sibling, current, 0) != CR_SUCCESS {
                break;
            }
            current = sibling;
            index += 1;
        }
    }

    Ok(UsbNode {
        device_id: DeviceId::from_instance(instance_id),
        description,
        port,
        location_path,
        children,
    })
}

fn enrich_node(node: &mut UsbNode, hubs: &[HubSnapshot]) {
    if let Some(hub) = hub_ioctl::find_hub(hubs, &node.device_id.instance_id) {
        if node.device_id.interface_path.is_none() {
            node.device_id.interface_path = Some(hub.interface_path.clone());
        }
        tracing::debug!(
            hub = %hub.instance_id,
            ports = hub.port_count,
            connected = hub.ports.values().filter(|p| p.connected).count(),
            "enriching CM USB node from hub IOCTL"
        );
        let by_addr = hub_ioctl::address_to_port(hub);
        for child in &mut node.children {
            refine_child_port(child, hub, &by_addr);
            enrich_node(child, hubs);
        }
    } else {
        for child in &mut node.children {
            enrich_node(child, hubs);
        }
    }
}

fn refine_child_port(
    child: &mut UsbNode,
    hub: &HubSnapshot,
    by_addr: &std::collections::HashMap<u16, u32>,
) {
    // Prefer an explicit CM port that IOCTL also reports as connected.
    if let Some(port) = child.port {
        if let Some(info) = hub.ports.get(&port).filter(|p| p.connected) {
            if info.is_hub {
                tracing::trace!(port, speed = info.speed, "child is downstream hub");
            }
            return;
        }
    }

    // Otherwise map by USB device address when CM exposed it as the address field.
    if let Some(port) = child
        .port
        .and_then(|addr| by_addr.get(&(addr as u16)).copied())
    {
        if let Some(info) = hub.ports.get(&port) {
            tracing::trace!(
                old = ?child.port,
                new = port,
                speed = info.speed,
                "refined USB port via hub IOCTL address map"
            );
        }
        child.port = Some(port);
    }
}

unsafe fn device_id(devinst: u32) -> CoreResult<String> {
    let mut len = 0u32;
    if CM_Get_Device_ID_Size(&mut len, devinst, 0) != CR_SUCCESS || len == 0 {
        return Err(CoreError::Backend("CM_Get_Device_ID_Size failed".into()));
    }
    let mut buf = vec![0u16; len as usize];
    if CM_Get_Device_IDW(devinst, &mut buf, 0) != CR_SUCCESS {
        return Err(CoreError::Backend("CM_Get_Device_IDW failed".into()));
    }
    Ok(utf16_z(&buf))
}

unsafe fn device_description(devinst: u32) -> Option<String> {
    registry_string(devinst, CM_DRP_DEVICEDESC)
}

unsafe fn device_location_path(devinst: u32) -> Option<String> {
    let multi = registry_bytes(devinst, CM_DRP_LOCATION_PATHS)?;
    let words: Vec<u16> = multi
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    // REG_MULTI_SZ: take the first non-empty path.
    let mut start = 0usize;
    while start < words.len() {
        if let Some(rel) = words[start..].iter().position(|&c| c == 0) {
            if rel == 0 {
                break;
            }
            let path = String::from_utf16_lossy(&words[start..start + rel]);
            if !path.is_empty() {
                return Some(path);
            }
            start += rel + 1;
        } else {
            break;
        }
    }
    None
}

unsafe fn registry_string(devinst: u32, prop: u32) -> Option<String> {
    let bytes = registry_bytes(devinst, prop)?;
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(utf16_z(&words))
}

unsafe fn registry_bytes(devinst: u32, prop: u32) -> Option<Vec<u8>> {
    let mut ty = 0u32;
    let mut len = 0u32;
    let status = CM_Get_DevNode_Registry_PropertyW(devinst, prop, Some(&mut ty), None, &mut len, 0);
    if status != CR_SUCCESS || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    let status = CM_Get_DevNode_Registry_PropertyW(
        devinst,
        prop,
        Some(&mut ty),
        Some(buf.as_mut_ptr().cast()),
        &mut len,
        0,
    );
    if status != CR_SUCCESS {
        return None;
    }
    Some(buf)
}

unsafe fn device_address(devinst: u32) -> Option<u32> {
    let mut ty = 0u32;
    let mut value = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    let status = CM_Get_DevNode_Registry_PropertyW(
        devinst,
        CM_DRP_ADDRESS,
        Some(&mut ty),
        Some((&mut value as *mut u32).cast()),
        &mut len,
        0,
    );
    if status != CR_SUCCESS {
        return None;
    }
    // For USB, the port number is commonly in the low byte.
    Some(value & 0xff)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn utf16_z(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
