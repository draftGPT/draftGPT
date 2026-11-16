//! Keeping the panel next to the League client window.
//!
//! Docked: while a champ select is on, the panel sits against the client's
//! right edge (left edge if there's no room), as tall as the client, and stays
//! on top. Otherwise it's an ordinary window left where the user puts it: when
//! floating is toggled on, outside champ select (so it never sits over the
//! game), when the client is fullscreen or minimized, or when there's no
//! client window. Only Windows can find the client window; elsewhere the panel
//! always floats.
//!
//! The client window is `LeagueClientUx`'s: class `RCLIENT`, title
//! `League of Legends`. The game's window (`RiotWindowClass`) is never looked
//! up: champ select only.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

use crate::engine::Shared;

/// The client window's outer rectangle in physical pixels, and whether it
/// covers its whole monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientWindow {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub fullscreen: bool,
    /// The monitor's work area (left, right edges), to pick a side.
    pub monitor_left: i32,
    pub monitor_right: i32,
}

/// Where the docked panel goes for a client window and a panel width.
pub fn docked_rect(c: &ClientWindow, panel_width: i32) -> (i32, i32, i32, i32) {
    let right = c.x + c.width;
    let x = if right + panel_width <= c.monitor_right || c.x - panel_width < c.monitor_left {
        right
    } else {
        c.x - panel_width
    };
    (x, c.y, panel_width, c.height)
}

const POLL: Duration = Duration::from_millis(300);

/// Follows the client window until the app exits.
pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    tauri::async_runtime::spawn(async move {
        let mut was_docked = false;
        loop {
            tokio::time::sleep(POLL).await;
            let Some(window) = app.get_webview_window("panel") else {
                return;
            };
            let target = if shared.floating() || !shared.has_draft() {
                None
            } else {
                find_client().filter(|c| !c.fullscreen)
            };
            let docked = target.is_some();
            if docked != was_docked {
                let _ = window.set_always_on_top(docked);
                was_docked = docked;
            }
            let Some(client) = target else { continue };
            // Positions and the client rectangle are outer (frame included);
            // set_size takes the inner size, so take the frame off.
            let (Ok(outer), Ok(inner)) = (window.outer_size(), window.inner_size()) else {
                continue;
            };
            let frame_w = outer.width.saturating_sub(inner.width);
            let frame_h = outer.height.saturating_sub(inner.height);
            let width = i32::try_from(outer.width).unwrap_or(380);
            let (x, y, w, h) = docked_rect(&client, width);
            let pos = PhysicalPosition::new(x, y);
            if window.outer_position().ok() != Some(pos) {
                let _ = window.set_position(pos);
            }
            let want = PhysicalSize::new(w.unsigned_abs(), h.unsigned_abs());
            if outer != want {
                let inner_want = PhysicalSize::new(
                    want.width.saturating_sub(frame_w),
                    want.height.saturating_sub(frame_h),
                );
                let _ = window.set_size(inner_want);
            }
        }
    });
}

#[cfg(windows)]
use self::windows::find_client;

#[cfg(not(windows))]
fn find_client() -> Option<ClientWindow> {
    None
}

#[cfg(windows)]
#[allow(unsafe_code)] // Win32 calls; each is a plain query on a window handle.
mod windows {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, IsIconic, IsWindowVisible,
    };
    use windows::core::w;

    use super::ClientWindow;

    /// The League client's window (class `RCLIENT`, title `League of
    /// Legends`), unless it's hidden or minimized. Never the game window.
    pub fn find_client() -> Option<ClientWindow> {
        // SAFETY: FindWindowW takes two valid, NUL-terminated wide strings.
        let hwnd = unsafe { FindWindowW(w!("RCLIENT"), w!("League of Legends")) }.ok()?;
        // SAFETY: plain queries on a window handle we just got; a stale
        // handle only makes them fail.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(hwnd, &raw mut r).ok()?;
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
                ..MONITORINFO::default()
            };
            if !GetMonitorInfoW(monitor, &raw mut mi).as_bool() {
                return None;
            }
            let m = mi.rcMonitor;
            Some(ClientWindow {
                x: r.left,
                y: r.top,
                width: r.right - r.left,
                height: r.bottom - r.top,
                fullscreen: r.left <= m.left
                    && r.top <= m.top
                    && r.right >= m.right
                    && r.bottom >= m.bottom,
                monitor_left: mi.rcWork.left,
                monitor_right: mi.rcWork.right,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(x: i32, width: i32) -> ClientWindow {
        ClientWindow {
            x,
            y: 100,
            width,
            height: 720,
            fullscreen: false,
            monitor_left: 0,
            monitor_right: 1920,
        }
    }

    #[test]
    fn docks_right_of_the_client_when_there_is_room() {
        assert_eq!(docked_rect(&client(200, 1280), 380), (1480, 100, 380, 720));
    }

    #[test]
    fn docks_left_when_the_right_side_is_full() {
        assert_eq!(docked_rect(&client(600, 1280), 380), (220, 100, 380, 720));
    }

    #[test]
    fn stays_right_when_neither_side_fits() {
        assert_eq!(docked_rect(&client(100, 1700), 380), (1800, 100, 380, 720));
    }
}
