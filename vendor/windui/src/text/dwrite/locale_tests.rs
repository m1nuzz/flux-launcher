use super::*;
use windows::Win32::Graphics::DirectWrite::IDWriteFontFace3;

#[test]
fn rtl_language_tags_are_case_insensitive() {
    for locale in ["ar", "AR-EG", "he-IL", "HE", "fa-IR", "Ur-PK"] {
        assert!(is_rtl_locale(locale), "{locale}");
    }
    assert!(!is_rtl_locale("en-US"));
}

#[derive(Debug)]
struct Run {
    weight: i32,
    bidi: u32,
    glyphs: Vec<u16>,
    nominal: Vec<u16>,
}

#[implement(IDWriteTextRenderer)]
struct ProbeRenderer;

#[allow(non_snake_case)]
impl IDWriteTextRenderer_Impl for ProbeRenderer_Impl {
    fn DrawGlyphRun(
        &self,
        context: *const c_void,
        _x: f32,
        _y: f32,
        _mode: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<'_, IUnknown>,
    ) -> Result<()> {
        // SAFETY: layout.Draw invokes this callback synchronously with the
        // writable Vec passed by probe() and a valid DirectWrite glyph run.
        unsafe {
            let run = &*run;
            let face: IDWriteFontFace3 =
                run.fontFace.as_ref().expect("resolved font face").cast()?;
            let text = &*description;
            let characters = String::from_utf16_lossy(std::slice::from_raw_parts(
                text.string.0,
                text.stringLength as usize,
            ));
            let points: Vec<u32> = characters.chars().map(u32::from).collect();
            let mut nominal = vec![0; points.len()];
            face.GetGlyphIndices(points.as_ptr(), points.len() as u32, nominal.as_mut_ptr())?;
            let output = &mut *(context as *mut Vec<Run>);
            output.push(Run {
                weight: face.GetWeight().0,
                bidi: run.bidiLevel,
                glyphs: std::slice::from_raw_parts(run.glyphIndices, run.glyphCount as usize)
                    .to_vec(),
                nominal,
            });
        }
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: Ref<'_, IUnknown>,
    ) -> Result<()> {
        Ok(())
    }
    fn DrawStrikethrough(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Ref<'_, IUnknown>,
    ) -> Result<()> {
        Ok(())
    }
    fn DrawInlineObject(
        &self,
        _: *const c_void,
        _: f32,
        _: f32,
        _: Ref<'_, IDWriteInlineObject>,
        _: BOOL,
        _: BOOL,
        _: Ref<'_, IUnknown>,
    ) -> Result<()> {
        Ok(())
    }
}

#[allow(non_snake_case)]
impl IDWritePixelSnapping_Impl for ProbeRenderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
        Ok(BOOL(1))
    }
    fn GetCurrentTransform(&self, _: *const c_void, matrix: *mut DWRITE_MATRIX) -> Result<()> {
        // SAFETY: DirectWrite supplies a writable transform pointer.
        unsafe {
            *matrix = DWRITE_MATRIX {
                m11: 1.0,
                m22: 1.0,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
        Ok(1.0)
    }
}

struct RestoreLocale(String);

impl RestoreLocale {
    fn set(locale: &str) -> Self {
        let previous = current_ui_locale();
        set_ui_locale(locale);
        Self(previous)
    }
}

impl Drop for RestoreLocale {
    fn drop(&mut self) {
        set_ui_locale(&self.0);
    }
}

fn probe(layout: &IDWriteTextLayout) -> Vec<Run> {
    let renderer: IDWriteTextRenderer = ProbeRenderer.into();
    let mut runs = Vec::<Run>::new();
    // SAFETY: the synchronous callback cannot outlive the stack-owned output.
    unsafe {
        layout.Draw(
            Some(&mut runs as *mut _ as *const c_void),
            &renderer,
            0.0,
            0.0,
        )
    }
    .unwrap();
    runs
}

#[test]
fn cyrillic_semibold_resolves_to_a_semibold_glyph_face() {
    let _locale = RestoreLocale::set("ru-RU");
    for family in [None, Some("Segoe UI")] {
        let mut engine = DWriteEngine::new();
        let style = TextStyle {
            family,
            weight: 600,
            ..TextStyle::new(14.0)
        };
        let layout = engine.layout("\u{41a}\u{43b}\u{430}\u{432}\u{438}\u{448}\u{430} \u{430}\u{43a}\u{442}\u{438}\u{432}\u{430}\u{446}\u{438}\u{438}", &style, 14.0, 600.0).unwrap();
        let runs = probe(&layout);
        assert!(!runs.is_empty());
        assert!(
            runs.iter().all(|run| run.weight >= 600),
            "actual resolved glyph-run weights: {runs:?}"
        );
        assert!(runs
            .iter()
            .all(|run| run.glyphs.iter().all(|glyph| *glyph != 0)));
    }
}

#[test]
fn rtl_paragraphs_shape_arabic_and_preserve_mixed_direction_runs() {
    for (locale, text) in [
        ("ar", "\u{644}\u{627} 12 Flux"),
        ("he", "\u{5e9}\u{5dc}\u{5d5}\u{5dd} 12 Flux"),
    ] {
        let _locale = RestoreLocale::set(locale);
        let mut engine = DWriteEngine::new();
        let style = TextStyle::new(20.0);
        let layout = engine.layout(text, &style, 20.0, 400.0).unwrap();
        assert_eq!(
            unsafe { layout.GetReadingDirection() },
            windows::Win32::Graphics::DirectWrite::DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
        );
        let runs = probe(&layout);
        assert!(
            runs.iter().any(|run| run.bidi % 2 == 1),
            "missing RTL run: {runs:?}"
        );
        assert!(
            runs.iter().any(|run| run.bidi % 2 == 0),
            "Latin and numbers must stay LTR: {runs:?}"
        );
        assert!(
            runs.iter()
                .all(|run| run.glyphs.iter().all(|glyph| *glyph != 0)),
            "missing shaped glyph: {runs:?}"
        );
        if locale == "ar" {
            let ligature = engine
                .layout("\u{644}\u{627}", &style, 20.0, 400.0)
                .unwrap();
            let shaped = probe(&ligature);
            assert!(
                shaped
                    .iter()
                    .any(|run| run.glyphs.iter().any(|glyph| !run.nominal.contains(glyph))),
                "Arabic must use contextual glyphs rather than isolated nominal glyphs: {shaped:?}"
            );
        }
        // The software path uses tight bitmaps. RTL text must paint pixels even
        // when its layout rectangle is much wider than the actual glyph block.
        let mut pixmap = Pixmap::new(400, 60).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        engine.draw(
            &mut pixmap,
            text,
            Rect::new(0, 0, 400, 60),
            Color::BLACK,
            Align::Start,
            &style,
            None,
        );
        assert!(pixmap.pixels().iter().any(|pixel| pixel.red() < 200));
    }
}

#[test]
fn changing_ui_locale_refreshes_formats_and_measurements() {
    let _locale = RestoreLocale::set("en-US");
    let mut engine = DWriteEngine::new();
    let style = TextStyle::new(14.0);
    engine.measure("Flux", &style, None);
    assert!(!engine.formats.is_empty());
    set_ui_locale("he");
    engine.measure("Flux", &style, None);
    assert_eq!(engine.locale, "he");
    assert_eq!(engine.formats.len(), 1);
    assert_eq!(engine.measure_cache.len(), 1);
    let format = engine.format(&style, 14.0).unwrap();
    assert_eq!(
        unsafe { format.GetReadingDirection() },
        windows::Win32::Graphics::DirectWrite::DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
    );
    let mut locale = vec![0_u16; unsafe { format.GetLocaleNameLength() } as usize + 1];
    unsafe { format.GetLocaleName(&mut locale) }.unwrap();
    assert_eq!(String::from_utf16_lossy(&locale[..locale.len() - 1]), "he");
}
