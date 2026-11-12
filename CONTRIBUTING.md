# Contributing to Flux Launcher

Flux Launcher is a solo-maintained project. Issues and pull requests are welcome, but every contribution of code, documentation, translations, artwork, or other materials must be covered by the [Contributor License Agreement](CLA.md).

## Before your pull request can be merged

1. Read [CLA.md](CLA.md). It assigns the copyright in your contribution to the maintainer; you keep a non-exclusive license back to use your own contribution anywhere.
2. Sign it by commenting exactly this phrase on your pull request:

   ```text
   I have read the CLA Document and I hereby sign the CLA
   ```

3. Wait for the `CLAAssistant` check to pass.

Pull requests without a signed CLA cannot be merged. Signing is required once per GitHub account.

## Scope

The maintainer decides what lands in the project. Changes that affect the product direction — interface localization, new providers, release automation, licensing — should be discussed in an issue before code is written.

## Quality gates

Run the gates that apply to your change before opening the pull request:

```text
cargo fmt --all
cargo fmt --all -- --check
cargo check --workspace --target x86_64-pc-windows-gnu
cargo clippy -p flux-core -p flux-launcher --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo test -p flux-core
cargo test -p flux-launcher
```

For UI, lifecycle, installer, or startup changes, verify the behavior on Windows and describe what you tested. Screenshots or a short recording help.

## Code rules

- English comments, documentation, and user-facing strings.
- The GUI stays on the vendored `windui` framework: no WebView, Electron, Tauri, egui, iced, or another GUI toolkit.
- Keep the platform split: Windows-specific code behind platform modules, non-Windows compilation preserved where practical.
- Plugin support is limited to native or executable JSON-RPC plugins; no Python or C# plugin runtime.
- Keep changes focused. Unrelated refactors and formatting-only churn in an existing area make a pull request harder to review.

## Commit messages

Use concise English [Conventional Commits](https://www.conventionalcommits.org/) messages, for example `fix(launcher): ...` or `feat(settings): ...`.
