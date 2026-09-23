//! Win32 剪贴板读写（CF_UNICODETEXT 文本 / CF_DIB 图片）。

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

use crate::core::ClipboardProvider;

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;

/// Win32 剪贴板实现，由 UiHost 注入 `Tree`。
pub struct WinClipboard;

impl ClipboardProvider for WinClipboard {
    fn get_text(&self) -> Option<String> {
        unsafe { get_text() }
    }
    fn set_text(&self, text: &str) {
        unsafe { set_text(text) };
    }
    fn set_image(&self, width: u32, height: u32, rgba: &[u8]) {
        unsafe { set_image(width, height, rgba) };
    }
    fn set_text_and_image(&self, text: &str, width: u32, height: u32, rgba: &[u8]) {
        unsafe { set_text_and_image(text, width, height, rgba) };
    }
}

/// 把直排 RGBA8 拼成 bottom-up 的 32bpp BI_RGB DIB（CF_DIB 载荷）。
/// 32bpp 的第四字节为保留位（填 0，读取方忽略）；DIB 没有 alpha 通道概念，
/// 故输入按直排理解——调用方负责把预乘缓冲先反预乘。
fn dib_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    if width > i32::MAX as u32 || height > i32::MAX as u32 {
        return None;
    }
    let stride = (width as u64).checked_mul(4)?;
    let pixels_len = stride.checked_mul(height as u64)?;
    if pixels_len != rgba.len() as u64 || pixels_len > u32::MAX as u64 {
        return None;
    }
    let total = 40u64.checked_add(pixels_len)?;
    if total > isize::MAX as u64 {
        return None;
    }
    let mut dib = Vec::with_capacity(total as usize);
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight>0 = bottom-up
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    dib.extend_from_slice(&(pixels_len as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
                                                // Bottom-up：先写最后一行；通道 RGBA→BGRX。
    for y in (0..height).rev() {
        let row = &rgba[(y as usize) * (stride as usize)..][..stride as usize];
        for px in row.chunks_exact(4) {
            dib.push(px[2]);
            dib.push(px[1]);
            dib.push(px[0]);
            dib.push(0);
        }
    }
    Some(dib)
}

/// 分配可移动全局内存并拷入内容。返回已解锁句柄，调用方负责系统交接。
unsafe fn alloc_copy(src: *const u8, len: usize) -> Option<HGLOBAL> {
    if len == 0 {
        return None;
    }
    let hmem = GlobalAlloc(GMEM_MOVEABLE, len).ok()?;
    let ptr = GlobalLock(hmem) as *mut u8;
    if ptr.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(src, ptr, len);
    let _ = GlobalUnlock(hmem);
    Some(hmem)
}

unsafe fn get_text() -> Option<String> {
    let _: Option<HWND> = None;
    OpenClipboard(None).ok()?;
    let result = (|| {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        if h.0.is_null() {
            return None;
        }
        let hg = HGLOBAL(h.0);
        let ptr = GlobalLock(hg) as *const u16;
        if ptr.is_null() {
            return None;
        }
        // 剪贴板是不可信的跨进程数据：用分配块大小作上界，防止无 NUL 数据越界读。
        let cap = GlobalSize(hg) / 2; // u16 元素数
        let mut len = 0usize;
        while len < cap && *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let s = String::from_utf16_lossy(slice);
        let _ = GlobalUnlock(hg);
        Some(s)
    })();
    let _ = CloseClipboard();
    result
}

unsafe fn set_text(text: &str) {
    if OpenClipboard(None).is_err() {
        return;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;
    if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
        let ptr = GlobalLock(hmem) as *mut u16;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);
            // 数据就绪后才清空（分配/锁定失败时不丢失用户原剪贴板内容）。
            let _ = EmptyClipboard();
            // 成功后系统接管 hmem 所有权。SetClipboardData 失败为极罕见路径，
            // 此时 hmem 未交系统、会泄漏一次；可忽略。
            let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0)));
        }
    }
    let _ = CloseClipboard();
}

unsafe fn set_image(width: u32, height: u32, rgba: &[u8]) {
    let Some(dib) = dib_from_rgba(width, height, rgba) else {
        return;
    };
    if OpenClipboard(None).is_err() {
        return;
    }
    if let Some(hmem) = alloc_copy(dib.as_ptr(), dib.len()) {
        let _ = EmptyClipboard();
        // 成功后系统接管 hmem 所有权（注释同 set_text）。
        let _ = SetClipboardData(CF_DIB, Some(HANDLE(hmem.0)));
    }
    let _ = CloseClipboard();
}

unsafe fn set_text_and_image(text: &str, width: u32, height: u32, rgba: &[u8]) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let dib = dib_from_rgba(width, height, rgba);
    if OpenClipboard(None).is_err() {
        return;
    }
    // 两份载荷都就绪后才 EmptyClipboard：任一失败都不丢失用户原剪贴板内容。
    let text_mem = alloc_copy(wide.as_ptr() as *const u8, wide.len() * 2);
    let dib_mem = dib.as_ref().and_then(|d| alloc_copy(d.as_ptr(), d.len()));
    if let (Some(th), Some(dh)) = (text_mem, dib_mem) {
        let _ = EmptyClipboard();
        // 成功后系统接管所有权（注释同 set_text）。
        let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(th.0)));
        let _ = SetClipboardData(CF_DIB, Some(HANDLE(dh.0)));
    }
    let _ = CloseClipboard();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_header_describes_bottom_up_32bpp() {
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let dib = dib_from_rgba(2, 1, &rgba).unwrap();
        assert_eq!(dib.len(), 40 + 8);
        let bi_size = u32::from_le_bytes(dib[0..4].try_into().unwrap());
        let bi_width = i32::from_le_bytes(dib[4..8].try_into().unwrap());
        let bi_height = i32::from_le_bytes(dib[8..12].try_into().unwrap());
        let bi_planes = u16::from_le_bytes(dib[12..14].try_into().unwrap());
        let bi_bitcount = u16::from_le_bytes(dib[14..16].try_into().unwrap());
        let bi_compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
        let bi_size_image = u32::from_le_bytes(dib[20..24].try_into().unwrap());
        assert_eq!(
            (
                bi_size,
                bi_width,
                bi_height,
                bi_planes,
                bi_bitcount,
                bi_compression,
                bi_size_image
            ),
            (40, 2, 1, 1, 32, 0, 8)
        );
    }

    #[test]
    fn dib_rows_are_bottom_up_bgr() {
        // 1x2：上红下蓝 → DIB 先存蓝行，通道 RGBA→BGRX。
        let rgba = vec![255, 0, 0, 255, 0, 0, 255, 255];
        let dib = dib_from_rgba(1, 2, &rgba).unwrap();
        assert_eq!(&dib[40..44], &[255, 0, 0, 0]);
        assert_eq!(&dib[44..48], &[0, 0, 255, 0]);
    }

    #[test]
    fn dib_rejects_bad_inputs() {
        assert!(dib_from_rgba(0, 1, &[]).is_none());
        assert!(dib_from_rgba(1, 0, &[]).is_none());
        assert!(dib_from_rgba(2, 2, &[0u8; 15]).is_none());
        assert!(dib_from_rgba(1, 1, &[0u8; 5]).is_none());
        assert!(dib_from_rgba(1, 1, &[0u8; 4]).is_some());
    }
}
