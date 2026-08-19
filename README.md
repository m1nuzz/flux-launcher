# Flux Launcher

<p align="center">
  <img src="assets/logotype.png" alt="Flux Launcher logo" width="520">
</p>

<p align="center">
  <strong>A lightweight, native Windows 11 launcher and file search tool built in Rust.</strong>
</p>

<p align="center">
  <a href="https://github.com/m1nuzz/flux-launcher/releases/latest"><img src="https://img.shields.io/github/v/release/m1nuzz/flux-launcher?label=latest%20release" alt="Latest release"></a>
  <a href="https://github.com/m1nuzz/flux-launcher/actions/workflows/ci.yml"><img src="https://github.com/m1nuzz/flux-launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status"></a>
  <a href="https://github.com/m1nuzz/flux-launcher/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-2ea44f.svg" alt="MIT license"></a>
  <a href="https://ko-fi.com/m1nuz"><img src="https://img.shields.io/badge/Support%20on-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white" alt="Support Flux Launcher on Ko-fi"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built%20with-Rust-orange.svg" alt="Built with Rust"></a>
</p>

<p align="center">
  <a href="https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe">Download</a>
  · <a href="#features">Features</a>
  · <a href="#usage">Usage</a>
  · <a href="#performance">Performance</a>
  · <a href="#plugins">Plugins</a>
  · <a href="#build-from-source">Build</a>
</p>

**Flux Launcher** is a lightweight **Flow Launcher alternative for Windows 11**. It opens with `Alt+Space`, finds applications and files, understands Everything search syntax, launches web searches, supports Obsidian vaults, and remains compatible with native Flow Launcher executable plugins. The interface is built exclusively with **windui** and uses the Windows 11 Acrylic/DWM composition path without a WebView or browser engine.

## Why Flux Launcher

Flux is designed for users who want a fast, keyboard-first Windows launcher with a native look, predictable resource usage, and a small distribution footprint.

| Capability | What Flux provides |
| --- | --- |
| Native Windows UI | Rust and windui only, with Windows 11 Acrylic and a translucent fallback when composition is unavailable |
| App-first search | Installed applications and shortcuts are ranked ahead of ordinary indexed files and folders |
| File search | Everything IPC with support for queries such as `ext:zip`, `parent:`, `file:`, `folder:`, and `dm:today` |
| Built-in providers | Google Search with `g` and Obsidian vault search with `ob` |
| Plugin compatibility | Legacy Flow `Executable`/`Executable_V2` JSON-RPC plugins plus isolated native Rust community plugins |
| Keyboard workflow | Result navigation, action mode, history, copy path, run as administrator, and open file location |
| Windows integration | Global hotkeys, fullscreen-aware Game Mode, Windows accent color, system tray, Recycle Bin commands, monitor selection, and optional Windows startup |

## Install

For most users, install Flux Launcher with the [latest Windows 11 installer](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe). The installer is the recommended option, registers Flux in Windows startup with **Start Flux Launcher automatically with Windows** enabled by default, and adds a Start Menu shortcut. The same setting can be changed later in `Settings > General > Windows startup`.

You can also install or upgrade Flux with WinGet:

```powershell
winget install --id m1nuzz.FluxLauncher --exact
```

If you do not want an installer, download the [latest portable build](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Portable.exe) and run it directly. Portable mode uses the same startup preference; disable `Start Flux automatically with Windows` in Settings if you do not want it registered.

Flux does not require Everything, but Everything is recommended for indexed file and folder search. If it is not installed, Flux can offer the following command from Settings:

```powershell
winget install -e --id voidtools.Everything
```

Flux stores user settings in `%APPDATA%\FluxLauncher\settings.json`. The default activation hotkey is `Alt+Space`, the default monitor is the display containing the mouse cursor, and fullscreen hotkey suppression is enabled by default.

## Usage

| Input | Result |
| --- | --- |
| `Alt+Space` | Show or hide the launcher |
| `Steam`, `Chrome`, or any app name | Search installed applications first |
| `ext:zip`, `.zip`, `.mp4 video` | Search Everything by file extension |
| `g space exploration` | Open a Google search in the default browser |
| `ob project roadmap` | Search Obsidian vaults |
| `Ctrl+H` | Open committed query history |
| `ArrowUp` / `ArrowDown` | Move through results with wraparound |
| `Tab` / `Shift+Tab` | Move through results using Flow-style tab navigation |
| `Enter` | Launch the selected result or execute its plugin action |
| `ArrowRight` | Open actions for the selected result |
| `Ctrl+C` | Copy the selected path when available |
| `Ctrl+R` | Run the selected application as administrator |
| `Escape` | Return from actions or hide the launcher |

Search results remain bounded to 16 items per provider. Applications are deduplicated and ranked before ordinary Everything files. Query history is persisted atomically, deduplicated case-insensitively, and capped at 32 entries.

## Performance

### Flux memory measurements

The following measurements come from the latest successful Windows smoke run, using the same launcher process and the same PowerShell process counters for each state. Values are reported as **working set** and **private bytes**; private bytes are the more useful indicator for the process-owned memory footprint.

| State | Working set | Private bytes |
| --- | ---: | ---: |
| Idle, empty query | **33.75 MiB** | **8.72 MiB** |
| Query active | **41.97 MiB** | **19.23 MiB** |
| History panel | **57.69 MiB** | **24.30 MiB** |

The measurements are point-in-time evidence rather than a universal guarantee. They were captured on a GitHub-hosted Windows Server 2025 runner in [smoke run 32301567507](https://github.com/m1nuzz/flux-launcher/actions/runs/32301567507). Desktop composition, display count, DPI scaling, fonts, drivers, Everything, and installed plugins can change memory usage.

### Comparison with Flow Launcher

Flux is intentionally designed as a **lower-memory Flow Launcher alternative**. Flow Launcher’s own issue tracker contains a maintainer reference of approximately **130–160 MB** as a normal baseline in one configuration, with reports of higher usage after opening Settings, using plugins, or browsing the plugin store [1]. Flux measured **8.72 MiB private bytes while idle** and **19.23 MiB during an active query** in the smoke run above.

This is a directional comparison, not a laboratory apples-to-apples benchmark: the Flow figures are community reports from different machines and configurations, while Flux’s figures are automated CI measurements. The important distinction is that Flux publishes concrete measurements instead of claiming a universal memory number.

## Plugins and providers

Flux uses a hybrid plugin architecture. Google Search and Obsidian are built into `flux-launcher.exe` and add no plugin subprocess. Legacy Flow executable plugins remain supported through bounded newline-delimited JSON-RPC. New community plugins can be written in Rust as `cdylib` DLLs using the stable `flux-plugin-sdk` C ABI.

The native community host is isolated from the UI process and is started only when `%APPDATA%\FluxLauncher\NativePlugins` contains an installed plugin. The same executable runs the host:

```text
flux-launcher.exe --plugin-host <plugin-root>
```

A native plugin package contains `plugin.toml` and its platform-matched DLL. The manifest declares the API version, action keywords, and permissions. Declarative actions include `OpenUrl`, `OpenPath`, and `CopyText`. If the host exits or a plugin crashes, Flux discards native results and retries the host on a later query without terminating the launcher UI.

The repository includes a complete SDK and an example plugin:

| Path | Purpose |
| --- | --- |
| `crates/flux-plugin-sdk` | Stable C ABI types, buffer ownership, manifest validation, permissions, and actions |
| `crates/flux-plugin-example` | Rust `cdylib` example used by the native host smoke test |
| `crates/flow-plugin-fixture` | Native executable Flow JSON-RPC fixture used by compatibility tests |

## Build from source

Flux targets **Windows 11 x64** and uses the `x86_64-pc-windows-msvc` Rust target. Install the stable Rust toolchain and Visual Studio C++ build tools, then run:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --workspace --release --target x86_64-pc-windows-msvc
```

The launcher executable is written to:

```text
target\x86_64-pc-windows-msvc\release\flux-launcher.exe
```

Run portable tests with:

```bash
cargo test -p flux-core -p flux-plugin-sdk
```

The Windows CI workflow additionally runs formatting, Clippy with warnings denied, workspace tests, and the release build. The Windows visual smoke workflow checks launch/hide cycles, Acrylic lifecycle behavior, keyboard selection, Settings, Everything syntax, history, native Flow compatibility, and monitor placement.

## Project status

Flux Launcher is actively developed. The `main` branch may contain improvements that have not yet been packaged into a stable release. Download the [latest installer](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe), or follow development in the [issue tracker](https://github.com/m1nuzz/flux-launcher/issues).

## Support

Support Flux Launcher on [Ko-fi](https://ko-fi.com/m1nuz).

## License

Flux Launcher is distributed under the [MIT License](LICENSE).

## References

| Reference | What it provides |
| --- | --- |
| [Flow Launcher](https://github.com/Flow-Launcher/Flow.Launcher) | Reference for keyboard-first Windows launcher UX, Everything integration, query history, hotkeys, and legacy plugin compatibility |
| [windui](https://github.com/huanfeng/wind-ui-rust) | The native Rust GUI framework used by Flux Launcher |
| [look](https://github.com/kunkka19xx/look) | Reference for the Smooth Caret interaction in the search field |

[1]: https://github.com/Flow-Launcher/Flow.Launcher/issues/2940 "Flow Launcher memory usage discussion"
[2]: https://github.com/Flow-Launcher/Flow.Launcher/blob/dev/README.md "Flow Launcher README"
[3]: https://github.com/matiassingers/awesome-readme "Awesome README examples"
[4]: https://github.com/banesullivan/README "README writing guidance"
[5]: https://www.voidtools.com/support/everything/ipc/ "Everything IPC documentation"
