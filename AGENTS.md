# Flux Launcher Agent 指南

# 第一部分 总则与不变量

## 1. 适用范围与优先级

* `AGENTS.md` 是本仓库唯一的 Agent 规则文件，Agent 的改动与交付行为以本文为准；`README.md` 的权威范围见第 13 节。
* 用户在当前任务中的明确指令优先于本文；用户指令若与本文的不变量冲突，先向用户确认，不得擅自绕过。
* 章节编号做过一次分组重排，旧编号映射见 `doc/guide/AGENTS章节编号映射.md`；修改章节编号前须同步该映射（见第 26 节）。

规则冲突时按以下优先级处理，编号越小越优先：

1. Windows 生命周期与安全
2. 用户行为与 UX
3. i18n 与架构
4. 验证与正确性
5. 最小修改
6. 发布与交付
7. 文档与提交

不得为了通过检查而绕过 Windows 生命周期、真实 Acrylic / Mica、i18n、Everything 回落、启动语义、安装器烟雾测试或必要的 Windows 验证。

---

## 2. 项目定位与核心约束

Flux Launcher 是一款使用 Rust 编写的 Windows 11 原生启动器。

### GUI 与渲染

* GUI **只能使用内嵌 `windui` 框架**。
* 禁止引入 WebView、Electron、egui、iced、Tauri 或其他 GUI 框架。
* 应用为常驻系统托盘程序。
* 窗口背景必须通过既有 Win32 + DirectComposition 路径使用真实 Windows DWM Acrylic 或 Mica。
* 必须保持启动器整个表面透明，使系统材质覆盖完整窗口。
* 不得以虚假渐变、不透明卡片、着色渐变或 WCA AccentPolicy 替代系统材质。

### 架构

* 保留 `flux-core`、Flux 应用代码与内嵌 `windui` 后端之间的职责边界。
* 优先进行小范围、平台相关的修改，避免无必要的大规模重写。
* Everything 集成必须保留优雅的不可用回落路径。
* Windows 专属代码应位于合适的平台模块中。
* 在可行时保持非 Windows 目标的跨平台编译能力及现有依赖锁定。

---

## 3. Windows 生命周期与启动行为

* 默认全局热键为 `Alt+Space`，必须保持可配置。
* 重复激活切换启动器可见性；显示后搜索框立即获得焦点。
* “激活时清空查询”、游戏模式保护、全屏热键保护默认启用。
* 应用程序结果优先于普通文件和文件夹。
* 保持既有 Flow 键盘导航：`Up`、`Down`、`Home`、`End`、`Enter`、`Right`、`Left`、`Escape`。

---

## 4. Windows Acrylic / Mica 与生命周期不变量

* `ShowWindow` 必须先于 show 回调中的布局状态修改。
* 可见激活后，首个透明 D2D 帧必须先失效并呈现，再依赖输入或查询状态。
* 反复隐藏 / 显示及窗口尺寸、绘制、合成变化后，Acrylic / Mica 必须保持正常。
* 发布前验证暗 / 亮色可读性、标题与结果行无重叠、选中状态清晰响应，以及系统强调色和自定义调色板回退正常。

---

## 5. 查询历史与 UX 不变量

* 已提交查询持久化到有界、不区分大小写的历史记录，按最新优先。
* `Ctrl+H` 打开历史；`Enter` 或鼠标点击重新运行所选查询。
* 空查询按普通 `Up` 回溯最近查询；`Alt+Up` / `Alt+Down` 前后循环。
* 设置中提供清空历史功能。
* 展开操作栏中始终显示 Provider 状态，不得破坏 Acrylic / Mica，也不得使用不透明容器替代。

---

## 6. Everything 集成

* 安装后始终注册 Everything 文件 / 文件夹 provider；服务不可用时优雅回落为无服务状态。
* 保持现有原生语法支持，如 `ext:zip`、`parent:`、`file:`、`dm:`。
* Everything 不可用时必须保持启动器正常运行，不得崩溃。
* 应用程序结果始终优先于普通 Everything 文件 / 文件夹结果。

---

## 7. Flow 插件与内置能力

* Flow 插件仅支持原生插件和可执行 JSON-RPC 插件。
* 禁止 Python、C# 插件执行。
* Google、Obsidian 必须保留在主可执行文件中。
* 除非用户明确要求独立社区插件宿主，否则不得拆分上述内置能力。

---

## 8. Windows 启动项与安装器启动行为

* 安装器的“Windows 启动时启动”和 `Launch Flux Launcher now` 必须独立，前者默认启用且可取消，后者默认选中。
* 启动注册表命令必须使用 `--startup`。
* `--startup` 必须调用 `windui::start_hidden()`，仅创建托盘进程，不显示搜索窗口；仅全局热键或托盘 `Show launcher` 可显示窗口。
* 安装器烟雾测试必须覆盖默认启动项、`/TASKS=!startup` 禁用路径及隐藏 `--startup` 模式。

---

## 9. 开始菜单快捷方式与图标

* 开始菜单快捷方式必须指向实际安装的可执行文件，并显式引用 Flux Launcher 的 `.ico`。
* 安装目录必须包含多分辨率 `.ico` 资源。
* 安装器烟雾测试必须验证快捷方式目标、图标元数据及实际图标引用，不得仅检查快捷方式文件存在。

---

# 第二部分 语言、文档与提交

## 10. 仓库语言与提交规范

### 源码、文档与发布说明

保留已有内容的原有语言，不做无关翻译或重写。

后续新增或修改内容默认使用中文，包括：

* 源码注释
* 项目文档
* 修复 / 功能文档
* 发布说明

如需英文版本，可使用 `_en` 后缀单独提供，例如：

```text
release-notes.md
release-notes_en.md
```

除明确要求外，不强制已有内容提供英文版本。

### Git Commit

* 遵循 **Conventional Commits 1.0.0**；提交信息使用英文和祈使式描述。
* type 常用 `fix`、`feat`、`docs`、`refactor`、`ci`、`build`、`chore`、`test`。
* scope 默认 `launcher`；改动仅涉及某个独立改动面时可改用该改动面自身的标识（如 `agents`、`ci`、`release`、`smoke`、`windui`）。

格式：

```text
<type>(<scope>): <description>
```

---

## 11. 国际化（i18n）

Flux 使用 `rust-i18n` 处理所有面向用户的 UI 文案。

翻译文件：

```text
crates/flux-launcher/locales/en.yml
crates/flux-launcher/locales/zh-CN.yml
```

其中 `en.yml` 为回退语言。

### 规则

* 所有用户可见 UI 文案必须通过 `t!` 获取，禁止在 UI 代码中硬编码。
* 新增或修改文案时，必须同步更新 `en.yml` 和 `zh-CN.yml`，两者键保持一致。
* 缺失或不支持的翻译统一回退到英文，不得显示原始键名或 panic。
* 启动时使用 `sys-locale` 检测系统语言，并通过 `rust_i18n::set_locale` 设置；未知语言回退到 `en`。
* 保持现有 `i18n!("locales", fallback = "en")` 路径不变，除非同步调整相关配置。
* 版本号、内部标识符、颜色、常量及 stderr 诊断信息无需翻译。
* 键一致性由 `scripts/validate-i18n-keys.ps1` 校验（双向缺键、同文件重复键、空值叶子键与不可解析行逐条报错），本地按第 14 节执行，CI 的「校验 i18n 键一致性」步骤强制；发现方式即该脚本与该 CI 步骤。

---

## 12. 行尾与格式化

* Rust 使用仓库现有 `rustfmt.toml`，格式或行尾问题统一执行 `cargo fmt --all`，禁止手工修改 Rust 行尾。
* 非 Rust 文件如发现 CRLF、行尾损坏或仅因行尾产生的 diff，停止自动编辑，人工规范为 LF，并通过抽样十六进制检查与 `git diff --check` 验证工作区与暂存区。
* 非 Rust 行尾的强制基线是 `.gitattributes` 的 `* text=auto eol=lf`，这是本节规则的发现方式。
* 除非人工处理不可行，否则禁止使用脚本批量重写行尾。
* 仅执行 `git add` 不视为完成修复。

### 导入与调用风格

* crate 内部条目（类型、trait、常量、函数）优先 `use` 引入后以裸名调用，例如 `use crate::settings_state::{record_query_history, LauncherSettingsState};`。
* 允许 `use crate::module;` 后写 `module::item(...)`，也允许在调用处直接写内部路径（如 `crate::settings_state::save_settings`）；两者均为合法形式，存量不强制迁移。
* 禁止重复限定：已 `use` 的条目不得再写完整路径，例如已导入 `Duration` 仍写 `std::time::Duration`。
* 外部 crate 与 `std` 保持整模块导入 + 限定调用（`use std::env;` 后 `env::var_os(...)`）；`use windui::prelude::*;` 等 prelude glob 保持现状。
* 规则说明、门槛覆盖范围与收紧路线见本地记录 `doc/guide/Rust-导入与调用规范.md`（第 13 节）。
* 发现方式：`unused_qualifications` 在 5 个成员 crate 上设为 `deny`（根 `Cargo.toml` 的 `[workspace.lints.rust]` 加各成员 `[lints] workspace = true`），由第 14 节的 clippy 检查在本地与 CI 强制；「未 `use` 同一项即写内部路径」由第 24 节审查发现，如需机械强制可后续启用 `clippy::absolute_paths`（现存写法需先迁移）。

### 函数规模

* 单函数行数不得超过根 `clippy.toml` 的 `too-many-lines-threshold`；该阈值**只降不升**，收紧必须与对应的函数拆分同批提交。
* 函数参数不得超过 `clippy::too_many_arguments` 阈值（默认 7）；超出时引入上下文结构（沿用现有 `*Context` / `*Bundle` 模式）。
* 发现方式：同源的按包 `[lints.clippy]` 门槛（`too_many_lines = "deny"`），由第 14 节 clippy 检查强制。

---

## 13. 文档规则

* `doc/` 下文档使用中文。
* `doc/` 根目录不得直接存放文档，只能包含分类子目录；固定分类：
    * `doc/fix/`：产品修复报告，至少包含实现说明、验证结果、已知限制、用户验证步骤。
    * `doc/feat/`：新增功能说明与使用方式。
    * `doc/plan/`：Plan 模式产出计划与 Act 模式回写。
    * `doc/question/`：工具失败、异常与外部阻塞记录。
    * `doc/test/`：测试脚本说明、探针设计与验证记录。
    * `doc/guide/`：指南、项目概览与参考文档。
    * `doc/upstream/`：vendor / 上游改动记录。
    * `doc/archive/`：已完成并随发布交付的修复 / 功能 / 计划 / 工具失败记录归档，按原分类镜像子目录，仅作历史查阅；活跃文档不得放入。
* 分类目录不存在时创建；新增分类必须在本节登记后再使用。
* 任何修改都必须有文档：产品代码、测试脚本、workflow、i18n、AGENTS.md 自身的改动，均须在对应分类生成或更新文档；修复进 `doc/fix/`，功能进 `doc/feat/`，规划与回写进 `doc/plan/`，测试或脚本调整进 `doc/test/`。
* 无需文档的豁免仅限：纯行尾或 `cargo fmt` 格式化、`temp/` 下草稿、无任何语义变化的重跑；豁免之外不得跳过。
* `doc/` 及其子目录默认仅作本地记录，不加入暂存区或提交，除非项目负责人明确要求；该本地性由 `.gitignore` 中的 `doc/` 条目强制，这是本条的发现方式。
* `doc/` 定时清理按时间线触发，属例行维护，无需单独产出修复 / 功能文档；执行计划的回写小节必须列出清理清单：
    * 修复 / 功能随 beta 或稳定版发布交付后，对应的 `doc/fix/`、`doc/feat/` 文档移入 `doc/archive/<分类>/`；已完成并回写的 `doc/plan/` 计划一并归档，未完成的计划保留原地。
    * `doc/archive/` 单个分类文件数超过 50，或总数超过 100 时，按归档时间线从旧到新整批删除，保留最近两个发布周期的归档；禁止挑选性删除。
    * `doc/guide/`、`doc/upstream/` 为活文档，不参与清理；上游报告被新报告取代且距生成超过 90 天时可移入 `doc/archive/upstream/`。
    * 删除归档不构成「任何修改都必须有文档」的豁免；被删清单写入当次执行计划的回写小节。

### README.md 同步要求

* `README.md`（中文）为权威版本，内容、命令、链接与发布说明以它为准；`README.en.md` 是其英文版本，必须与 `README.md` 保持同步。
* 涉及用户可见功能、默认行为、快捷键、安装方式、发布流程、WinGet、构建命令、产品身份或项目链接的改动，必须检查并在需要时同步更新 `README.md`；仅内部重构、测试或 CI 实现细节变化，且不影响用户使用或项目维护方式时无需修改。
* 更新时保持现有结构，避免无关改写，并确保产品名称、链接、命令和发布说明与当前实现一致，不得引用已删除或改名的 workflow、脚本或产物。
* 未改动 `README.md` 语义时，不对 `README.en.md` 做无关编辑。

---

# 第三部分 验证与发布

## 14. 提交前验证

提交或报告任务完成前按改动范围执行适用检查，检查项与 CI（`ci.yml`）逐项对齐：

* Rust 改动：执行下方全部 cargo 检查。
* 仅文档改动：执行 `git diff --check`。
* PowerShell / workflow / locales 改动：执行下方「PowerShell 脚本验证」与「CI 校验脚本」。

```powershell
cargo fmt --all
cargo fmt --all -- --check
git diff --check
cargo check --workspace --target x86_64-pc-windows-msvc
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace --target x86_64-pc-windows-msvc
cargo build --workspace --release --target x86_64-pc-windows-msvc
```

目标与 CI（`ci.yml`）保持一致，均为 `x86_64-pc-windows-msvc`。本机若已安装 MinGW 工具链，可额外执行 GNU 目标检查，但 CI 不强制。`cargo check` 等命令需要 Cargo 环境时，先按本机方式加载（Windows 上通常已由 rustup 配置，无需 `source` 脚本）。

CI 的 `rust` job 还会执行 `scripts/shortcut-icon-smoke.ps1` 与 `scripts/single-instance-smoke.ps1`（依赖 release 可执行文件）：本地在改动快捷方式图标、单实例或 Everything 进程保护相关路径时执行，其余由 CI 覆盖。

涉及 `windui`、发布打包或安装器的改动，必须执行对应入口：

* `windui`：按 `doc/upstream/` 既有改动记录执行对应校验，并检查 Flux 侧调用契约、生命周期顺序与线程模型。
* 发布打包 / 安装器：执行 `scripts/build-installer.ps1` 与 `scripts/installer-smoke.ps1`，并按第 16 节运行 `Windows 发布` workflow。

### PowerShell 脚本验证

凡改动 `scripts/*.ps1`、`PSScriptAnalyzerSettings.psd1` 或 workflow 内嵌 PowerShell 片段，提交前必须执行以下检查（与 CI 同为按需安装）：

```powershell
Set-PSRepository PSGallery -InstallationPolicy Trusted
if (-not (Get-Module -ListAvailable -Name PSScriptAnalyzer)) {
    Install-Module PSScriptAnalyzer -Scope CurrentUser -Force
}
Invoke-ScriptAnalyzer -Path scripts -Recurse -Settings ./PSScriptAnalyzerSettings.psd1 -Severity Warning,Error
```

输出为空方为通过。同时必须全量解析 `scripts/*.ps1` 语法（PowerShell AST Parser），零错误：

```powershell
$parseErrors = @()
Get-ChildItem scripts/*.ps1 | ForEach-Object {
    $errors = $null
    $null = [System.Management.Automation.Language.Parser]::ParseFile($_.FullName, [ref]$null, [ref]$errors)
    if ($errors) { $parseErrors += $errors }
}
if ($parseErrors.Count -gt 0) {
    $parseErrors | ForEach-Object { Write-Host "$($_.Extent.File):$($_.Extent.StartLineNumber) $($_.Message)" }
    throw "PowerShell 脚本存在语法错误。"
}
```

### CI 校验脚本

改动 `.github/workflows/`、`scripts/*.ps1` 或 `crates/flux-launcher/locales/` 时执行（对应 CI 的四个校验步骤）：

```powershell
./scripts/validate-workflow-secret-references.ps1 -WorkflowDirectory .github/workflows
./scripts/validate-powershell-command-references.ps1 -ScriptsDirectory scripts
./scripts/validate-workflow-input-interpolation.ps1 -WorkflowDirectory .github/workflows
./scripts/validate-i18n-keys.ps1 -LocalesDirectory crates/flux-launcher/locales
```

* 未改动任何 PowerShell 内容时不强制「PowerShell 脚本验证」；只要改动 workflow、`scripts/*.ps1` 或 locales，仍须执行上面四个校验。
* 改动 `PSScriptAnalyzerSettings.psd1` 本身时必须全量复扫，并逐条说明新增例外的理由。
* 禁止为绕过告警新增 ExcludeRules 或内联抑制；确需排除必须在设置文件内以注释写明理由（现有 `PSAvoidUsingWriteHost` 例外即此先例）。
* 上述检查已在 `ci.yml` 的「验证 PowerShell 脚本」与四个「校验 …」步骤中强制；未经本地检查即推送会导致 CI 失败。

任何检查失败都必须先分析日志并判断原因；仅在确认属于环境或瞬时问题后才可重跑，不能因“看起来无关”而忽略失败。

---

## 15. 测试与验证资产规范

适用于 `scripts/*.ps1`、`.github/workflows/` 与 Rust 测试代码。

### 探针与烟雾测试

* 探针必须断言产品不变量（隐藏态近乎零 CPU、热键切换可见性、Acrylic / Mica 生效、启动语义等），不得只检查“进程没崩溃”或“文件存在”。
* 探针必须先建立前置条件再断言，且不得假定前置条件在多次操作之间保持不变；对焦点、可见性这类会被环境改变的状态，须在派发前重新确认，并对竞态场景恢复前置条件后重试，而不是把一次派发结果当作失败。
* 断言失败必须先看日志与 artifact 再定性；定性为竞态或环境问题时，报告须附 run id、trace 证据与「为何不是产品回归」的依据。禁止凭猜测把失败归因于 flake。
* 断言消息必须写明被破坏的不变量，便于从 CI 日志定位。
* 新增或修改烟雾测试须同步三处：脚本本身、workflow 的输入开关与默认值、`doc/` 下的验证记录。

### 测试代码

* 新增功能必须带单元测试；可测逻辑不得只靠手工验证。
* 单元测试与集成测试沿用现有组织方式（crate 内 `tests.rs` 与既有 helper），不引入新的测试框架或外部测试依赖。
* 涉及 Windows 生命周期的行为，测试须覆盖隐藏 / 显示往返，不得只测单次转换。

### Workflow

* workflow 改动必须先做本地可验证的部分（`scripts/*.ps1` AST 解析、PSSA、第 14 节的四个 `validate-*.ps1` 校验脚本），再推送。
* 嵌入 workflow 的 PowerShell 片段受第 14 节 PowerShell 检查约束。
* 新增 workflow 输入须同步默认值、调用方与文档；不得留下只在 CI 中可用而无法本地复现的开关。
* CI 目标平台即为本地检查目标平台，二者必须一致，不得要求本地执行 CI 无法执行的检查。
* workflow 的运行时长必须可控：能用缓存消除的冷编译必须用缓存（`Swatinem/rust-cache`，缓存 key 与其它 workflow 区分），不得重复执行同 HEAD 已由其它 workflow 完成的全量检查；确需同 HEAD 验证结论时改为校验该 workflow 的成功记录，而不是重跑。发布类 workflow 成功运行超过 15 分钟须在计划回写中说明原因或给出优化（发现方式：Actions 运行列表显示每次 run 时长）。
* 构建配置（profile、lto、rustflags 等）变更会使缓存产物失效而缓存 key 不变；GitHub 缓存同 key 不可覆盖，此时必须同步更换缓存 key，否则旧缓存永远无法替换、每次全量重编（发现方式：构建步骤日志的 `Compiling` 计数与缓存 restore / save 判定行）。
* workflow 引用的 secrets / vars 名称必须维持在允许清单内，并由 `scripts/validate-workflow-secret-references.ps1` 在 CI 校验；新增、改名或删除密钥时必须同步该脚本的清单与第 20 节。
* 编辑器提示 “Context access might be invalid: NAME” 属预期：本仓库按需配置密钥，workflow 在运行时自行守卫（无提交凭据时 fail-fast、签名仅限稳定通道），因此不得为了消除提示而创建空密钥或伪造变量。

---

## 16. Windows 发布验证

涉及生命周期、视觉、安装器、启动项、快捷方式、Acrylic / Mica 或启动行为时，必须手工运行 `Windows 发布` workflow，并使用 `release_channel=beta`：

```powershell
gh workflow run windows-release.yml `
  --repo lvxingqi/flux-launcher-cn `
  --ref main `
  -f release_tag=vX.Y.Z-beta.N `
  -f runner_label=windows-latest `
  -f release_channel=beta `
  -f publish_release=false
```

该命令在 PowerShell 中执行，行尾为反引号续行。`release_tag` 必须对应本次实际构建版本（与 Cargo 版本逐字一致），且不得复用已有 release tag。

`publish_release` 默认 false：验证运行只构建、执行完整 smoke 与 UI 捕获并上传产物，不创建 Release；只有确实要发布时才传 `publish_release=true`，并同时提供人工校对的 `release_notes`，否则 workflow 在创建 Release 前失败（见第 19 节）。

该 workflow 起始即校验同 HEAD 的 `ci.yml` 成功记录：CI 缺失、未完成或未成功时立即失败，因此发布前必须先让同 HEAD CI 全绿（见第 14 节），发布 workflow 自身不再重复执行全量测试。

工作流成功前，不得发布或报告 beta 已完成。

工作流必须包含：

* 安装程序与便携版构建
* Windows UI 捕获
* `scripts/installer-smoke.ps1`

安装器烟雾测试必须覆盖第 8、9 节规定的启动项、隐藏启动、快捷方式、图标，以及可执行文件哈希、安装后配置和卸载清理。

### Workflow 运行历史清理

* 远程 `gh` 操作必须显式 `--repo lvxingqi/flux-launcher-cn`：本仓库配置了 upstream remote，`gh` 默认可能解析到 upstream，曾导致对错误仓库执行删除。
* 触发（时间线）：每次发布 workflow 成功完成后，或仓库 `actions/runs` 总数超过 50。
* 保留策略：保留最近 20 条运行（含失败与进行中）；进行中（queued / in_progress）的运行不得删除。
* 删除属例行维护，列入发布收尾步骤执行，并在计划回写中记录删除数量；无需单独文档。

---

## 17. Windows 视觉烟雾测试

* 可用时使用已配置的副显示器，覆盖：空启动、查询前捕获、反复隐藏 / 显示、查询扩展、键盘选择、操作模式、Enter、设置。
* GitHub Windows runner 截图仅作为渲染路径证据，不能替代或推翻实体 Windows 11 的 DWM Acrylic / Mica 观察。
* 发布说明必须如实注明该限制。

---

## 18. 版本与 Release 规范

* 项目版本遵循 **Semantic Versioning 2.0.0（SemVer）**。
* GitHub Release 使用 v 前缀的版本 tag，例如 `v0.1.0`。
* Beta 版本使用 SemVer 预发布标识，Cargo 版本、tag 与安装器显示版本三者一致
  （如 `0.2.0-beta.1` / `v0.2.0-beta.1`）；Inno 的 `AppVersion`/`VersionInfoVersion`
  使用去后缀的纯数字版本（如 `0.2.0`）。稳定版必须是纯数字版本，禁止预发布标识。
* 历史 release / tag 经项目所有者确认删除后，版本序列可重启：重启后的首个 beta 为
  `vX.Y.Z-beta.1`，该系列的稳定版为 `vX.Y.Z`；已删除的历史 release / tag 不得再被
  构建产物、更新器、清单或文档引用。

---

## 19. Beta 发布

* 每次产品修复完成后准备一次**手工 beta 发布**。
* Beta 发布必须通过 `Windows 发布` workflow 的 `release_channel=beta` 生成，且 `prerelease: true`，Release 名称不得包含 `(beta)` 后缀（名称取 tag 本身）；稳定版走同一 workflow 的 `release_channel=stable`，`prerelease: false`。
* 禁止空、重复或无说明发布，也不得由 push、定时任务或内部提交自动创建。
* 发布版本、通道和说明必须由 Agent 主动选择并核对。
* 发布前确认版本、`Cargo.lock`、安装器版本、tag、产物元数据一致，且安装程序与便携版资产均存在。
* 发布说明须使用中文并由人工撰写或校对，包含变更摘要、实际验证、已知限制、runner / DWM 限制、Windows 验证步骤及安装程序 / 便携版直接下载链接。
* 下载优先级：安装程序 > 便携版。

---

## 20. 稳定发布与 WinGet

* 稳定发布必须通过明确的用户指令，并手动运行 `Windows 发布` workflow 的 `release_channel=stable`（beta 与稳定版共用该 workflow，由 `release_channel` 区分）。
* WinGet 仅提交稳定版本，包标识符为 `lvxingqi.FluxLauncherCN`，路径为 `manifests/l/lvxingqi/FluxLauncherCN/<version>/`。
* 提交前根据实际安装程序核实 URL、SHA256、schema、安装器元数据及“应用与功能”名称。
* Beta 不得进入 WinGet；WinGet 自动化不得创建 GitHub Release，且仅在明确启用稳定版策略后才能准备或提交稳定版 PR。
* 未经用户明确要求，不得创建 `WINGET_GITHUB_TOKEN` 或签名密钥。
* 手工 WinGet PR 不需要仓库密钥，且必须与 Beta 发布流程分离。

---

# 第四部分 协作与维护

## 21. 工作方式

* 修改前先检查相关源码、仓库规则、CI、测试、发布记录、安装器、Windows 验证及问题报告；已有日志能回答的问题不得猜测。
* 先确认问题，再做**最小完整修改**，保持现有架构，避免无关重构。
* 多步骤任务先制定涵盖“调研 → 实现 → 验证 → 交付”的计划。
* 在 CI 等待 / 失败、外部评审或 Windows 验证未完成等关键节点及时汇报。
* 能否声称完成以第 25 节完成标准为准。

---

## 22. 工具失败与异常记录

* 工具出现失败、超时、异常返回或部分状态变更时，及时在 `doc/question/` 记录中文 Markdown 报告，包含时间、工具 / 服务、操作、影响、原始错误、诊断与恢复、验证结果。
* `apply_patch` 已禁用，禁止调用。
* 瞬时失败的判定与必须停止的情形见第 23 节；本节只规定失败记录的落盘位置与内容。

---

## 23. Plan / Act 协作

### Plan 模式

* Plan 模式在对话中输出规划结果，规划结果必须包含：任务目标、编号步骤清单、每步验收标准。
* 步骤清单必须编号，便于 Act 模式回写状态。
* 落盘到 `doc/plan/<任务名>.md` 由 Act 第一步完成：Plan 阶段所处工具环境通常禁止写文件，不得把「Plan 期间写文件」当作前提。

### Act 模式

* 开始前必须先查找 `doc/plan/` 下与当前任务对应的最近一份计划文档并阅读，严格按其中步骤执行；找不到时，第一步先把 Plan 输出落盘为 `doc/plan/<任务名>.md`，再按其步骤执行。
* 存在多份相关计划文档时，以最近一份且尚未完成的为准；状态只回写正在执行的那份。
* 不得擅自扩大范围或跳过步骤。

#### Act 可自行解决（不必停止，按步骤回写即可）

* 计划内步骤的实现细节：命名、内部拆分、代码写法，前提是不改变验收标准与对外行为。
* 计划内代码的编译、格式或测试失败：属语法、类型、小逻辑问题，直接修复并重跑。
* 瞬时工具失败：网络抖动、终端截断、CI 偶发超时等，重跑一次并以日志佐证为瞬时；同一失败连续两次则视为不瞬时，转入停止条件。
* 计划已明确授权的操作：步骤中写明命令、范围与验收的破坏性、远程或外部状态变更，可直接执行，无需再次确认。
* 计划必然牵连的机械同步：i18n 键、文档、`Cargo.lock`、行尾与 BOM 规范化。
* 例行清理：按第 13 节与第 16 节的触发条件与保留策略执行 doc 归档与远程 workflow 运行历史删除（保留策略已在规则中写明），并在计划回写中列出清理清单与被删数量。

#### 必须停止并回写「执行阻塞」

满足任一条即停止，禁止猜测或绕过；在计划文档对应步骤下追加「执行阻塞」小节，写清现象、报错、已尝试方法与阻塞点，然后结束本次 Act，等待用户手动切回 Plan 重新规划：

* 出现计划未覆盖的新问题或新需求（含实机反馈的新现象）。
* 计划前提被推翻：依赖的外部状态与假设不符（远程 release / tag 不存在、上游接口或文件结构变化等）。
* 方案不成立：实现后无法满足验收标准，或必须改动计划未列出的公共 API、架构边界、依赖或 vendor 补丁集。
* 需要计划未授权的破坏性或远程操作：删除、覆盖、force push、凭据配置。
* 计划步骤与不变量冲突：Windows 生命周期、真实 Acrylic / Mica、i18n、Everything 回落、安装器语义等（第 1 节优先级高于“完成计划”）。
* 验证无法完成：需要实机或用户参与，或同一检查连续两次失败且无法证明为瞬时。
* 需要用户决策的取舍：版本号、发布时机、功能取舍、对外行为变化。

#### 约束

* Act 不得修改计划文档的步骤定义本身，只能回写步骤状态与阻塞；发现计划本身有误，按上文停止条件处理。
* Act 不得为通过检查新增例外规则或抑制告警（与第 14、15 节一致）。

---

## 24. 代码审查与依赖引入评估

### 审查范围

* 审查覆盖 `crates/flux-core`、`crates/flux-launcher`、`crates/flux-plugin-sdk` 及示例 / 夹具 crate 的全部源码。
* `vendor/windui` 本体不纳入代码审查（上游 vendor 代码，其改动记录与取舍见第 2、21 节与 `doc/upstream/`）；
  但 Flux 侧调用 windui 的接口契约、生命周期顺序与线程模型属于审查范围。

### 审查维度

每次审查至少覆盖：

1. 不变量：第 3–8 节的生命周期、启动语义、Acrylic / Mica、查询历史、Everything 回落、结果优先级，以及第 11 节 i18n 是否被破坏。
2. 架构边界：`flux-core` 是否保持平台无关；Windows 专属代码是否位于合适的平台模块；是否存在越界重复实现。
3. 错误处理与回落：是否存在 panic、静默吞错、缺失不可用回落路径。
4. 并发与线程：跨线程共享状态是否只在约定线程写入（UI 信号仅 UI 线程读写）；追加写是否原子成行。
5. 测试覆盖：可测逻辑是否有单元测试；生命周期行为是否覆盖隐藏 / 显示往返。
6. 重复与残留：同一逻辑多处实现、产品代码中常驻的测试探针代码、对手写平台 API 的重复包装。
7. 性能：查询路径（UI 线程）中是否存在文件系统调用、逐条 I/O 或无界增长。

### 第三方依赖引入门槛

自定义实现只有在以下条件全部满足时才可替换为第三方库：

1. 已证明存在缺陷或明确维护成本（有测试、日志或问题报告支撑），不得仅因“库更专业”而替换。
2. 候选 crate 活跃维护、许可兼容，且不引入 GUI 框架、WebView、异步运行时或与第 2 节冲突的依赖树。
3. 不改变对外行为与既有不变量；任何行为变化必须先有回归测试覆盖再落地。
4. 收益评级达到 ★★★★ 及以上，或经项目负责人确认取舍。
5. 落地后在计划文档中记录前后对比（产物体积、启动开销、测试结果），并同步 `Cargo.lock`。

优先级：优先选择已在当前构建图中编译的 crate（零新增编译成本，确认方式见依赖评估报告），
其次是无新增传递依赖的小 crate；不得为替换一段正确实现而引入重依赖。
若将来需要复杂文本解析，优先考虑构建图中已有的解析器组合子，而非引入新依赖。

### 星级评分体系

评分针对“替换收益”，不是对候选库本身质量的评价：

* ★★★★★：强烈推荐——消除显著缺陷或风险，且几乎无成本。
* ★★★★：推荐——收益明确、成本可控，落地前需验证产物体积与启动开销。
* ★★★：可选——收益中等，需先做取舍与回归测试评估。
* ★★：不建议——收益模糊，或既有实现已足够。
* ★：禁止 / 无意义——与架构约束冲突（如替代内嵌 windui、引入异步运行时或 WebView）。

### 类别级评估结论

完整证据与逐项星级以 `doc/guide/第三方依赖引入评估报告`（活文档）为证据来源，本节只保留审查时直接引用的结论；两者不一致时，先核对报告再修订本节：

* 推荐优先评估（★★★★）：目录遍历（自研递归，vault 扫描无深度上限）改 walkdir 类候选；环境变量展开（两处重复手写）改 Win32 `ExpandEnvironmentStringsW`（零新增依赖）。
* 可选（★★★）：搜索匹配与排序的模糊匹配候选、查询诊断 trace 改 `log` 门面（必须保持既有环境变量契约与文件格式）。
* 维持现状（★★ / ★）：计算器与 frontmatter 解析、图标 LRU 缓存、命名管道插件连接、自更新链路、HTTP 客户端、设置与配置持久化、并发原语、时间日期、图片解码；逐一理由见评估报告。
* 固定禁止（★）：引入异步运行时或 WebView 的替换方向（JSON-RPC 换 RPC 框架、HTTP 换异步全家桶、替代内嵌 windui），与第 2 节冲突。
* 重新评估时更新评估报告并同步本节类别级结论，不写入版本号等时效性快照。

### 评估与审查产出

* 审查结论按第 13 节落文档：缺陷进 `doc/fix/`，改进与评估进 `doc/plan/` 或依赖评估报告，并在评估报告登记结论。
* 评估报告为活文档，存放于 `doc/guide/`；每次评估新增或修订条目时同步本节类别级结论。
* 引入或移除依赖后必须同步 `Cargo.lock`，并按第 14、15 节执行检查与验证。

---

## 25. 完成标准

任务仅在以下条件满足后才能报告为完成：

* 已检查相关代码、CI、发布历史和测试，并完成最小完整修改。
* 必要的 i18n、文档及修复报告已生成或更新。
* 适用的本地检查与 Windows 验证全部通过。
* 涉及生命周期、视觉、安装器或启动行为时，已完成对应 Windows 验证。
* 对属于产品发布范围的修复，已准备对应 Beta 发布，并核实发布元数据、资产和说明。
* 已知限制已如实记录。

在条件满足前，不得声称已完成、已验证、可发布或可交付。

---

## 26. AGENTS.md 自身维护

* 规则修改的理由必须记录：规则调整进 `doc/plan/`，发现的矛盾或失效规则进 `doc/question/` 或对应计划文档。修改本文档本身也属于第 13 节“任何修改都必须有文档”的范围。
* 禁止写入时效性快照：版本号、发布日期、当前 CI / 发布状态、临时路径等易腐内容一律不写入，改为引用当前实现或规则本身。
* 格式规范：文档结构为一个 H1 标题加 `# 第X部分 …` 分部标题；章节标题统一为 `## N. 标题`（编号后有空格）；条目统一使用 `*` 或编号列表，不混用缩进层级，嵌套统一 4 空格缩进；正文与代码围栏之间保留一个空行。
* 修改章节编号前必须全文检查交叉引用（例如“第 8、9 节”“第 14 节”）并同步更新引用，同时更新 `doc/guide/AGENTS章节编号映射.md` 与已跟踪文件中的 `第 N 节` / `§N` 注释（清单见该映射文档）。
* 增量克制：优先修订既有条目，不为单次事件新增规则；新增规则必须能被“违反时如何发现”支撑，否则应写成注意事项而非硬约束。
* 新增规则不得与第 1 节优先级冲突；冲突时应修订既有条目而不是叠加例外。
