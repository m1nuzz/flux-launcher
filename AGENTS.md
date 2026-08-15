# Flux Launcher Agent Guidelines

## Project scope

Flux Launcher is a native Windows 11 launcher written in Rust. The GUI must use the vendored `windui` framework exclusively. Do not introduce WebView, Electron, egui, iced, Tauri, or another GUI framework.

## Repository language

All Flux-owned source comments, documentation, release notes, and user-facing application strings must be written in English. Do not add Chinese symbols or Chinese comments to Flux-owned code. Conversation with the project owner may use Russian.

## Windows Acrylic requirements

The launcher must use the real Windows DWM system backdrop through the existing Win32 and DirectComposition path. Keep the entire launcher surface transparent so Acrylic fills the complete window. Do not replace the backdrop with fake gradients, opaque cards, tinting gradients, or WCA AccentPolicy as the primary solution. Any visibility, resize, paint, or composition change must preserve Acrylic after repeated hide/show activation and must be validated before release.

The Win32 lifecycle is sensitive to ordering. `ShowWindow` must establish visibility before application show callbacks mutate layout state. After a visible activation, the first transparent D2D frame must be invalidated and presented before relying on user input or a query change.

## UX invariants

The default activation hotkey is Alt+Space and must remain configurable. Repeated activation must toggle visibility. Search receives keyboard focus immediately when shown. Clear-query-on-activation is enabled by default. Game Mode and fullscreen hotkey protection are enabled by default. Application results must rank before ordinary files and folders. Keyboard navigation must support Up, Down, Home, End, Enter, Right, Left, and Escape according to the existing Flow-style behavior.

Result rows must remain readable on both dark and bright Acrylic samples. Titles must not overlap subtitles or adjacent rows. Selection state must be reactive and visibly unique. Keep the Windows accent-color default and custom palette fallback intact.

## Dependencies and architecture

Keep dependencies pinned where practical. Prefer small, platform-specific changes over broad rewrites. Preserve the separation between `flux-core`, Flux application code, and the vendored `windui` backend. Everything integration must retain graceful fallback behavior. Flow plugin support is limited to native or executable JSON-RPC plugins; do not add Python or C# plugin execution.

## Required validation

Before committing, run:

```text
cargo fmt --all --check
git diff --check
cargo check --workspace --target x86_64-pc-windows-gnu
cargo clippy -p windui --target x86_64-pc-windows-gnu -- -D warnings
cargo clippy -p flux-launcher --target x86_64-pc-windows-gnu -- -D warnings
cargo test -p flux-core
```

For lifecycle or visual changes, dispatch both Windows workflows:

```text
gh workflow run windows-release-artifact.yml --repo m1nuzz/flux-launcher --ref main
gh workflow run windows-visual-smoke.yml --repo m1nuzz/flux-launcher --ref main
```

The visual smoke must start on the configured second display when available, capture the empty launcher before query input, exercise repeated hide/show activation, and then exercise query expansion, keyboard selection, action mode, Enter, and Settings. GitHub runner screenshots are evidence of the rendering path but may not expose the live DWM blur under remote desktop composition.

## Commits and releases

Commits to `m1nuzz/flux-launcher` must use:

```text
user.name=m1nuzz
user.email=m1nusz0r@gmail.com
```

Use concise English commit messages. Do not commit Manus-internal scratch notes, generated planning files, or unrelated artifacts. Release notes must be in English and must state Windows x64 requirements, validation status, and known runner-compositor limitations honestly.
