use std::sync::OnceLock;

static GOOGLE_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();
static OBSIDIAN_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_obsidian_icon_decodes_to_32_pixel_rgba_bitmap() {
        let icon = obsidian_icon_rgba().expect("bundled Obsidian icon should decode");
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert!(icon.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn obsidian_result_uses_the_bundled_icon() {
        let icon = bundled_icon_rgba("builtin:obsidian:notes/readme.md")
            .expect("Obsidian result should use the bundled icon");
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert!(bundled_icon_rgba("everything:file:readme.md").is_none());
    }
}
