//! Windows keyboard glue for the hold-to-quit gauge: a keyboard hook on the UI thread and the system key repeat
//! settings.
//!
//! The browser windows of CEF (page child windows) and the GPUI windows all live on the UI thread of the process, so
//! one `WH_KEYBOARD` hook sees every key the application receives, whichever window holds the keyboard focus, before
//! the window procedures do.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, SystemParametersInfoW, HC_ACTION, SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, WH_KEYBOARD,
};

/// A key message seen by the hook.
#[derive(Clone, Copy, Debug)]
pub struct WinKey {
    /// Virtual key code (`VK_*`).
    pub vk: u32,
    /// Key release.
    pub up: bool,
    /// The key was already down (previous state bit of `lParam`).
    pub repeat: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

static HANDLER: OnceLock<fn(WinKey) -> bool> = OnceLock::new();

fn down(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(handler) = HANDLER.get()
    {
        let l = lparam as usize;
        let key = WinKey {
            vk: wparam as u32,
            up: l & (1 << 31) != 0,
            repeat: l & (1 << 30) != 0 && l & (1 << 31) == 0,
            ctrl: down(VK_CONTROL),
            shift: down(VK_SHIFT),
            alt: down(VK_MENU),
            meta: down(VK_LWIN) || down(VK_RWIN),
        };
        // A non-zero result keeps the message from the window.
        if handler(key) {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Installs the keyboard hook of the calling (UI) thread. The handler returns true to swallow the key. Once per process.
pub fn install_key_hook(handler: fn(WinKey) -> bool) {
    if HANDLER.set(handler).is_err() {
        return;
    }
    let h = unsafe { SetWindowsHookExW(WH_KEYBOARD, Some(hook), std::ptr::null_mut(), GetCurrentThreadId()) };
    if h.is_null() {
        tracing::warn!("keyboard hook not installed");
    }
}

/// System key repeat settings: (`SPI_GETKEYBOARDDELAY` 0..3, `SPI_GETKEYBOARDSPEED` 0..31).
pub fn repeat_settings() -> Option<(u32, u32)> {
    let mut delay: u32 = 0;
    let mut speed: u32 = 0;
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETKEYBOARDDELAY, 0, &mut delay as *mut u32 as *mut _, 0) != 0
            && SystemParametersInfoW(SPI_GETKEYBOARDSPEED, 0, &mut speed as *mut u32 as *mut _, 0) != 0
    };
    ok.then_some((delay, speed))
}
