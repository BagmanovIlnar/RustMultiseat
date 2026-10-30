//! DXGI adapter/output enumeration (`IDXGIFactory1`).

use multiseat_core::{CoreError, CoreResult, DisplayId, DisplayInfo, GpuInfo};
use windows::Win32::Graphics::Dxgi::CreateDXGIFactory1;
use windows::Win32::Graphics::Dxgi::IDXGIFactory1;

/// DXGI software adapter flag (`DXGI_ADAPTER_FLAG_SOFTWARE`).
const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 2;

/// Enumerates GPUs and attached outputs via DXGI 1.1.
pub fn enumerate_gpus_and_displays() -> CoreResult<(Vec<GpuInfo>, Vec<DisplayInfo>)> {
    unsafe { enumerate_gpus_and_displays_inner() }
}

unsafe fn enumerate_gpus_and_displays_inner() -> CoreResult<(Vec<GpuInfo>, Vec<DisplayInfo>)> {
    let factory: IDXGIFactory1 = CreateDXGIFactory1()
        .map_err(|e| CoreError::Backend(format!("CreateDXGIFactory1 failed: {e}")))?;

    let mut gpus = Vec::new();
    let mut displays = Vec::new();
    let mut adapter_index = 0u32;

    loop {
        let adapter = match factory.EnumAdapters1(adapter_index) {
            Ok(adapter) => adapter,
            Err(_) => break,
        };

        let desc = adapter
            .GetDesc1()
            .map_err(|e| CoreError::Backend(format!("IDXGIAdapter1::GetDesc1 failed: {e}")))?;

        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE != 0 {
            adapter_index += 1;
            continue;
        }

        let description = wchar_to_string(&desc.Description);
        // Prefer AdapterLuid over EnumAdapters ordinal — ordinals shift when
        // adapters appear/disappear; LUID stays stable for the adapter lifetime.
        let luid = desc.AdapterLuid;
        let gpu_id = format!(
            "pci:{:04x}:{:04x}:luid:{:08x}-{:08x}",
            desc.VendorId, desc.DeviceId, luid.HighPart as u32, luid.LowPart
        );

        gpus.push(GpuInfo {
            id: gpu_id.clone(),
            description,
            vendor_id: Some(desc.VendorId),
            device_id: Some(desc.DeviceId),
        });

        let mut output_index = 0u32;
        loop {
            let output = match adapter.EnumOutputs(output_index) {
                Ok(output) => output,
                Err(_) => break,
            };

            let out_desc = output
                .GetDesc()
                .map_err(|e| CoreError::Backend(format!("IDXGIOutput::GetDesc failed: {e}")))?;

            let name = wchar_to_string(&out_desc.DeviceName);
            let width = (out_desc.DesktopCoordinates.right - out_desc.DesktopCoordinates.left)
                .max(0) as u32;
            let height = (out_desc.DesktopCoordinates.bottom - out_desc.DesktopCoordinates.top)
                .max(0) as u32;

            displays.push(DisplayInfo {
                id: DisplayId(name),
                gpu_id: gpu_id.clone(),
                width: Some(width),
                height: Some(height),
                attached: out_desc.AttachedToDesktop.as_bool(),
                virtual_display: false,
            });

            output_index += 1;
        }

        adapter_index += 1;
    }

    Ok((gpus, displays))
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
