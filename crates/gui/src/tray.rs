//! System tray bridge for the Iced dashboard.
//!
//! Windows: `tray-icon` / `muda` types are `!Send`, so the icon is owned by a
//! dedicated OS thread. That thread forwards Show/Exit into a channel; an Iced
//! [`Subscription`] only reads the channel (no second Win32 event loop, no
//! `!Send` values across `.await`).
//!
//! Non-Windows: tray is unavailable; the window exits on close as usual.

use iced::Subscription;

/// Commands produced by the tray icon / context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Show and focus the main window.
    Show,
    /// Quit the GUI process.
    Exit,
}

/// Whether this build can host a tray icon (Windows today).
pub fn is_available() -> bool {
    cfg!(windows)
}

/// Stable menu ids used when matching [`tray_icon::menu::MenuEvent`].
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_SHOW: &str = "multiseat.show";
/// Exit menu id.
#[cfg_attr(not(windows), allow(dead_code))]
pub const MENU_EXIT: &str = "multiseat.exit";

/// Subscription that receives tray commands for the app lifetime.
pub fn subscription(enabled: bool) -> Subscription<TrayCommand> {
    if !(enabled && is_available()) {
        return Subscription::none();
    }
    #[cfg(windows)]
    {
        Subscription::run(windows_tray_worker)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Subscription::none()
    }
}

#[cfg(windows)]
fn windows_tray_worker() -> impl iced::futures::Stream<Item = TrayCommand> {
    use iced::futures::SinkExt;
    use std::sync::mpsc;

    iced::stream::channel(8, async |mut output| {
        let (tx, rx) = mpsc::sync_channel::<TrayCommand>(32);
        let spawn = std::thread::Builder::new()
            .name("multiseat-tray".into())
            .spawn(move || tray_thread_main(tx));
        if spawn.is_err() {
            return;
        }

        loop {
            while let Ok(cmd) = rx.try_recv() {
                let _ = output.send(cmd).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
}

/// Owns `!Send` tray objects for the process lifetime.
#[cfg(windows)]
fn tray_thread_main(tx: std::sync::mpsc::SyncSender<TrayCommand>) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, TrayIconBuilder, TrayIconEvent};

    let Ok(icon) = app_icon() else {
        return;
    };

    let show = MenuItem::with_id(MENU_SHOW, "Show RustMultiseat", true, None);
    let exit = MenuItem::with_id(MENU_EXIT, "Exit", true, None);
    let menu = Menu::new();
    if menu
        .append_items(&[&show, &PredefinedMenuItem::separator(), &exit])
        .is_err()
    {
        return;
    }

    let Ok(tray) = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("RustMultiseat")
        .with_icon(icon)
        .build()
    else {
        return;
    };

    let tx_menu = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id: &str = event.id.as_ref();
        let cmd = match id {
            MENU_SHOW => Some(TrayCommand::Show),
            MENU_EXIT => Some(TrayCommand::Exit),
            _ => None,
        };
        if let Some(cmd) = cmd {
            let _ = tx_menu.send(cmd);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } = event
        {
            let _ = tx.send(TrayCommand::Show);
        }
    }));

    // Keep `!Send` tray/menu items alive; Windows tray callbacks arrive on
    // tray-icon's internal service thread and hop into `tx`.
    let _keep = (tray, show, exit);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

#[cfg(windows)]
fn app_icon() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * SIZE + x) * 4) as usize;
            let edge = x < 2 || y < 2 || x >= SIZE - 2 || y >= SIZE - 2;
            if edge {
                rgba[i] = 0x1a;
                rgba[i + 1] = 0x3a;
                rgba[i + 2] = 0x5c;
                rgba[i + 3] = 0xff;
            } else {
                rgba[i] = 0x3d;
                rgba[i + 1] = 0x8b;
                rgba[i + 2] = 0xd4;
                rgba[i + 3] = 0xff;
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_matches_cfg() {
        assert_eq!(is_available(), cfg!(windows));
    }

    #[test]
    fn menu_ids_are_stable() {
        assert_eq!(MENU_SHOW, "multiseat.show");
        assert_eq!(MENU_EXIT, "multiseat.exit");
    }
}
