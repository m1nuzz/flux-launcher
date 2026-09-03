# Flux Launcher Agent 指南

## 项目范围

Flux Launcher 是一款使用 Rust 编写的 Windows 11 原生启动器。GUI 必须专属使用内嵌的 `windui` 框架。不得引入 WebView、Electron、egui、iced、Tauri 或其他 GUI 框架。

本启动器为常驻托盘的应用程序。必须通过既有的 Win32 与 DirectComposition 路径使用真实的 Windows DWM Acrylic 或 Mica 系统背景。保持启动器整个表面透明，使系统材质填满整个窗口。不得以虚假渐变、不透明卡片、着色渐变或 WCA AccentPolicy 作为主方案来替换该背景。

## 仓库语言与归属

所有 Flux 自有的源码注释、文档与发布说明必须使用英文编写。不得在 Flux 自有的代码中添加中文符号或中文注释。面向用户的应用字符串必须按照下文"国际化"章节所述，通过 rust-i18n 的 `t!` 宏外部化；不得将可见文本直接嵌入 UI 代码。与项目负责人的对话可以使用俄语。

使用简洁的英文提交信息。不得提交 Manus 内部草稿笔记、生成的计划文件、临时截图或无关产物。当面向用户的文档属于所请求的产品变更的一部分时，方可提交。

## 国际化（i18n）

Flux 使用 `rust-i18n` 框架处理所有面向用户的字符串。翻译文件位于 `crates/flux-launcher/locales/`，其中 `en.yml` 为回退语言，`zh-CN.yml` 为简体中文。框架在 `crates/flux-launcher/src/main.rs` 顶部以 `i18n!("locales", fallback = "en")` 初始化。

- 启动器 UI 中的每个用户可见字符串都必须通过 `t!` 宏读取。不得在 UI 代码中硬编码可见文本；常量、颜色、内部标识符与 stderr 诊断信息除外。
- 新增或修改可见字符串时，必须在同一次变更中同时更新 `en.yml` 与 `zh-CN.yml`，使两份语言文件的键保持一致。
- 缺失的翻译必须回退到 `en`；绝不为缺失的语言渲染原始键或触发 panic。
- 启动时使用 `sys-locale` 检测系统语言，并通过 `rust_i18n::set_locale` 应用；不支持或未知的语言回退到 `en`。
- `i18n!` 中的 `locales` 路径是相对于 flux-launcher crate 清单（`CARGO_MANIFEST_DIR`）解析的。除非同步更新宏路径，否则不得将该目录移动到 workspace 根目录。
- 设置字段名、版本字符串及其他内部标识符不参与翻译。

## 必备工作流程

以审慎、手工优先的循环方式开展工作。在做出改动之前，先检查相关源码、工作流、发布历史与既有测试。对于多步骤变更，需制定包含调研、实现、验证与交付阶段的具体计划。当仓库文件、CI 日志、安装程序日志或 Windows 烟雾测试结果能回答问题时，不得凭空猜测。

做出能解决所观察问题的最小完整改动。保留既有架构，避免大范围重写。在关键节点向用户汇报进展，尤其是当 CI 运行器在等待、工作流失败或外部评审在待办时。在相关本地与 Windows 验证通过之前，绝不宣称修复已就绪。

文档规则：每次完成产品修复后，都必须在 `doc/fix/` 下生成对应的 Markdown 修复报告，并遵循现有修复报告的结构。新增产品功能应使用独立的文档分类，例如 `doc/feat/`；目录不存在时应创建。实现说明、验证结果、已知限制和用户验证步骤必须写入对应文档，不得提交临时草稿或生成的无关产物。

每一次完成产品修复之后都必须跟进一份手工准备的 beta 发布。Beta 发布必须使用 `prerelease: true`，且发布名称中不得包含 `(beta)`。发布正文必须在交付前手工撰写或手工校对，并说明变更了什么、测试了什么、已知限制、下载链接以及项目负责人应核实的内容。不得创建空、重复或无说明的发布。不得在每次推送、定时调度或内部提交时自动创建发布。可以手工启动发布工作流来构建 Windows 产物，但发布版本、通道与详细说明必须由 agent 有意挑选并核对。

稳定发布需明确的提升操作或用户指令。Beta 发布不得提交至 WinGet。WinGet 自动化绝不得创建 GitHub release；只有在仅稳定版策略被明确启用时，它才可以准备或提交稳定版清单 PR。除非用户明确要求启用该自动化，否则不得为手工 WinGet PR 请求或创建 `WINGET_GITHUB_TOKEN` 或签名密钥。

## Windows 生命周期与启动行为

默认激活热键为 Alt+Space，且必须保持可配置。重复激活必须切换可见性。显示时搜索框立即获得键盘焦点。激活时清空查询默认启用。游戏模式与全屏热键保护默认启用。应用程序结果必须排在普通文件与文件夹之前。键盘导航必须按既有 Flow 风格行为支持 Up、Down、Home、End、Enter、Right、Left 与 Escape。

安装程序中的 Windows 启动项复选框默认启用，但必须保持用户可选。安装程序还必须显示一个默认选中的安装后 **Launch Flux Launcher now** 选项。这是两个独立选择：安装后立即启动不得与在未来 Windows 登录时启用启动项相混淆。

启动注册表命令必须使用 `--startup`。启动模式必须调用 windui `start_hidden()`，使 Windows 登录时只创建一个运行中的托盘进程，而不显示搜索窗口。必须要求使用全局热键或托盘的 "Show launcher" 操作才能显示搜索窗口。安装程序烟雾测试必须同时验证默认启动项与禁用路径。

开始菜单快捷方式必须指向已安装的可执行文件，并显式引用 Flux Launcher 的 `.ico` 资源。安装程序必须在安装目录中包含多分辨率图标资源。安装程序烟雾测试必须核实快捷方式目标与图标元数据，而不仅是快捷方式文件存在。

## Windows Acrylic 与生命周期不变量

Win32 生命周期对调用顺序敏感。`ShowWindow` 必须在应用 show 回调修改布局状态之前确立可见性。在可见激活之后，第一个透明 D2D 帧必须先标记为失效并予以呈现，然后才能依赖用户输入或查询变更。

任何可见性、尺寸、绘制或合成变更都必须在反复隐藏/显示激活后保留 Acrylic，并在发布前予以验证。结果行必须在暗色与亮色 Acrylic 样本上均保持可读。标题不得与副标题或相邻行重叠。选中状态必须响应式且视觉上唯一。保持 Windows 强调色默认值与自定义调色板回退完好无损。

## UX 不变量

已提交的查询会持久化到一个有界、不区分大小写的历史中。Ctrl+H 必须打开可选的最新优先历史行；Enter 或鼠标点击会重新运行所选查询；在空字段上按普通 Up 键会回溯最近一次查询；Alt+Up 向后循环、Alt+Down 向前循环；且设置可清空历史。Provider 状态必须保持可见于展开的操作栏中，且不改变透明 Acrylic 表面。

Everything 集成在安装后必须作为始终可用的文件与文件夹 provider 工作，并在其不可用时优雅地回落至无服务状态。诸如 `ext:zip`、`parent:`、`file:` 与 `dm:` 等 Everything 原生语法必须保持受支持。应用程序结果必须保持优先于普通 Everything 结果。

Flow 插件支持仅限于原生或可执行的 JSON-RPC 插件。不得添加 Python 或 C# 插件执行。内置 Google 与 Obsidian 功能必须保留在主可执行文件中，除非用户明确请求一个独立的社区插件宿主。

## 依赖与架构

在可行时保持依赖锁定。优先采用小而针对平台的具体改动，而非大范围重写。保留 `flux-core`、Flux 应用代码与内嵌 `windui` 后端之间的分离。Everything 集成必须保留优雅的回落行为。将 Windows 专属代码放在恰当的平台模块之后，并在可行时维持非 Windows 的跨目标编译。

## 必备本地验证

在提交之前，运行适用的质量门禁。标准本地门禁为：

```text
source "$HOME/.cargo/env"
cargo fmt --all
cargo fmt --all -- --check
git diff --check
cargo check --workspace --target x86_64-pc-windows-gnu
cargo clippy -p flux-core -p flux-launcher --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo test -p flux-core
```

如果变更触及内嵌的 `windui`，请同时运行其相关检查。如果变更触及发布打包，请在 Rust 门禁之外运行静态清单或安装程序检查。绝不因某项检查看起来无关就忽视其失败：先检查日志，区分真正的回归与不稳定运行器，并仅在失败可证明为环境性或瞬时性时才重跑。

## Windows 烟雾测试与发布验证

对于生命周期、视觉、安装程序、启动项或快捷方式变更，请手工派发 Windows UI 发布工作流，并显式带 beta 标签与 `release_channel=beta`：

```text
gh workflow run windows-ui-release.yml \
  --repo m1nuzz/flux-launcher \
  --ref main \
  -f release_tag=vX.Y.Z \
  -f runner_label=windows-latest \
  -f release_channel=beta
```

在运行成功之前不得发布或报告 beta。工作流必须构建安装程序与便携版可执行文件，运行 Windows UI 捕获，并运行 `scripts/installer-smoke.ps1`。安装程序烟雾测试必须覆盖默认启动项、`/TASKS=!startup` 禁用、隐藏 `--startup` 模式、可执行文件哈希、安装后配置、开始菜单快捷方式目标、开始菜单图标与卸载清理。

视觉烟雾测试应在可用时于已配置的副显示器上启动，在查询输入前捕获空启动器，演练反复隐藏/显示激活，然后演练查询扩展、键盘选择、操作模式、Enter 与设置。GitHub 运行器截图是渲染路径的证据，但可能无法暴露远程桌面合成下的实时 DWM 模糊。请在发布说明中如实陈述此限制，且不得以运行器截图推翻物理 Windows 11 上的观察。

## 手工发布清单

在 beta 发布之前，确认工作区版本、`Cargo.lock`、安装程序回落版本、发布标签与产物元数据一致。确认 `prerelease: true`、发布名称中无 `(beta)` 后缀，并存在预期的安装程序与便携版资产。在汇报完成之前，用详尽的英文发布说明替换工作流生成的通用说明。

发布说明必须包含对用户可见变更的简洁摘要、所执行的精确验证、任何运行器或 DWM 限制、安装程序与便携版的直接下载链接，以及简短的 Windows 验证步骤。安装程序为主要下载项；便携版为次要选项。

## WinGet 策略

WinGet 清单必须使用小写包标识符 `m1nuzz.FluxLauncher` 与规范路径 `manifests/m/m1nuzz/FluxLauncher/<version>/`。仅提交稳定版本。依据实际构建的安装程序核实安装程序 URL、SHA256、schema、安装程序元数据与"应用与功能"显示名称。手工 PR 不需要仓库密钥。保持 WinGet PR 与 beta 发布工作分离，且不得将其更新为 beta 版本。
