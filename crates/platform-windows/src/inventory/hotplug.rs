//! Device hot-plug watcher: message-only window + debounced invalidation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, PostMessageW, RegisterClassExW, RegisterDeviceNotificationW,
    SetWindowLongPtrW, UnregisterDeviceNotification, CREATESTRUCTW, DBT_DEVTYP_DEVICEINTERFACE,
    DEVICE_NOTIFY_ALL_INTERFACE_CLASSES, DEVICE_NOTIFY_WINDOW_HANDLE, DEV_BROADCAST_DEVICEINTERFACE_W,
    GWLP_USERDATA, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_CREATE,
    WM_DESTROY, WM_DEVICECHANGE, WNDCLASSEXW, HDEVNOTIFY,
};

use crate::debounce::DebounceGate;

struct Shared {
    gate: Arc<DebounceGate>,
    /// HWND stored as isize so `Shared` stays `Send + Sync` across crate versions.
    hwnd: Mutex<Option<isize>>,
    stop: AtomicBool,
}

/// Background WM_DEVICECHANGE listener.
pub struct HotplugWatcher {
    shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for HotplugWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotplugWatcher")
            .field("stopped", &self.shared.stop.load(Ordering::SeqCst))
            .finish()
    }
}

impl HotplugWatcher {
    /// Starts a message-only window thread that signals `gate` on device changes.
    pub fn start(gate: Arc<DebounceGate>) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            gate,
            hwnd: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        let shared_thread = Arc::clone(&shared);

        let join = thread::Builder::new()
            .name("multiseat-hotplug".into())
            .spawn(move || {
                if let Err(err) = unsafe { run_message_loop(shared_thread) } {
                    tracing::warn!(error = %err, "hotplug watcher exited with error");
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(Self {
            shared,
            join: Some(join),
        })
    }

    /// Requests shutdown of the watcher thread.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.shared.hwnd.lock() {
            if let Some(raw) = *guard {
                unsafe {
                    let hwnd = HWND(raw as *mut _);
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HotplugWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn run_message_loop(shared: Arc<Shared>) -> Result<(), String> {
    let hinstance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
    let class_name = w!("RustMultiseatHotplugWnd");

    let wnd = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: Default::default(),
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance.into(),
        hIcon: Default::default(),
        hCursor: Default::default(),
        hbrBackground: Default::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: class_name,
        hIconSm: Default::default(),
    };
    let _ = RegisterClassExW(&wnd);

    let shared_ptr = Arc::into_raw(Arc::clone(&shared));
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        w!("RustMultiseatHotplug"),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(hinstance.into()),
        Some(shared_ptr as *const _),
    )
    .map_err(|e| {
        drop(Arc::from_raw(shared_ptr));
        e.to_string()
    })?;

    if let Ok(mut slot) = shared.hwnd.lock() {
        *slot = Some(hwnd.0 as isize);
    }

    let mut filter = DEV_BROADCAST_DEVICEINTERFACE_W {
        dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
        dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
        dbcc_reserved: 0,
        dbcc_classguid: windows::core::GUID::zeroed(),
        dbcc_name: [0],
    };

    let notify: Option<HDEVNOTIFY> = RegisterDeviceNotificationW(
        HANDLE(hwnd.0),
        &mut filter as *mut _ as *const _,
        DEVICE_NOTIFY_WINDOW_HANDLE | DEVICE_NOTIFY_ALL_INTERFACE_CLASSES,
    )
    .ok();

    if notify.is_none() {
        tracing::warn!("RegisterDeviceNotificationW failed; WM_DEVICECHANGE may be limited");
    }

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let _ = DispatchMessageW(&msg);
    }

    if let Some(h) = notify {
        let _ = UnregisterDeviceNotification(h);
    }
    let _ = DestroyWindow(hwnd);
    // Drop the extra Arc cloned into window userdata (released in WM_DESTROY).
    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create = lparam.0 as *const CREATESTRUCTW;
            if !create.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
            }
            return LRESULT(0);
        }
        WM_DEVICECHANGE => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared;
            if !raw.is_null() {
                (*raw).gate.signal();
            }
            return LRESULT(1);
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared;
            if !raw.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Arc::from_raw(raw));
            }
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
