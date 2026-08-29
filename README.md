# Flux Launcher

> **兼容性说明：** Flux Launcher 目前仅在 Windows 11 上验证过。其他 Windows 版本尚未经过正式确认。

<p align="center">
  <img src="assets/logotype.png" alt="Flux Launcher logo" width="520">
</p>

<p align="center">
  <strong>一款使用 Rust 构建的轻量级原生 Windows 11 启动器与文件搜索工具。</strong>
</p>

<p align="center">
  <a href="https://github.com/m1nuzz/flux-launcher/releases/latest"><img src="https://img.shields.io/github/v/release/m1nuzz/flux-launcher?label=latest%20release" alt="最新版本"></a>
  <a href="https://github.com/m1nuzz/flux-launcher/actions/workflows/ci.yml"><img src="https://github.com/m1nuzz/flux-launcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI 状态"></a>
  <a href="https://github.com/m1nuzz/flux-launcher/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-2ea44f.svg" alt="MIT license"></a>
  <a href="https://ko-fi.com/m1nuz"><img src="https://img.shields.io/badge/Support%20on-Ko--fi-ff5e5b?logo=ko-fi&logoColor=white" alt="在 Ko-fi 上支持 Flux Launcher"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built%20with-Rust-orange.svg" alt="使用 Rust 构建"></a>
</p>

<p align="center">
  <a href="https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe">下载</a>
  · <a href="#features">功能特性</a>
  · <a href="#usage">使用方法</a>
  · <a href="#performance">性能</a>
  · <a href="#plugins">插件</a>
  · <a href="#build-from-source">从源码构建</a>
</p>

**Flux Launcher** 是一款用 Rust 构建的轻量级 **Windows 11 版 Flow Launcher 替代品**。按下 `Alt+Space` 即可唤起，可查找应用程序与文件、理解 Everything 搜索语法、发起网页搜索、支持 Obsidian 笔记库，并兼容原生 Flow Launcher 可执行插件。界面完全基于 **windui** 构建，使用 Windows 11 Acrylic/DWM 合成路径，不依赖 WebView 或浏览器内核。

## 为什么选择 Flux Launcher

Flux 面向希望拥有一款快速、键盘优先、外观原生、资源占用可预测且分发体积小的 Windows 启动器的用户。

| 能力 | Flux 提供的功能 |
| --- | --- |
| 原生 Windows UI | 仅使用 Rust 与 windui，支持 Windows 11 Acrylic，并在合成不可用时提供半透明回退 |
| 应用优先搜索 | 已安装的应用程序与快捷方式排在普通索引文件与文件夹之前 |
| 文件搜索 | Everything IPC，支持 `ext:zip`、`parent:`、`file:`、`folder:`、`dm:today` 等查询 |
| 内置 provider | Google 搜索（`g`）与 Obsidian 笔记库搜索（`ob`） |
| 插件兼容 | 兼容旧版 Flow `Executable`/`Executable_V2` JSON-RPC 插件，以及隔离的原生 Rust 社区插件 |
| 键盘工作流 | 结果导航、操作模式、历史记录、复制路径、以管理员身份运行、打开文件位置 |
| Windows 集成 | 全局热键、全屏感知的游戏模式、Windows 强调色、系统托盘、回收站命令、显示器选择、可选的 Windows 开机自启 |

## 安装

对于大多数用户，推荐使用[最新的 Windows 11 安装程序](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe)安装 Flux Launcher。安装程序是推荐选项，会默认勾选 **Start Flux Launcher automatically with Windows**（随 Windows 自动启动 Flux）并创建开始菜单快捷方式。该设置之后可在 `Settings > General > Windows startup` 中更改。

也可以通过 WinGet 安装或升级 Flux：

```powershell
winget install --id m1nuzz.FluxLauncher --exact
```

如果不希望使用安装程序，可下载[最新的便携版](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Portable.exe)直接运行。便携版使用相同的开机自启偏好；如果不希望注册开机自启，可在 Settings 中关闭 `Start Flux automatically with Windows`。

Flux 不强制要求 Everything，但推荐安装它用于索引文件与文件夹搜索。如果未安装，Flux 可在 Settings 中提供以下命令：

```powershell
winget install -e --id voidtools.Everything
```

Flux 将用户设置存储在 `%APPDATA%\FluxLauncher\settings.json`。默认激活热键为 `Alt+Space`，默认显示器为鼠标光标所在的显示器，且默认启用全屏热键抑制。稳定版更新检查默认开启，每 24 小时运行一次，错过检查间隔后会立即补检。Flux 只检查 GitHub 稳定版本，忽略 beta/prerelease 版本。在 `Settings > General > Updates` 中可更改检查间隔，并选择"安装前询问"或"自动安装稳定更新"。

## 使用方法

| 输入 | 结果 |
| --- | --- |
| `Alt+Space` | 显示或隐藏启动器 |
| `Steam`、`Chrome` 或任意应用名 | 优先搜索已安装的应用程序 |
| `ext:zip`、`.zip`、`.mp4 video` | 按文件扩展名搜索 Everything |
| `g space exploration` | 在默认浏览器中打开 Google 搜索 |
| `ob project roadmap` | 搜索 Obsidian 笔记库 |
| `Ctrl+H` | 打开已提交的查询历史 |
| `ArrowUp` / `ArrowDown` | 循环移动选择结果 |
| `Tab` / `Shift+Tab` | 使用 Flow 风格的 Tab 导航移动选择结果 |
| `Enter` | 启动选中的结果或执行其插件操作 |
| `ArrowRight` | 打开选中结果的操作菜单 |
| `Ctrl+C` | 可用时复制选中路径 |
| `Ctrl+R` | 以管理员身份运行选中的应用程序 |
| `Escape` | 从操作返回或隐藏启动器 |

### 键盘布局与输入法（IME）

Flux 使用 windui 提供的 Unicode Win32 输入路径。普通文本、Unicode `WM_IME_CHAR` 结果以及已提交的 `WM_IME_COMPOSITION` 结果都经过同一个 UTF-16 解码器，因此中文字符和增补平面 Unicode 文本遵循相同的焦点与代理对行为。输入法组合窗口在组合进行期间定位于搜索光标的焦点位置。`Settings > General > Start typing in English and restore the previous layout on hide`（开始输入时切换英文布局，隐藏时恢复之前的布局）选项仍然可用；禁用该选项可让 Windows 保留用户选择的输入法。

如果键盘输入无法到达搜索框，可通过 PowerShell 为单次会话启用诊断：

```powershell
$env:FLUX_INPUT_TRACE_FILE = Join-Path $env:TEMP "flux-input-trace.log"
.\flux-launcher.exe
```

跟踪信息仅包含消息名称、HWND/线程关系、活动 HKL 标识符、IME 组合状态与路由标志。它绝不记录键入字符、查询文本、剪贴板内容或私有文件路径。诊断结束后移除该环境变量即可恢复默认的零跟踪路径。

搜索结果每个 provider 最多 16 条。应用程序会去重并排在普通 Everything 文件之前。查询历史原子化持久化、不区分大小写去重，且上限为 32 条。

## 性能

### Flux 内存测量

以下测量来自最近一次成功的 Windows smoke 运行，是进程自有内存占用的有用指标。

| 状态 | Working set | Private bytes |
| --- | ---: | ---: |
| 空闲、空查询 | **33.75 MiB** | **8.72 MiB** |
| 查询进行中 | **41.97 MiB** | **19.23 MiB** |
| 历史面板 | **57.69 MiB** | **24.30 MiB** |

这些测量是特定时间点的证据，而非普遍保证。它们采集自 GitHub 托管的 Windows Server 2025 运行器（[smoke run 32301567507](https://github.com/m1nuzz/flux-launcher/actions/runs/32301567507)）。桌面合成、显示器数量、DPI 缩放、字体、驱动、Everything 以及已安装的插件都可能改变内存占用。

### 与 Flow Launcher 的对比

Flux 被有意设计为**内存更低的 Flow Launcher 替代品**。Flow Launcher 自己的 issue 跟踪器中，维护者在某一配置下引用了约 **130–160 MB** 的常规基线，并报告了打开 Settings、使用插件或浏览插件商店后更高的占用 [1]。Flux 在上述 smoke 运行中测得**空闲时私有字节 8.72 MiB**、**查询进行中 19.23 MiB**。

这是方向性对比，而非实验室的逐项基准：Flow 的数据来自不同机器与配置的社区报告，而 Flux 的数据是自动化 CI 测量。重要区别在于 Flux 发布了具体测量值，而不是宣称一个通用的内存数字。

## 插件与 provider

Flux 采用混合插件架构。Google 搜索与 Obsidian 内置于 `flux-launcher.exe`，不产生插件子进程。旧版 Flow 可执行插件仍通过有界的换行分隔 JSON-RPC 获得支持。新的社区插件可以用 Rust 编写为 `cdylib` DLL，使用稳定的 `flux-plugin-sdk` C ABI。

原生社区宿主与 UI 进程隔离，仅当 `%APPDATA%\FluxLauncher\NativePlugins` 中存在已安装的插件时才启动。同一可执行文件充当宿主：

```text
flux-launcher.exe --plugin-host <plugin-root>
```

原生插件包包含 `plugin.toml` 及其平台匹配的 DLL。清单声明 API 版本、动作关键词与权限。声明式动作包括 `OpenUrl`、`OpenPath` 与 `CopyText`。如果宿主退出或插件崩溃，Flux 会丢弃原生结果并在后续查询中重试宿主，而不终止启动器 UI。

仓库包含完整的 SDK 与示例插件：

| 路径 | 用途 |
| --- | --- |
| `crates/flux-plugin-sdk` | 稳定的 C ABI 类型、缓冲区所有权、清单校验、权限与动作 |
| `crates/flux-plugin-example` | Rust `cdylib` 示例，用于原生宿主 smoke 测试 |
| `crates/flow-plugin-fixture` | 用于兼容性测试的原生可执行 Flow JSON-RPC 夹具 |

## 从源码构建

Flux 面向 **Windows 11 x64**，使用 `x86_64-pc-windows-msvc` Rust 目标。安装稳定版 Rust 工具链与 Visual Studio C++ 构建工具，然后运行：

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --workspace --release --target x86_64-pc-windows-msvc
```

启动器可执行文件输出到：

```text
target\x86_64-pc-windows-msvc\release\flux-launcher.exe
```

运行便携测试：

```bash
cargo test -p flux-core -p flux-plugin-sdk
```

Windows CI 工作流还会运行格式化、Clippy（警告视为错误）、workspace 测试与 release 构建。Windows 视觉 smoke 工作流检查启动/隐藏循环、Acrylic 生命周期行为、键盘选择、Settings、Everything 语法、历史记录、原生 Flow 兼容性与显示器位置。

## 发布通道

自动 Windows 发布任务**默认发布 beta/prerelease 构建**。Beta 构建用于测试，Flux 稳定版更新器会忽略它们。要获得面向 SmartScreen 的稳定版发布，维护者应在配置签名密钥后以 `release_channel=stable` 运行 `Windows UI release` 工作流；该路径会对二进制签名、构建安装程序并生成 WinGet 清单包。旧版 `Promote stable release` 工作流只更改 GitHub release 元数据，不得用于面向 WinGet 的未签名构建。稳定版更新器只消费已发布且 `prerelease: false`、`draft: false` 的 GitHub releases，并在启动前验证安装程序资产。

## WinGet 与 SmartScreen

Flux 在 [`packaging/winget/manifests`](packaging/winget/manifests) 下包含一个 schema 1.12 的多文件 WinGet 清单种子，并在 [`scripts/generate-winget-manifest.ps1`](scripts/generate-winget-manifest.ps1) 提供生成器。社区仓库需要向 [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) 提交单独的 pull request；向本仓库添加文件只是准备提交，并不会自动把 Flux 发布到 WinGet。清单针对稳定、版本明确的 GitHub release 资产，而非 beta 或可变的 `latest` URL。

发布工作流仅在配置了加密的 `WINDOWS_SIGNING_CERTIFICATE_BASE64` 与 `WINDOWS_SIGNING_CERTIFICATE_PASSWORD` 密钥时才对稳定版 Windows 产物签名。Beta 构建无需签名即可使用，因为它们是测试产物。Authenticode 签名提供已验证的发布者身份，并有助于跨发布积累信誉，但微软 SmartScreen 在文件哈希或发布者信誉积累足够的干净下载历史之前，仍可能对新文件显示初始警告。验证与发布签名流程见 [`packaging/winget/README.md`](packaging/winget/README.md)。

## 项目状态

Flux Launcher 正在积极开发中。`main` 分支可能包含尚未打包到稳定版中的改进。下载[最新的安装程序](https://github.com/m1nuzz/flux-launcher/releases/latest/download/FluxLauncher-Setup.exe)，或在 [issue 跟踪器](https://github.com/m1nuzz/flux-launcher/issues)中关注开发进展。

## 支持

在 [Ko-fi](https://ko-fi.com/m1nuz) 上支持 Flux Launcher。

## 许可证

Flux Launcher 依据 [MIT License](LICENSE) 分发。

## 参考资料

| 参考 | 提供内容 |
| --- | --- |
| [Flow Launcher](https://github.com/Flow-Launcher/Flow.Launcher) | 键盘优先的 Windows 启动器 UX、Everything 集成、查询历史、热键与旧版插件兼容性的参考 |
| [windui](https://github.com/huanfeng/wind-ui-rust) | Flux Launcher 使用的原生 Rust GUI 框架 |
| [look](https://github.com/kunkka19xx/look) | 搜索框平滑光标（Smooth Caret）交互的参考 |
| [Windows Acrylic material](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic) | Flux Launcher 使用的 Windows 11 Acrylic/DWM 背景参考；Flux 使用 Acrylic 而非 Mica |

[1]: https://github.com/Flow-Launcher/Flow.Launcher/issues/2940 "Flow Launcher 内存占用讨论"
[2]: https://github.com/Flow-Launcher/Flow.Launcher/blob/dev/README.md "Flow Launcher README"
[3]: https://github.com/matiassingers/awesome-readme "Awesome README 示例"
[4]: https://github.com/banesullivan/README "README 编写指南"
[5]: https://www.voidtools.com/support/everything/ipc/ "Everything IPC 文档"
[6]: https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic "Windows Acrylic 材质"
