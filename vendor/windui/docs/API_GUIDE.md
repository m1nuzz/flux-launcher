# Text locale on Windows

Call `windui::text::set_ui_locale("ar")` on the UI thread before `App::run()`
to select the BCP 47 locale used by both DirectWrite rendering backends. When
unset, the engine uses the Windows user locale. Existing measurement and layout
caches refresh when the locale changes. Other UI threads retain their own locale.
`windui::text::ui_locale()` returns the current thread's locale so temporary
rendering or tests can restore it after selecting another language.

Arabic and Hebrew use right-to-left paragraph shaping. Widget coordinates and
physical `Align::Start`/`Align::End` placement remain unchanged; full interface
mirroring is the application's responsibility.

The default family is `Segoe UI Variable`. Native renderers prefer its registered
umbrella family, then `Segoe UI Variable Text`, and fall back to `Segoe UI`, which
is available on Windows clients and Server and has real semibold Cyrillic faces.
If font enumeration fails or finds none of these families, the default request
also falls back to `Segoe UI`. Explicit widget font families, including icon
fonts and private-use fallbacks, continue to take precedence.

When the umbrella family is unavailable, `Segoe UI Variable Text` is intentionally
used at every size, including headings. The renderer does not switch between the
Text, Display, and Small optical families by font size; using one registered
family keeps measured layouts consistent across the two native rendering paths.
