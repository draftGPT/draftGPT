//! Keeping the panel next to the League client window.
//!
//! The panel is hidden outside champ select and shows itself when a draft
//! starts. Docked: it sits against the client's right edge (left edge if
//! there's no room), as tall as the client. It's an ordinary window left where
//! the user puts it when floating is toggled on, when the client is fullscreen,
//! or when there's no client window. Only Windows can find the client window;
//! elsewhere the panel always floats.
//!
//! On Windows, docked means *owned* by the client window (only our window's
//! owner field changes): Windows keeps an owned window directly above its
//! owner, so the panel is never behind the client nor above other apps, and
//! it minimizes and restores with the client. It follows the client through a
//! location-change event hook on the client's process, moving in the same
//! frame instead of catching up on the next poll. Windows destroys owned
//! windows with their owner, so the owner is dropped as soon as the client
//! window hides or goes away. Rectangles are the *visible* bounds (DWM
//! extended frame): Windows 10 windows carry invisible resize borders that
//! would otherwise leave a gap and a shorter panel.
//!
//! The client window is `LeagueClientUx`'s: class `RCLIENT`, title
//! `League of Legends`. The game's window (`RiotWindowClass`) is never looked
//! up: champ select only.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::engine::Shared;

/// The client window's visible rectangle in physical pixels, and whether it
/// covers its whole monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows tracker uses it
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

/// Where the docked panel's visible rectangle goes for a client window and a
/// panel width.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn docked_rect(c: &ClientWindow, panel_width: i32) -> (i32, i32, i32, i32) {
    let right = c.x + c.width;
    let x = if right + panel_width <= c.monitor_right || c.x - panel_width < c.monitor_left {
        right
    } else {
        c.x - panel_width
    };
    (x, c.y, panel_width, c.height)
}

/// Show/hide, floating changes and a client window appearing are picked up
/// this often. Following a moving client doesn't wait for it (Windows hook).
const POLL: Duration = Duration::from_millis(250);

/// Shows, hides and docks the panel until the app exits.
pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    tauri::async_runtime::spawn(async move {
        let mut was_shown = false;
        loop {
            tokio::time::sleep(POLL).await;
            let Some(window) = app.get_webview_window("panel") else {
                return;
            };
            let draft = shared.has_draft();
            // Docks (and places the panel) before it's shown, so it appears in place.
            follow(&app, &window, draft && !shared.floating());
            if draft != was_shown {
                let _ = if draft { window.show() } else { window.hide() };
                was_shown = draft;
            }
        }
    });
}

#[cfg(windows)]
use self::windows::follow;

/// Elsewhere there's no client window to find: the panel always floats.
#[cfg(not(windows))]
fn follow(_app: &AppHandle, _window: &tauri::WebviewWindow, _dock: bool) {}

#[cfg(windows)]
#[allow(unsafe_code)] // Win32 calls on window handles; see the SAFETY notes.
mod windows {
    use std::sync::Mutex;

    use tauri::{AppHandle, WebviewWindow};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        CHILDID_SELF, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE,
        FindWindowW, GWLP_HWNDPARENT, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos, WINEVENT_OUTOFCONTEXT,
    };
    use windows::core::w;

    use super::{ClientWindow, docked_rect};

    /// What the event hooks need; handles are kept as integers (HWND isn't Send).
    struct State {
        /// The panel's window, set while it's docked.
        panel: Option<isize>,
        /// The client process the hooks watch, and the hooks.
        hooks: Option<(u32, [isize; 2])>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        panel: None,
        hooks: None,
    });

    fn lock() -> std::sync::MutexGuard<'static, State> {
        STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut _)
    }

    /// Called from the poll loop: docks the panel (owner + place) when docking
    /// is wanted and the client window is there, undocks it otherwise, and
    /// keeps the event hooks on the current client process.
    pub fn follow(app: &AppHandle, window: &WebviewWindow, dock: bool) {
        let Some(panel) = window.hwnd().ok().map(|h| HWND(h.0)) else {
            return;
        };
        // A minimized client keeps the panel owned: it's hidden with the client.
        let client = find_client().filter(|(_, c)| !c.is_some_and(|c| c.fullscreen));
        match client.filter(|_| dock) {
            Some((client_hwnd, rect)) => {
                set_owner(panel, client_hwnd);
                if let Some(c) = rect {
                    place(panel, &c);
                }
                lock().panel = Some(panel.0 as isize);
                watch(app, client_hwnd);
            }
            None => {
                lock().panel = None;
                set_owner(panel, HWND::default());
            }
        }
    }

    /// Sets the panel's owner window (none with a null handle).
    fn set_owner(panel: HWND, owner: HWND) {
        // SAFETY: reads and writes our own window's owner field only.
        unsafe {
            if GetWindowLongPtrW(panel, GWLP_HWNDPARENT) != owner.0 as isize {
                SetWindowLongPtrW(panel, GWLP_HWNDPARENT, owner.0 as isize);
            }
        }
    }

    /// (Re)installs the hooks for the client's process. Their callback runs on
    /// the thread that installed them, which must pump messages: Tauri's main
    /// thread.
    fn watch(app: &AppHandle, client: HWND) {
        let mut pid = 0;
        // SAFETY: plain query on a window handle; pid is a valid out pointer.
        unsafe { GetWindowThreadProcessId(client, Some(&raw mut pid)) };
        if pid == 0 || lock().hooks.is_some_and(|(p, _)| p == pid) {
            return;
        }
        let _ = app.run_on_main_thread(move || {
            let mut state = lock();
            if state.hooks.is_some_and(|(p, _)| p == pid) {
                return;
            }
            if let Some((_, old)) = state.hooks.take() {
                for h in old {
                    // SAFETY: hooks this thread installed and hasn't removed.
                    let _ = unsafe { UnhookWinEvent(HWINEVENTHOOK(h as *mut _)) };
                }
            }
            // Moves, and the client window hiding or going away (two ranges so
            // the events in between aren't delivered).
            let ranges = [
                (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
                (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
            ];
            let hooks = ranges.map(|(from, to)| {
                // SAFETY: out-of-context hook with a valid callback; it's
                // delivered through this (main) thread's message loop.
                let h = unsafe {
                    SetWinEventHook(
                        from,
                        to,
                        None,
                        Some(on_event),
                        pid,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                h.0 as isize
            });
            state.hooks = Some((pid, hooks));
        });
    }

    /// An event from the client's process.
    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        event: u32,
        window: HWND,
        object: i32,
        child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
            return;
        }
        let Some(panel) = lock().panel.map(hwnd) else {
            return;
        };
        // SAFETY: a plain query; a stale handle only makes it fail.
        let owner = unsafe { GetWindowLongPtrW(panel, GWLP_HWNDPARENT) };
        if window.0 as isize != owner {
            return;
        }
        if event == EVENT_OBJECT_LOCATIONCHANGE {
            if let Some((_, Some(c))) = find_client()
                && !c.fullscreen
            {
                place(panel, &c);
            }
        } else {
            // Hidden or destroyed: let go before Windows takes the panel with it.
            // The poll loop docks again when the client window is back.
            lock().panel = None;
            set_owner(panel, HWND::default());
        }
    }

    /// The visible bounds (without invisible resize borders), falling back to
    /// the window rectangle.
    fn visible_rect(h: HWND) -> Option<RECT> {
        let mut r = RECT::default();
        // SAFETY: r is a valid RECT and the size passed matches it.
        let ok = unsafe {
            DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&raw mut r).cast(),
                u32::try_from(std::mem::size_of::<RECT>()).ok()?,
            )
        }
        .is_ok();
        if ok { Some(r) } else { window_rect(h) }
    }

    fn window_rect(h: HWND) -> Option<RECT> {
        let mut r = RECT::default();
        // SAFETY: plain query; a stale handle only makes it fail.
        unsafe { GetWindowRect(h, &raw mut r) }.ok()?;
        Some(r)
    }

    /// Moves and sizes the panel so its visible rectangle sits against the
    /// client, in one call (no separate move and resize).
    fn place(panel: HWND, c: &ClientWindow) {
        let (Some(win), Some(vis)) = (window_rect(panel), visible_rect(panel)) else {
            return;
        };
        // Invisible borders of the panel, added back around the target.
        let (left, top) = (vis.left - win.left, vis.top - win.top);
        let (right, bottom) = (win.right - vis.right, win.bottom - vis.bottom);
        let (x, y, w, h) = docked_rect(c, vis.right - vis.left);
        let target = RECT {
            left: x - left,
            top: y - top,
            right: x + w + right,
            bottom: y + h + bottom,
        };
        if target == win {
            return;
        }
        // SAFETY: our own window handle; no z-order or activation change.
        let _ = unsafe {
            SetWindowPos(
                panel,
                None,
                target.left,
                target.top,
                target.right - target.left,
                target.bottom - target.top,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
            )
        };
    }

    /// The League client's window (class `RCLIENT`, title `League of
    /// Legends`) unless it's hidden, with its rectangle unless it's minimized.
    /// Never the game window.
    fn find_client() -> Option<(HWND, Option<ClientWindow>)> {
        // SAFETY: FindWindowW takes two valid, NUL-terminated wide strings.
        let h = unsafe { FindWindowW(w!("RCLIENT"), w!("League of Legends")) }.ok()?;
        // SAFETY: plain queries on a window handle we just got; a stale
        // handle only makes them fail.
        unsafe {
            if !IsWindowVisible(h).as_bool() {
                return None;
            }
            if IsIconic(h).as_bool() {
                return Some((h, None));
            }
        }
        let r = visible_rect(h)?;
        // SAFETY: as above; mi is a valid MONITORINFO with cbSize set.
        let mi = unsafe {
            let monitor = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
                ..MONITORINFO::default()
            };
            if !GetMonitorInfoW(monitor, &raw mut mi).as_bool() {
                return None;
            }
            mi
        };
        let m = mi.rcMonitor;
        Some((
            h,
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
            }),
        ))
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
