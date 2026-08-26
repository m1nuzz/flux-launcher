#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, HKL, KLF_ACTIVATE,
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

/// Switch the Flux UI thread to an English HKL without touching the foreground
/// application's thread. `GetKeyboardLayout(0)` and `ActivateKeyboardLayout`
/// intentionally use the same calling thread, so the saved layout is always the
/// one that will later be restored.
#[cfg(windows)]
pub fn switch_to_english() {
    let current = unsafe { GetKeyboardLayout(0) };
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

/// Restore the layout on the same Flux UI thread that was switched on show.
/// Posting a language-change request to `GetForegroundWindow()` would be wrong
/// after hiding because that HWND belongs to the user's other application.
#[cfg(windows)]
pub fn restore_previous() {
    let previous = previous_layout()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take());
    let Some(previous) = previous else {
        return;
    };
    let previous = HKL(previous as *mut core::ffi::c_void);
    let _ = unsafe { ActivateKeyboardLayout(previous, KLF_ACTIVATE) };
}

#[cfg(not(windows))]
pub fn switch_to_english() {}

#[cfg(not(windows))]
pub fn restore_previous() {}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::is_english_layout;
    #[cfg(windows)]
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;

    #[cfg(windows)]
    #[test]
    fn english_layout_detection_uses_primary_language_id() {
        assert!(is_english_layout(
            HKL(0x0409usize as *mut core::ffi::c_void)
        ));
        assert!(is_english_layout(
            HKL(0x1009usize as *mut core::ffi::c_void)
        ));
        assert!(!is_english_layout(HKL(
            0x0804usize as *mut core::ffi::c_void
        )));
    }
}
