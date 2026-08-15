# Flux Launcher

<p align="center">
  <img src="assets/logotype.jpg" alt="Flux Launcher logo" width="392">
</p>

Flux Launcher is a lightweight Windows launcher written in Rust with **windui** as its only GUI framework. It combines a compact Spotlight-style search surface with Everything IPC, native Flow Launcher executable plugins, configurable global hotkeys, fullscreen-aware Game Mode, a system tray menu, and a windui-native Settings panel.

The primary interaction is intentionally minimal. With an empty query, Flux shows only a short dark translucent search strip. After at least one character is entered, the native popup expands and displays a bounded, ranked result list. The frameless Windows path uses a popup-style alpha-aware client surface, the Windows system backdrop API, and a DirectComposition-compatible path where supported; it does not retain the opaque native title-bar/client frame that caused the previous white rectangle.

## Features

| Area | Implementation |
| --- | --- |
| GUI | Rust + windui only; no WebView, browser engine, or secondary UI toolkit |
| Window material | Windows system Mica request with safe fallback and Acrylic-capable backend path |
| Search | Built-in command palette, bounded eight-result pipeline, Everything IPC provider, and native Flow plugin provider |
| Ranking | Exact/prefix application matches and executable/shortcut results are ranked ahead of ordinary indexed files and folders |
| Keyboard UX | Up/Down/Home/End select results, Enter launches, Right opens action mode, Left/Escape returns, and actions support Open, Copy path, Copy name, and native plugin execution |
| Query layout | Compact 72 dp search strip when empty; expanded 400 dp result surface after typing |
| Smooth Caret | Optional ease-out visual caret transition with configurable duration; IME coordinates remain exact |
| Global hotkey | Configurable modifier/key combination, default `Alt+Space` |
| Game Mode | Fullscreen suppression enabled by default, manual toggle through `Ctrl+F12` and the tray |
| Flow plugins | `Executable` and `Executable_V2` newline-delimited JSON-RPC plugins only |
| Tray | Left-click show action and right-click menu for Show launcher, Settings, Game Mode, and Exit; uses the transparent `assets/ico.png` branding icon |
| Settings | Hotkey editor, fullscreen protection, Game Mode, Smooth Caret, caret duration, and atomic JSON persistence |
| License | MIT |

## Requirements

Flux Launcher targets **Windows 11 x64** and is built for the `x86_64-pc-windows-msvc` target. Everything is optional: when the Everything service is not available, Flux keeps the built-in and plugin providers active and reports a graceful fallback state. Flow plugins must be native executable plugins; Python and C# plugin runtimes are intentionally outside the MVP scope.

The repository CI uses Windows Server 2025 for automated launch, render, input, compositor-path, plugin, and screenshot smoke tests. A Server runner is useful for repeatable automation, but its desktop composition and wallpaper treatment are not a visual substitute for a physical Windows 11 desktop. The exact dark Mica tint and wallpaper blending should therefore be validated on Windows 11 hardware.

## Build

Install the stable Rust toolchain and the MSVC build tools, then build the Windows release binary:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --workspace --release --target x86_64-pc-windows-msvc
```

The optimized release profile uses size optimization, link-time optimization, one codegen unit, panic abort, and symbol stripping. The resulting executable is:

```text
target\x86_64-pc-windows-msvc\release\flux-launcher.exe
```

To run the core portable tests on a host with a native Linux desktop backend available:

```bash
cargo test -p flux-core
```

The Windows CI job additionally runs formatting, clippy with warnings denied, the workspace tests, and the Windows release build. The Linux sandbox used for development does not provide the GTK or XDG-portal backend required by windui's optional `rfd` file-dialog dependency, so the full windui test binary is compiled for the Windows target and executed by Windows CI instead.

## Configuration

Settings are stored atomically in:

```text
%APPDATA%\FluxLauncher\settings.json
```

The default activation combination is `Alt+Space`. The default policy suppresses activation while another application occupies the full monitor bounds and enables Game Mode protection. Smooth Caret is enabled by default for the launcher search field with a 95 ms transition.

The environment variable below opens the Settings panel directly and is intended for smoke testing:

```powershell
$env:FLUX_OPEN_SETTINGS = "1"
.\flux-launcher.exe
```

## Keyboard navigation and actions

The search field remains focused while the result list is navigated. `ArrowUp` and `ArrowDown` move the selection with wraparound, while `Home` and `End` jump to the first and last result. `Enter` launches a selected file, folder, executable, or shortcut, and executes a native Flow plugin result when the plugin supplies an action.

`ArrowRight` enters an action surface for the selected result. `ArrowUp` and `ArrowDown` select an action, `Enter` executes it, and `ArrowLeft` or `Escape` returns to the result list. The initial action set includes opening a path, copying a path, copying a result name, and executing the action supplied by an `Executable`/`Executable_V2` plugin. Mouse selection remains supported and uses the same result/action model.

## Everything integration

Flux uses the Everything IPC protocol through the `everything-ipc` crate. Requests are dispatched in a background worker, use a short timeout, reject stale query sequences, and return at most eight results. If Everything is not running or cannot answer within the timeout, Flux remains usable without it.

Install Everything from [voidtools](https://www.voidtools.com/) and leave its IPC service enabled to obtain indexed file and folder results. No Everything installation is required for the built-in palette or Flow plugin results.

## Native Flow plugins

Flux discovers manifests from `%APPDATA%\FluxLauncher\Plugins` and from the directory specified by `FLUX_PLUGIN_DIR`. The host accepts only Flow Launcher manifests whose language is `Executable` or `Executable_V2`. For each query, Flux starts the executable, sends a newline-delimited JSON-RPC request, reads bounded results with a timeout, and terminates the short-lived process after the response.

A minimal manifest has the following shape:

```json
{
  "ID": "example.native",
  "Name": "Example Native Plugin",
  "Description": "A native Flow Launcher plugin",
  "ActionKeyword": "",
  "Language": "Executable_V2",
  "Execute": "example-native.exe"
}
```

The repository contains a Rust fixture under `crates/flow-plugin-fixture` and a CI smoke installation under `tests/fixtures/flow-native-plugin`. Native plugins should keep query handling bounded and return standard Flow Launcher JSON-RPC result objects.

## Architecture

The workspace separates portable behavior from Windows integration:

| Crate or directory | Responsibility |
| --- | --- |
| `crates/flux-core` | Settings persistence, hotkey policy, Game Mode policy, search models, and Flow wire models with unit tests |
| `crates/flux-launcher` | windui application, native Windows integrations, Everything worker, plugin host, tray, and launch actions |
| `crates/flow-plugin-fixture` | Native executable Flow JSON-RPC fixture used by integration smoke tests |
| `vendor/windui` | Pinned local windui fork containing the Mica/DirectComposition seam, runtime window sizing, and Smooth Caret support |
| `scripts/capture-mica.ps1` | Proactive Windows screenshot, input, plugin, Settings, and memory smoke harness |
| `assets/logotype.jpg` / `assets/ico.png` | Repository branding artwork and transparent tray icon asset |

The UI keeps result state bounded and uses background workers only for external providers. The compact empty state also reduces the rendered surface and memory pressure while the launcher is idle.

## Smoke and memory evidence

The latest successful Windows smoke run is available at [GitHub Actions run 31861928506](https://github.com/m1nuzz/flux-launcher/actions/runs/31861928506). It verifies the compact empty state, typed-query expansion, ranked results, ArrowDown selection, Right-arrow action mode, Enter action execution, native Flow fixture output, Settings rendering, the requested system-backdrop path, and the following process samples from Windows Server 2025:

| State | Working set | Private bytes |
| --- | ---: | ---: |
| Idle, empty query | approximately 29.5 MiB | approximately 13.4 MiB |
| Query active | approximately 36.4 MiB | approximately 20.3 MiB |

These are point-in-time smoke measurements, not a formal performance guarantee. The query sample includes the expanded UI and a native plugin response. Memory usage can vary with Windows composition, display scale, fonts, GPU driver, plugin behavior, and Everything availability.

## Release

The first MVP release is **Flux Launcher v0.1.0**. The current main branch additionally contains the translucent popup, app-first ranking, keyboard navigation, and action-mode corrections described above; the next patch release will carry these changes as a downloadable Windows x64 artifact. See the [GitHub Releases page](https://github.com/m1nuzz/flux-launcher/releases) for the available binary and English release notes.

## License

Flux Launcher is distributed under the MIT License. See [LICENSE](LICENSE).

## References

[1]: https://github.com/m1nuzz/flux-launcher "Flux Launcher repository"
[2]: https://www.voidtools.com/support/everything/ipc/ "Everything IPC documentation"
[3]: https://www.flowlauncher.com/docs/ "Flow Launcher documentation"
[4]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/system-backdrop-controller "Windows system backdrop documentation"
[5]: https://learn.microsoft.com/en-us/windows/win32/directcomp/directcomposition-portal "DirectComposition documentation"
