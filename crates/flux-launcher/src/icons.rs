use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex, OnceLock,
};
use std::thread;

use crate::applications::resolve_bare_executable_path;
use windui::core::{EventCtx, Widget};
use windui::event::Event;
use windui::prelude::*;
use windui::render::Canvas;

#[cfg(windows)]
pub(crate) const MAX_SHELL_ICON_CACHE_ENTRIES: usize = 128;
#[cfg(windows)]
static SHELL_ICON_CACHE: OnceLock<Mutex<ShellIconCache>> = OnceLock::new();
pub(crate) static SHELL_ICON_COMPLETION_GENERATION: AtomicU64 = AtomicU64::new(0);
static GOOGLE_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();
static OBSIDIAN_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// A stable result-row icon that starts with a lightweight fallback and swaps to the
/// cached Windows Shell image when the background icon worker completes. Keeping this
/// widget inside the existing row avoids rebuilding the dynamic result list, which
/// would otherwise reset row-local interaction state and can disturb scrolling.
pub(crate) struct ResultIconView {
    target: Option<String>,
    fallback: String,
    fallback_font: &'static str,
    refresh_generation: Signal<u64>,
    last_generation: u64,
    pub(crate) image: Option<Image>,
}

impl ResultIconView {
    pub(crate) fn new(
        target: Option<String>,
        fallback: String,
        fallback_font: &'static str,
        initial_rgba: Option<Vec<u8>>,
        refresh_generation: Signal<u64>,
    ) -> Self {
        let image = initial_rgba
            .as_deref()
            .and_then(|rgba| Image::from_rgba(32, 32, rgba).ok());
        Self {
            target,
            fallback,
            fallback_font,
            refresh_generation,
            last_generation: refresh_generation.get(),
            image,
        }
    }

    fn refresh_cached_image(&mut self) {
        let Some(target) = self.target.as_deref() else {
            return;
        };
        #[cfg(windows)]
        let rgba = shell_icon_cache_lookup(target).flatten();
        #[cfg(not(windows))]
        let rgba: Option<Vec<u8>> = None;
        self.image = rgba
            .as_deref()
            .and_then(|bytes| Image::from_rgba(32, 32, bytes).ok());
    }
}

impl Widget for ResultIconView {
    fn measure(
        &self,
        _avail: Size,
        _style: &Style,
        _text: &mut dyn windui::text::TextEngine,
    ) -> Size {
        Size::new(32, 32)
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &Style,
    ) {
        if let Some(image) = self.image.as_ref() {
            canvas.draw_image(image, bounds, Fit::Contain, style.corner_radius, 1.0);
            return;
        }
        let fallback_style = Style {
            font_family: Some(self.fallback_font.to_owned()),
            font_size: 20.0,
            text_align: Align::Center,
            fg: Color::rgba(201, 218, 240, 235),
            fg_role: None,
            ..style.clone()
        };
        canvas.draw_text(
            &self.fallback,
            bounds,
            fallback_style.fg,
            Align::Center,
            &windui::text::TextStyle::of(&fallback_style),
        );
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let generation = self.refresh_generation.get();
        if generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        self.refresh_cached_image();
        ctx.mark_dirty();
    }

    fn on_event(&mut self, _ctx: &mut EventCtx, _event: &Event) -> bool {
        false
    }
}

fn decode_bundled_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    const ICON_SIZE: usize = 32;
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let Ok(mut reader) = decoder.read_info() else {
        return None;
    };
    let mut source = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut source) else {
        return None;
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }

    let source = &source[..info.buffer_size()];
    let mut icon = vec![0_u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        let source_y = y * info.height as usize / ICON_SIZE;
        for x in 0..ICON_SIZE {
            let source_x = x * info.width as usize / ICON_SIZE;
            let source_index = (source_y * info.width as usize + source_x) * 4;
            let target_index = (y * ICON_SIZE + x) * 4;
            icon[target_index..target_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    Some(icon)
}

pub(crate) fn bundled_icon_rgba(result_id: &str) -> Option<Vec<u8>> {
    match result_id {
        "builtin:google-search" => google_icon_rgba(),
        _ if result_id.starts_with("builtin:obsidian:") => obsidian_icon_rgba(),
        _ => None,
    }
}

pub(crate) fn google_icon_rgba() -> Option<Vec<u8>> {
    GOOGLE_ICON_RGBA
        .get_or_init(|| decode_bundled_icon(include_bytes!("../assets/google.png")))
        .clone()
}

pub(crate) fn obsidian_icon_rgba() -> Option<Vec<u8>> {
    OBSIDIAN_ICON_RGBA
        .get_or_init(|| decode_bundled_icon(include_bytes!("../assets/obsidian.png")))
        .clone()
}

pub(crate) fn tray_icon() -> Vec<u8> {
    const ICON_SIZE: usize = 16;
    let fallback = || {
        let mut pixels = Vec::with_capacity(ICON_SIZE * ICON_SIZE * 4);
        for y in 0..ICON_SIZE {
            for x in 0..ICON_SIZE {
                let active = (x + y) % 5 < 3;
                let (red, green, blue) = if active { (78, 139, 255) } else { (28, 39, 62) };
                pixels.extend([red, green, blue, 255]);
            }
        }
        pixels
    };

    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../../assets/ico.png"
    )));
    let Ok(mut reader) = decoder.read_info() else {
        return fallback();
    };
    let mut source = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut source) else {
        return fallback();
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return fallback();
    }

    let source = &source[..info.buffer_size()];
    let mut icon = vec![0_u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        let source_y = y * info.height as usize / ICON_SIZE;
        for x in 0..ICON_SIZE {
            let source_x = x * info.width as usize / ICON_SIZE;
            let source_index = (source_y * info.width as usize + source_x) * 4;
            let target_index = (y * ICON_SIZE + x) * 4;
            icon[target_index..target_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    icon
}

pub(crate) fn icon_completion_generation_changed(previous: u64, current: u64) -> bool {
    previous != current
}

pub(crate) fn icon_target_for_path(target: &str) -> String {
    resolve_bare_executable_path(target).unwrap_or_else(|| target.to_owned())
}

#[cfg(windows)]
pub(crate) struct ShellIconCache {
    pub(crate) entries: HashMap<String, Option<Vec<u8>>>,
    pub(crate) lru_order: VecDeque<String>,
}

#[cfg(windows)]
impl ShellIconCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    pub(crate) fn get(&mut self, target: &str) -> Option<Option<Vec<u8>>> {
        let icon = self.entries.get(target).cloned()?;
        self.touch(target);
        Some(icon)
    }

    pub(crate) fn insert(&mut self, target: String, icon: Option<Vec<u8>>) {
        if self.entries.contains_key(&target) {
            self.entries.insert(target.clone(), icon);
            self.touch(&target);
            return;
        }
        while self.entries.len() >= MAX_SHELL_ICON_CACHE_ENTRIES {
            let Some(oldest) = self.lru_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.lru_order.push_back(target.clone());
        self.entries.insert(target, icon);
    }

    fn touch(&mut self, target: &str) {
        if let Some(position) = self.lru_order.iter().position(|key| key == target) {
            self.lru_order.remove(position);
        }
        self.lru_order.push_back(target.to_owned());
    }
}

struct ShellIconWorker {
    pending: Arc<Mutex<HashSet<String>>>,
    wake: SyncSender<String>,
}

#[cfg(windows)]
fn initialize_shell_icon_worker_com() -> bool {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_ok() {
        true
    } else if result == RPC_E_CHANGED_MODE {
        eprintln!("[flux] shell icon worker inherited an initialized COM apartment");
        false
    } else {
        eprintln!("[flux] shell icon worker COM initialization failed: {result:?}");
        false
    }
}

impl ShellIconWorker {
    fn spawn() -> Self {
        let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
        let pending_for_worker = Arc::clone(&pending);
        let (wake, receiver) = mpsc::sync_channel::<String>(64);
        thread::Builder::new()
            .name(String::from("flux-shell-icons"))
            .spawn(move || {
                #[cfg(windows)]
                let owns_com_apartment = initialize_shell_icon_worker_com();

                while let Ok(target) = receiver.recv() {
                    if let Ok(mut pending) = pending_for_worker.lock() {
                        pending.remove(&target);
                    }
                    #[cfg(windows)]
                    let _ = shell_icon_rgba(&target);
                    SHELL_ICON_COMPLETION_GENERATION.fetch_add(1, Ordering::Release);
                }

                #[cfg(windows)]
                if owns_com_apartment {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            })
            .expect("failed to create shell icon worker thread");
        Self { pending, wake }
    }

    fn request(&self, target: String) {
        let should_send = self
            .pending
            .lock()
            .map(|mut pending| pending.insert(target.clone()))
            .unwrap_or(false);
        if !should_send {
            return;
        }
        if self.wake.try_send(target.clone()).is_err() {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&target);
            }
        }
    }
}

static SHELL_ICON_WORKER: OnceLock<ShellIconWorker> = OnceLock::new();

fn shell_icon_worker() -> &'static ShellIconWorker {
    SHELL_ICON_WORKER.get_or_init(ShellIconWorker::spawn)
}

#[cfg(windows)]
fn shell_icon_cache_lookup(target: &str) -> Option<Option<Vec<u8>>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    cache.lock().ok().and_then(|mut cache| cache.get(target))
}

#[cfg(windows)]
pub(crate) fn request_shell_icon(target: &str) -> Option<Vec<u8>> {
    if let Some(icon) = shell_icon_cache_lookup(target) {
        return icon;
    }
    shell_icon_worker().request(target.to_owned());
    None
}

#[cfg(not(windows))]
pub(crate) fn request_shell_icon(_target: &str) -> Option<Vec<u8>> {
    None
}

pub(crate) fn trace_result_icon_probe(
    title: &str,
    target: Option<&str>,
    icon_target: Option<&str>,
    initial_loaded: bool,
) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let sanitize = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let title = sanitize(title);
    let target = target.map(sanitize).unwrap_or_default();
    let icon_target = icon_target.map(sanitize).unwrap_or_default();
    let _ = writeln!(
        file,
        "title={title}\ttarget={target}\ticon_target={icon_target}\tinitial_loaded={initial_loaded}"
    );
}

fn trace_shell_icon_probe(target: &str, loaded: bool) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let target = target.replace(['\t', '\r', '\n'], " ");
    let _ = writeln!(file, "target={target}\tloaded={loaded}");
}

#[cfg(windows)]
pub(crate) fn shortcut_icon_smoke(target: &str) -> bool {
    shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .is_some_and(|rgba| rgba.len() == 32 * 32 * 4)
}

#[cfg(windows)]
fn shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(icon) = cache.get(target) {
            return icon;
        }
    }

    // Steam's Start Menu entries are commonly .lnk/.url shortcuts whose icon is
    // stored separately from the launch target. Resolve that explicit icon first,
    // matching Flow Launcher's shortcut-aware image loader, then use Shell fallbacks.
    let icon = shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .or_else(|| {
            is_executable_icon_target(target)
                .then(|| extract_shell_icon_rgba(target))
                .flatten()
        })
        .or_else(|| extract_shell_thumbnail_rgba(target))
        .or_else(|| extract_shell_icon_rgba(target));
    trace_shell_icon_probe(target, icon.is_some());
    if let Ok(mut cache) = cache.lock() {
        cache.insert(target.to_owned(), icon.clone());
    }
    icon
}

#[cfg(windows)]
fn shortcut_icon_location(target: &str) -> Option<(String, i32)> {
    let extension = std::path::Path::new(target)
        .extension()
        .and_then(|value| value.to_str())?;
    if extension.eq_ignore_ascii_case("lnk") {
        return shell_link_icon_location(target);
    }
    if extension.eq_ignore_ascii_case("url") {
        let contents = std::fs::read_to_string(target).ok()?;
        let (path, index) = parse_internet_shortcut_icon_location(&contents)?;
        return resolve_shortcut_icon_path(target, &path).map(|path| (path, index));
    }
    None
}

pub(crate) fn parse_internet_shortcut_icon_location(contents: &str) -> Option<(String, i32)> {
    let mut icon_file = None;
    let mut icon_index = 0_i32;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key.eq_ignore_ascii_case("IconFile") && !value.is_empty() {
            icon_file = Some(value.to_owned());
        } else if key.eq_ignore_ascii_case("IconIndex") {
            icon_index = value.parse::<i32>().unwrap_or(0);
        }
    }
    icon_file.map(|path| (path, icon_index))
}

pub(crate) fn resolve_shortcut_icon_path(shortcut_path: &str, icon_path: &str) -> Option<String> {
    let expanded = expand_percent_variables_for_icon(icon_path)?;
    let expanded = expanded.trim().trim_matches('"');
    if expanded.is_empty() {
        return None;
    }
    let path = std::path::Path::new(expanded);
    if path.is_absolute() {
        return Some(expanded.to_owned());
    }
    let parent = std::path::Path::new(shortcut_path).parent()?;
    Some(parent.join(path).to_string_lossy().into_owned())
}

fn expand_percent_variables_for_icon(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let variable = &rest[start + 1..];
        let end = variable.find('%')?;
        let name = &variable[..end];
        let replacement = std::env::var_os(name)?.to_string_lossy().into_owned();
        output.push_str(&replacement);
        rest = &variable[end + 1..];
    }
    output.push_str(rest);
    Some(output)
}

#[cfg(windows)]
fn shell_link_icon_location(path: &str) -> Option<(String, i32)> {
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::IShellLinkW;

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let wide_path = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ).ok()?;
        let mut icon_path = [0_u16; 32_768];
        let mut icon_index = 0_i32;
        link.GetIconLocation(&mut icon_path, &mut icon_index).ok()?;
        let end = icon_path
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(icon_path.len());
        let icon_path = String::from_utf16_lossy(&icon_path[..end]);
        resolve_shortcut_icon_path(path, &icon_path).map(|path| (path, icon_index))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(windows))]
fn shell_icon_rgba(_target: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn extract_shell_thumbnail_rgba(target: &str) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_ICONONLY,
        SIIGBF_SCALEUP,
    };

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let shell_item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()? };
    let image_factory: IShellItemImageFactory = shell_item.cast().ok()?;
    let bitmap = unsafe {
        image_factory
            .GetImage(
                SIZE {
                    cx: ICON_SIZE,
                    cy: ICON_SIZE,
                },
                SIIGBF(SIIGBF_ICONONLY.0 | SIIGBF_SCALEUP.0),
            )
            .ok()?
    };
    if bitmap.0.is_null() {
        return None;
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let copied = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            ICON_SIZE as u32,
            Some(bgra.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    if copied == 0 {
        return None;
    }

    let has_alpha = bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend([
            pixel[2],
            pixel[1],
            pixel[0],
            if has_alpha { pixel[3] } else { 255 },
        ]);
    }
    Some(rgba)
}

#[cfg(windows)]
fn extract_shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    extract_icon_rgba_from_source(target, None)
}

pub(crate) fn is_executable_icon_target(target: &str) -> bool {
    matches!(
        std::path::Path::new(target.trim().trim_matches('"'))
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("exe") | Some("com") | Some("bat") | Some("cmd")
    )
}

#[cfg(windows)]
fn extract_icon_rgba_from_source(source: &str, icon_index: Option<i32>) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
    let icon = if let Some(index) = icon_index {
        let mut icon = HICON::default();
        let extracted = unsafe {
            ExtractIconExW(
                PCWSTR(path.as_ptr()),
                index,
                Some(&mut icon as *mut HICON),
                None,
                1,
            )
        };
        (extracted > 0 && !icon.is_invalid()).then_some(icon)?
    } else {
        let mut file_info = SHFILEINFOW::default();
        let flags = SHGFI_FLAGS(SHGFI_ICON.0 | SHGFI_LARGEICON.0);
        let result = unsafe {
            SHGetFileInfoW(
                PCWSTR(path.as_ptr()),
                Default::default(),
                Some(&mut file_info),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        (result != 0 && !file_info.hIcon.is_invalid()).then_some(file_info.hIcon)?
    };

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe { DestroyIcon(icon).ok()? };
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let bitmap =
        unsafe { CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) };
    let Ok(bitmap) = bitmap else {
        unsafe {
            DestroyIcon(icon).ok();
            let _ = DeleteDC(hdc);
        }
        return None;
    };
    let previous = unsafe { SelectObject(hdc, HGDIOBJ(bitmap.0)) };
    let drawn =
        unsafe { DrawIconEx(hdc, 0, 0, icon, ICON_SIZE, ICON_SIZE, 0, None, DI_NORMAL).is_ok() };
    let rgba = if drawn && !bits.is_null() {
        let bgra = unsafe {
            std::slice::from_raw_parts(bits.cast::<u8>(), (ICON_SIZE * ICON_SIZE * 4) as usize)
        };
        let mut rgba = Vec::with_capacity(bgra.len());
        for pixel in bgra.chunks_exact(4) {
            rgba.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        Some(rgba)
    } else {
        None
    };
    unsafe {
        SelectObject(hdc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(hdc);
        DestroyIcon(icon).ok();
    }
    rgba
}
