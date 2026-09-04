# Flux Launcher Agent Guidelines

## Project scope

Flux Launcher is a native Windows 11 launcher written in Rust. The GUI must use the vendored `windui` framework exclusively. Do not introduce WebView, Electron, egui, iced, Tauri, or another GUI framework.

The launcher is a tray-resident application. It must use the real Windows DWM Acrylic or Mica system backdrop through the existing Win32 and DirectComposition path. Keep the entire launcher surface transparent so the system material fills the complete window. Do not replace the backdrop with fake gradients, opaque cards, tinting gradients, or WCA AccentPolicy as the primary solution.

## Repository language and ownership

All Flux-owned source comments, documentation, and release notes must be written in English. Do not add Chinese symbols or Chinese comments to Flux-owned code. User-facing application strings must be externalized through the rust-i18n `t!` macro as described in the Internationalization section below; do not embed visible text directly in UI code. Conversation with the project owner may use Russian.

Use concise English commit messages. Do not commit Manus-internal scratch notes, generated planning files, temporary screenshots, or unrelated artifacts. User-facing documentation may be committed when it is part of the requested product change.

## Internationalization (i18n)

Flux uses the `rust-i18n` framework for every user-facing string. Translation files live in `crates/flux-launcher/locales/`, with `en.yml` as the fallback locale and `zh-CN.yml` for Simplified Chinese. The framework is initialized at the top of `crates/flux-launcher/src/main.rs` with `i18n!("locales", fallback = "en")`.

- Every user-facing string in the launcher UI must be read through the `t!` macro. Do not hardcode visible text in UI code; constants, colors, internal identifiers, and stderr diagnostics are excluded.
- Adding or changing a visible string requires updating both `en.yml` and `zh-CN.yml` in the same change so the two locale files stay key-consistent.
- Missing translations must fall back to `en`; never render a raw key or panic for a missing locale.
- On startup, detect the system language with `sys-locale` and apply it through `rust_i18n::set_locale`; unsupported or unknown languages fall back to `en`.
- The `locales` path in `i18n!` resolves relative to the flux-launcher crate manifest (`CARGO_MANIFEST_DIR`). Do not move the directory to the workspace root without updating the macro path accordingly.
- Settings field names, version strings, and other internal identifiers are not translated.

## Required working process

Work in a deliberate, manual-first loop. Before making a change, inspect the relevant source, workflow, release history, and existing tests. For a multi-step change, create a concrete plan with investigation, implementation, validation, and delivery phases. Do not guess when a repository file, CI log, installer log, or Windows smoke result can answer the question.

Make the smallest complete change that solves the observed problem. Preserve existing architecture and avoid broad rewrites. Keep the user informed at meaningful checkpoints, especially when a CI runner is waiting, a workflow fails, or an external review is pending. Never claim a fix is ready until the relevant local and Windows validation has passed.

Tool failure and Cline built-in tool policy:

- `AGENTS.md` and `AGENTS.zh-CN.md` are the English and Simplified Chinese versions of the same rules. Update both in the same change and keep their requirements semantically equivalent.
- Add a tool to `cline.disabledTools` only after confirming that the Cline built-in tool is unavailable or repeatedly fails the same operation. Do not disable tools for a single transient or unrelated error.
- Distinguish Cline built-in tools from MCP tools. Failures of `github`, `desktop-commander`, or `serena` do not justify disabling a Cline built-in tool.
- For any tool failure, timeout, unexpected result, or partial state change, promptly create or update a Chinese Markdown report under `doc/question/`. Include the time, tool/server, operation, impact, original error or log excerpt, diagnosis, recovery, and validation. English technical terms and original English logs may be retained.
- Preserve the original error text when possible. If the failure is transient or environmental, document the conclusion and its basis; do not silently retry and discard the first failure.

Line-ending policy:

- If a non-Rust text file has CRLF, line-ending corruption, or a line-ending-only diff, stop automated editing and require manual intervention.
- Preserve its content, normalize it to LF, and verify the working tree and index with byte-level checks and `git diff --check`.
- Do not batch-rewrite line endings with scripts unless manual correction is infeasible; `git add filename` alone does not repair the working tree.

Documentation policy: every completed product fix must have a corresponding Markdown repair report under `doc/fix/`, following the existing repair-report structure. New product features should use a dedicated documentation category such as `doc/feat/`; create the category when it does not exist. Keep implementation notes, validation results, known limitations, and user verification steps in the appropriate document, and do not commit temporary scratch notes or generated artifacts.

Every completed product fix must be followed by one manually prepared beta release. Beta releases must use `prerelease: true` and must not include `(beta)` in the release name. The release body must be written or manually corrected before delivery and must state what changed, what was tested, known limitations, download links, and what the project owner should verify. Do not create empty, duplicate, or unexplained releases. Do not create releases automatically on every push, schedule, or internal commit. A release workflow may be started manually to build Windows artifacts, but the release version, channel, and detailed notes must be intentionally selected and checked by the agent.

Stable releases require explicit promotion or user instruction. Beta releases must not be submitted to WinGet. WinGet automation must never create a GitHub release; it may only prepare or submit a stable manifest PR when the stable-only policy is explicitly enabled. Do not request or create `WINGET_GITHUB_TOKEN` or signing secrets for a manual WinGet PR unless the user explicitly asks to enable that automation.

## Windows lifecycle and startup behavior

The default activation hotkey is Alt+Space and must remain configurable. Repeated activation must toggle visibility. Search receives keyboard focus immediately when shown. Clear-query-on-activation is enabled by default. Game Mode and fullscreen hotkey protection are enabled by default. Application results must rank before ordinary files and folders. Keyboard navigation must support Up, Down, Home, End, Enter, Right, Left, and Escape according to the existing Flow-style behavior.

The Windows startup checkbox in the installer is enabled by default but must remain user-selectable. The installer must also show a post-install **Launch Flux Launcher now** option selected by default. These are separate choices: launching immediately after installation must not be confused with enabling startup on future Windows sign-ins.

The startup registry command must use `--startup`. Startup mode must call windui `start_hidden()` so Windows sign-in creates only a running tray process and does not display the search window. The global hotkey or the tray's Show launcher action must be required to show the search window. Installer smoke must verify both default startup and the opt-out path.

The Start Menu shortcut must target the installed executable and explicitly reference the Flux Launcher `.ico` resource. The installer must include the multi-resolution icon resource in the installed directory. Installer smoke must verify the shortcut target and icon metadata, not only that the shortcut file exists.

## Windows Acrylic and lifecycle invariants

The Win32 lifecycle is sensitive to ordering. `ShowWindow` must establish visibility before application show callbacks mutate layout state. After a visible activation, the first transparent D2D frame must be invalidated and presented before relying on user input or a query change.

Any visibility, resize, paint, or composition change must preserve Acrylic after repeated hide/show activation and must be validated before release. Result rows must remain readable on both dark and bright Acrylic samples. Titles must not overlap subtitles or adjacent rows. Selection state must be reactive and visibly unique. Keep the Windows accent-color default and custom palette fallback intact.

## UX invariants

Committed queries are persisted in a bounded, case-insensitive history. Ctrl+H must open selectable newest-first history rows; Enter or a mouse click reruns the selected query; plain Up on an empty field recalls the latest query; Alt+Up and Alt+Down cycle backward and forward; and Settings can clear history. Provider status must remain visible in the expanded action bar without changing the transparent Acrylic surface.

Everything integration must work as an always-available file and folder provider when installed, with graceful no-service fallback when it is unavailable. Native Everything syntax such as `ext:zip`, `parent:`, `file:`, and `dm:` must remain supported. Application results must retain priority over ordinary Everything results.

Flow plugin support is limited to native or executable JSON-RPC plugins. Do not add Python or C# plugin execution. Built-in Google and Obsidian features must remain in the main executable unless the user explicitly requests a separate community plugin host.

## Dependencies and architecture

Keep dependencies pinned where practical. Prefer small, platform-specific changes over broad rewrites. Preserve the separation between `flux-core`, Flux application code, and the vendored `windui` backend. Everything integration must retain graceful fallback behavior. Keep Windows-specific code behind appropriate platform modules and maintain non-Windows cross-target compilation where practical.

## Formatting and line endings

The repository root contains `rustfmt.toml` with the project formatting policy, including Unix line endings. When Rust files show line-ending or formatting sequence differences, run `cargo fmt --all` directly and do not manually rewrite line endings or bypass the configured formatter.

## Required local validation

Before committing, run the applicable quality gates. The standard local gate is:

```text
source "$HOME/.cargo/env"
cargo fmt --all
cargo fmt --all -- --check
git diff --check
cargo check --workspace --target x86_64-pc-windows-gnu
cargo clippy -p flux-core -p flux-launcher --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo test -p flux-core
```

If the change touches vendored `windui`, run its relevant checks as well. If the change touches release packaging, run static manifest or installer checks in addition to the Rust gates. Never ignore a failed check merely because it is unrelated-looking: inspect the log first, distinguish a real regression from a flaky runner, and rerun only when the failure is demonstrably environmental or transient.

## Windows smoke and release validation

For lifecycle, visual, installer, startup, or shortcut changes, manually dispatch the Windows UI release workflow with an explicit beta tag and `release_channel=beta`:

```text
gh workflow run windows-ui-release.yml \
  --repo m1nuzz/flux-launcher \
  --ref main \
  -f release_tag=vX.Y.Z \
  -f runner_label=windows-latest \
  -f release_channel=beta
```

Do not publish or report the beta until the run is successful. The workflow must build the installer and portable executable, run the Windows UI capture, and run `scripts/installer-smoke.ps1`. The installer smoke must cover the default startup entry, `/TASKS=!startup` opt-out, hidden `--startup` mode, executable hash, post-install configuration, Start Menu shortcut target, Start Menu icon, and uninstall cleanup.

The visual smoke should start on the configured second display when available, capture the empty launcher before query input, exercise repeated hide/show activation, and then exercise query expansion, keyboard selection, action mode, Enter, and Settings. GitHub runner screenshots are evidence of the rendering path but may not expose live DWM blur under remote desktop composition. State this limitation honestly in release notes and do not use a runner screenshot to override a physical Windows 11 observation.

## Manual release checklist

Before a beta release, confirm that the workspace version, `Cargo.lock`, installer fallback version, release tag, and artifact metadata agree. Confirm `prerelease: true`, no `(beta)` suffix in the release name, and the expected installer and portable assets are present. Replace generic workflow-generated notes with a detailed English release description before reporting completion.

The release description must include a concise summary of the user-visible changes, the exact validation performed, any runner or DWM limitations, direct installer and portable download links, and a short Windows verification procedure. The installer is the primary download; portable is a secondary option.

## WinGet policy

WinGet manifests must use the lowercase package identifier `m1nuzz.FluxLauncher` and the canonical path `manifests/m/m1nuzz/FluxLauncher/<version>/`. Submit only stable versions. Verify the installer URL, SHA256, schema, installer metadata, and Apps & Features display name against the actual built installer. A manual PR does not require repository secrets. Keep the WinGet PR separate from beta release work and do not update it to a beta version.
