//! Windows `CreateFile` / `DeviceIoControl` for the input-filter control device.
//!
//! Compiled only on Windows. Until the KMDF INF installs the device, open fails
//! and the caller treats the filter as absent (fail-open).

#![cfg(windows)]

use std::mem::MaybeUninit;
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

use super::filter_control::{InputFilterError, INPUT_FILTER_CONTROL_DEVICE};
use super::ioctl::{
    RmsIfFailOpenReq, RmsIfRouteTable, RmsIfStats, RmsIfVersionInfo, IOCTL_RMS_IF_GET_VERSION,
    IOCTL_RMS_IF_QUERY_STATS, IOCTL_RMS_IF_SET_FAIL_OPEN, IOCTL_RMS_IF_SET_ROUTES,
    RMS_IF_ABI_VERSION,
};

/// Opened control device handle.
pub struct NativeFilterDevice {
    handle: HANDLE,
}

impl std::fmt::Debug for NativeFilterDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFilterDevice")
            .field("handle", &self.handle.0)
            .finish()
    }
}

impl NativeFilterDevice {
    /// Opens `\\.\RustMultiseatInputFilter`.
    pub fn open() -> Result<Self, InputFilterError> {
        let wide: Vec<u16> = INPUT_FILTER_CONTROL_DEVICE
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: path is NUL-terminated; share mode allows concurrent daemon/ctl.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                (GENERIC_READ.0 | GENERIC_WRITE.0) as u32,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|e| InputFilterError::Io(e.to_string()))?;

        if handle.is_invalid() {
            return Err(InputFilterError::NotPresent);
        }
        Ok(Self { handle })
    }

    fn ioctl_out<O>(&self, code: u32) -> Result<O, InputFilterError>
    where
        O: Copy,
    {
        let mut out = MaybeUninit::<O>::uninit();
        let mut returned = 0u32;
        // SAFETY: buffered IOCTL; sizes match C ABI structs.
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                Some(ptr::null()),
                0,
                Some(out.as_mut_ptr() as *mut _),
                std::mem::size_of::<O>() as u32,
                Some(&mut returned),
                None,
            )
        };
        if ok.is_err() {
            return Err(InputFilterError::Io(format!(
                "DeviceIoControl 0x{code:08X} failed"
            )));
        }
        if returned as usize != std::mem::size_of::<O>() {
            return Err(InputFilterError::Io(format!(
                "DeviceIoControl 0x{code:08X} returned {returned} bytes, expected {}",
                std::mem::size_of::<O>()
            )));
        }
        // SAFETY: driver wrote a full O.
        Ok(unsafe { out.assume_init() })
    }

    fn ioctl_in_only<I>(&self, code: u32, input: &I) -> Result<(), InputFilterError> {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                Some(input as *const I as *const _),
                std::mem::size_of::<I>() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        };
        if ok.is_err() {
            return Err(InputFilterError::Io(format!(
                "DeviceIoControl 0x{code:08X} failed"
            )));
        }
        Ok(())
    }

    /// `IOCTL_RMS_IF_GET_VERSION`
    pub fn get_version(&self) -> Result<RmsIfVersionInfo, InputFilterError> {
        let info: RmsIfVersionInfo = self.ioctl_out(IOCTL_RMS_IF_GET_VERSION)?;
        if info.abi_version != RMS_IF_ABI_VERSION {
            return Err(InputFilterError::IncompatibleVersion {
                driver: info.abi_version,
                expected: RMS_IF_ABI_VERSION,
            });
        }
        Ok(info)
    }

    /// `IOCTL_RMS_IF_SET_FAIL_OPEN`
    pub fn set_fail_open(&self, enabled: bool) -> Result<(), InputFilterError> {
        let req = RmsIfFailOpenReq {
            enabled: u32::from(enabled),
        };
        self.ioctl_in_only(IOCTL_RMS_IF_SET_FAIL_OPEN, &req)
    }

    /// `IOCTL_RMS_IF_SET_ROUTES`
    pub fn set_routes(&self, table: &RmsIfRouteTable) -> Result<(), InputFilterError> {
        table
            .validate()
            .map_err(|m| InputFilterError::Io(m.into()))?;
        self.ioctl_in_only(IOCTL_RMS_IF_SET_ROUTES, table)
    }

    /// `IOCTL_RMS_IF_QUERY_STATS`
    pub fn query_stats(&self) -> Result<RmsIfStats, InputFilterError> {
        self.ioctl_out(IOCTL_RMS_IF_QUERY_STATS)
    }
}

impl Drop for NativeFilterDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
