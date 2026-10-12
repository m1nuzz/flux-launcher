#Requires -Version 7
<#
.SYNOPSIS
    跟踪 vendored windui 与上游的差距（只读）。
.DESCRIPTION
    输出落后量、逐版本增量、提交主题分布与冲突面重算，供
    doc/upstream/windui-上游更新跟踪.md 复核。
    不修改任何文件、不接入 CI；需要 gh 与 git 在 PATH 中。
    已知限制：GitHub compare 单次最多返回 250 条 commits 与 300 个 files，
    因此按相邻 tag 逐段查询后累加；整段 files 数达到上限时给出警告。
.PARAMETER BaselineTag
    vendored 基线 tag，默认 v0.12.0。
.PARAMETER LatestTag
    上游最新 tag；留空取 semver 最大的 tag。
.PARAMETER UpstreamRepo
    gh api 仓库路径，默认 repos/huanfeng/wind-ui-rust。
.PARAMETER VendorPath
    仓库内 vendored 路径，默认 vendor/windui。
.EXAMPLE
    pwsh -NoProfile -File ./scripts/check-windui-upstream.ps1
.EXAMPLE
    pwsh -NoProfile -File ./scripts/check-windui-upstream.ps1 -LatestTag v0.21.0
#>
param(
    [Parameter()][string]$BaselineTag = 'v0.12.0',
    [Parameter()][string]$LatestTag = '',
    [Parameter()][string]$UpstreamRepo = 'repos/huanfeng/wind-ui-rust',
    [Parameter()][string]$VendorPath = 'vendor/windui'
)

$RepoRoot = Split-Path -Parent $PSScriptRoot

function Get-GhJson {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Endpoint,
        [Parameter()][string]$Jq = ''
    )
    if ($Jq -ne '') {
        $out = & gh api $Endpoint --jq $Jq 2>&1
    }
    else {
        $out = & gh api $Endpoint 2>&1
    }
    if ($LASTEXITCODE -ne 0) {
        throw "gh api 失败（exit=$LASTEXITCODE）：$Endpoint；$out"
    }
    return $out
}

function Get-VersionString {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$TagName)
    if ($TagName -notmatch '^v(\d+\.\d+\.\d+)$') {
        throw "tag 非 vX.Y.Z 形式：$TagName"
    }
    return [version]$Matches[1]
}

function Get-CompareTotal {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Range,
        [Parameter(Mandatory = $true)][string]$Repo
    )
    # 计数走服务端 total_commits（含 merge），不受本地 stdout 捕获影响。
    return [int](Get-GhJson -Endpoint "$Repo/compare/$Range" -Jq '.total_commits')
}

function Get-CommitKind {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Range,
        [Parameter(Mandatory = $true)][string]$Repo
    )
    # 提交主题首词在 jq 服务端切出（纯 ASCII），中文正文不跨 stdout 边界，
    # 避免本地捕获层丢弃中文行导致主题分布缩水。jq 的 capture 无匹配时报错，用 try/catch 兜底。
    $jq = '.commits[].commit.message | split("\n")[0] as $s | try ($s | capture("^(?<k>[a-zA-Z]+)").k) catch "other"'
    return @(Get-GhJson -Endpoint "$Repo/compare/$Range" -Jq $jq)
}

# --- 解析 tag 列表（仅取 vX.Y.Z 正式 tag） ---
$allTags = @(Get-GhJson -Endpoint "$UpstreamRepo/tags" -Jq '.[].name' | Where-Object {
    $_ -match '^v\d+\.\d+\.\d+$'
})
$versions = @($allTags | ForEach-Object { Get-VersionString -TagName $_ } | Sort-Object)
if ($versions.Count -eq 0) {
    throw '上游没有可用的 vX.Y.Z tag，无法跟踪。'
}
$baseVersion = Get-VersionString -TagName $BaselineTag
$baseIndex = [array]::IndexOf($versions, $baseVersion)
if ($baseIndex -lt 0) {
    throw "基线 $BaselineTag 不在上游 tag 列表中。"
}
if ($LatestTag -eq '') {
    $headVersion = $versions[-1]
}
else {
    $headVersion = Get-VersionString -TagName $LatestTag
}
if ($headVersion -le $baseVersion) {
    throw "最新版本 $headVersion 不高于基线 $baseVersion。"
}

# --- 逐版本分组：vX.Y.Z -> vX.Y.Z 的提交数与主题清单 ---
$versionRows = @()
$topicCounts = @{}
$totalCommits = 0
for ($i = $baseIndex; $i -lt $versions.Count - 1; $i++) {
    $from = $versions[$i]
    $to = $versions[$i + 1]
    if ($to -gt $headVersion) { break }
    $range = "v$from...v$to"
    $count = Get-CompareTotal -Range $range -Repo $UpstreamRepo
    $totalCommits += $count
    $versionRows += "v$from -> v$to : $count"
    foreach ($kind in (Get-CommitKind -Range $range -Repo $UpstreamRepo)) {
        if (-not $topicCounts.ContainsKey($kind)) { $topicCounts[$kind] = 0 }
        $topicCounts[$kind] += 1
    }
}

# --- 冲突面重算：本地补丁文件（对基线 tree SHA） ∩ 上游已改文件 ---
function Get-UpstreamTreeMap {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Repo,
        [Parameter(Mandatory = $true)][string]$RefName
    )
    $map = @{}
    $rows = @(Get-GhJson -Endpoint "$Repo/git/trees/$RefName`?recursive=1" -Jq '.tree[] | select(.type == "blob") | .path + " " + .sha')
    foreach ($row in $rows) {
        $idx = $row.LastIndexOf(' ')
        if ($idx -gt 0) {
            $map[$row.Substring(0, $idx)] = $row.Substring($idx + 1)
        }
    }
    return $map
}

$baseTag = "v$baseVersion"
$headTag = "v$headVersion"
$baseTree = Get-UpstreamTreeMap -Repo $UpstreamRepo -RefName $baseTag
$headTree = Get-UpstreamTreeMap -Repo $UpstreamRepo -RefName $headTag
$upChanged = @()
foreach ($path in $baseTree.Keys) {
    if (-not $headTree.ContainsKey($path) -or $headTree[$path] -ne $baseTree[$path]) {
        $upChanged += $path
    }
}
foreach ($path in $headTree.Keys) {
    if (-not $baseTree.ContainsKey($path)) {
        $upChanged += $path
    }
}

$oursPatched = @()
$lsOut = @(git -C $RepoRoot ls-tree -r HEAD -- $VendorPath)
foreach ($line in $lsOut) {
    if ($line -match '^\d+ \w+ ([0-9a-f]{40})\t(.+)$') {
        $sha = $Matches[1]
        $full = $Matches[2]
        $prefix = "$VendorPath/"
        if (-not $full.StartsWith($prefix)) { continue }
        $rel = $full.Substring($prefix.Length)
        if ($baseTree.ContainsKey($rel) -and $baseTree[$rel] -ne $sha) {
            $oursPatched += $rel
        }
    }
}
$conflict = @($oursPatched | Where-Object { $upChanged -contains $_ })
$oursOnly = @($oursPatched | Where-Object { $upChanged -notcontains $_ })

# --- main 领先最新 tag 的未发版提交 ---
$mainAheadCount = Get-CompareTotal -Range "$headTag...main" -Repo $UpstreamRepo
$mainAheadKinds = @(Get-CommitKind -Range "$headTag...main" -Repo $UpstreamRepo)

# --- 输出 ---
Write-Host "=== windui 上游跟踪（只读）==="
Write-Host "基线 tag: $baseTag（vendored 起点）"
Write-Host "最新 tag: $headTag"
Write-Host "落后总量: $totalCommits 提交（逐版本累加）"
Write-Host "main 未发版: $mainAheadCount 提交"
Write-Host ""
Write-Host "--- 逐版本分组 ---"
foreach ($row in $versionRows) {
    Write-Host "  $row"
}
Write-Host ""
Write-Host "--- 主题分布（按提交首词聚合）---"
foreach ($key in ($topicCounts.Keys | Sort-Object)) {
    Write-Host "  $key : $($topicCounts[$key])"
}
Write-Host ""
Write-Host "--- main 未发版主题 ---"
foreach ($s in $mainAheadKinds) {
    Write-Host "  $s"
}
Write-Host ""
Write-Host "--- 冲突面 ---"
Write-Host "  本地补丁文件（内容异于 $baseTag）: $($oursPatched.Count)"
Write-Host "  上游已改文件（$baseTag -> $headTag）: $($upChanged.Count)"
Write-Host "  交集（冲突面）: $($conflict.Count) / $($oursPatched.Count)"
if ($oursOnly.Count -gt 0) {
    Write-Host "  仅本地改（上游未动）: $($oursOnly -join ', ')"
}
foreach ($p in ($conflict | Sort-Object)) {
    Write-Host "  BOTH: $p"
}
Write-Host ""
Write-Host "windui 上游跟踪完成：落后 $totalCommits 提交，冲突面 $($conflict.Count)/$($oursPatched.Count)。"
