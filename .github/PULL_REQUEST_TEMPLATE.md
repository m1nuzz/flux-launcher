## What changed

<!-- A short description of the change and why it is needed. -->

## How it was tested

<!-- Commands run, Windows behavior verified, screenshots or recordings for UI changes. -->

## Checklist

- [ ] I have read and signed the [CLA](../CLA.md) by commenting the exact phrase on this pull request.
- [ ] `cargo fmt --all -- --check`, `cargo check`, clippy with `-D warnings`, and the relevant tests pass.
- [ ] UI, lifecycle, installer, or startup changes were verified on Windows.
- [ ] Comments, documentation, and user-facing strings are in English.
- [ ] The change uses the vendored `windui` framework only (no WebView, Electron, Tauri, egui, iced, or another GUI toolkit).
- [ ] The change is focused; unrelated refactors and formatting-only churn are avoided.
