#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, HKL, KLF_ACTIVATE,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, INPUTLANGCHANGE_FORWARD,
    WM_INPUTLANGCHANGEREQUEST,
};

#[cfg(windows)]
static PREVIOUS_LAYOUT: OnceLock<Mutex<Option<isize>>> = OnceLock::new();

#[cfg(windows)]
fn previous_layout() -> &'static Mutex<Option<isize>> {
    PREVIOUS_LAYOUT.get_or_init(|| Mutex::new(None))
}

#[cfg(windows)]
fn is_english_layout(layout: HKL) -> bool {
    const PRIMARY_LANGUAGE_MASK: usize = 0x03ff;
    const LANG_ENGLISH: usize = 0x0009;
    (layout.0 as usize & PRIMARY_LANGUAGE_MASK) == LANG_ENGLISH
}

#[cfg(windows)]
fn find_english_layout() -> Option<HKL> {
    let count = unsafe { GetKeyboardLayoutList(None) };
    if count <= 0 {
        return None;
    }
    let mut layouts = vec![HKL(std::ptr::null_mut()); count as usize];
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    if count <= 0 {
        return None;
    }
    layouts
        .into_iter()
        .take(count as usize)
        .find(|layout| !layout.is_invalid() && is_english_layout(*layout))
}

#[cfg(windows)]
pub fn switch_to_english() {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return;
    }
    let thread = unsafe { GetWindowThreadProcessId(foreground, None) };
    if thread == 0 {
        return;
    }
    let current = unsafe { GetKeyboardLayout(thread) };
    if current.is_invalid() || is_english_layout(current) {
        return;
    }
    let Some(target) = find_english_layout() else {
        return;
    };
    if target == current {
        return;
    }
    if unsafe { ActivateKeyboardLayout(target, KLF_ACTIVATE) }.is_ok() {
        if let Ok(mut saved) = previous_layout().lock() {
            *saved = Some(current.0 as isize);
        }
    }
}

#[cfg(windows)]
pub fn restore_previous() {
    let previous = previous_layout()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take());
    let Some(previous) = previous else {
        return;
    };
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return;
    }
    let _ = unsafe {
        PostMessageW(
            Some(foreground),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(INPUTLANGCHANGE_FORWARD as usize),
            LPARAM(previous),
        )
    };
}

#[cfg(not(windows))]
pub fn switch_to_english() {}

#[cfg(not(windows))]
pub fn restore_previous() {}
