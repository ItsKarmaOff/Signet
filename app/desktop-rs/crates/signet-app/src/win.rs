//! Windows glue: the native popup menu (`CreatePopupMenu` + `TrackPopupMenuEx`), synchronous like the macOS one.

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow, TrackPopupMenuEx, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TPM_TOPALIGN, WM_CANCELMODE, WM_NULL,
};

use crate::windows::Entry;

/// Menu text: `&` would turn the next letter into a mnemonic, so it is doubled; NUL ends the string.
pub fn label_text(s: &str) -> Vec<u16> {
    s.chars().take_while(|c| *c != '\0').flat_map(|c| if c == '&' { vec!['&', '&'] } else { vec![c] }).collect::<String>().encode_utf16().chain(std::iter::once(0)).collect()
}

fn build(menu: HMENU, entries: &[Entry], ids: &mut Vec<String>) {
    for e in entries {
        unsafe {
            match e {
                Entry::Sep => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                }
                Entry::Item { label, id, enabled, checked } => {
                    ids.push(id.clone());
                    let mut flags = MF_STRING;
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    // Command ids start at 1 (0 means "nothing chosen").
                    AppendMenuW(menu, flags, ids.len(), label_text(label).as_ptr());
                }
                Entry::Sub { label, items } => {
                    let sub = CreatePopupMenu();
                    build(sub, items, ids);
                    AppendMenuW(menu, MF_STRING | MF_POPUP, sub as usize, label_text(label).as_ptr());
                }
            }
        }
    }
}

/// Shows the menu at (`x`, `y`) in the window's client coordinates (logical pixels), or at the pointer when the
/// position is not finite. Returns the id of the chosen item, None when dismissed.
pub fn popup(hwnd: isize, x: f64, y: f64, entries: &[Entry]) -> Option<String> {
    let hwnd = hwnd as HWND;
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        let mut ids: Vec<String> = Vec::new();
        build(menu, entries, &mut ids);
        let mut pt = POINT { x: 0, y: 0 };
        if x.is_finite() && y.is_finite() {
            let scale = f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0;
            pt = POINT { x: (x * scale).round() as i32, y: (y * scale).round() as i32 };
            ClientToScreen(hwnd, &mut pt);
        } else {
            GetCursorPos(&mut pt);
        }
        // The owner must be the foreground window, or the menu does not close when the user clicks elsewhere.
        SetForegroundWindow(hwnd);
        if let Some(ms) = crate::windows::popup_menu::autoclose() {
            // Self-test: the menu loop is modal, so another thread cancels it (a posted message reaches the loop).
            let target = hwnd as isize;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                PostMessageW(target as HWND, WM_CANCELMODE, 0, 0);
            });
        }
        let cmd = TrackPopupMenuEx(menu, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD, pt.x, pt.y, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        usize::try_from(cmd).ok().filter(|c| *c > 0).and_then(|c| ids.get(c - 1).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ampersands_are_doubled_and_the_text_is_nul_terminated() {
        let w = label_text("Save & close");
        assert_eq!(String::from_utf16_lossy(&w[..w.len() - 1]), "Save && close");
        assert_eq!(w.last(), Some(&0));
        assert_eq!(label_text("a\0b"), vec![b'a' as u16, 0]);
    }
}
