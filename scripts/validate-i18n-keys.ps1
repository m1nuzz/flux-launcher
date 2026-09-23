#Requires -Version 7
<#
.SYNOPSIS
    Validates that the locale files declare the same translation keys.

.DESCRIPTION
    AGENTS.md 第 11 节要求 en.yml 与 zh-CN.yml 两者的键保持一致：缺键在运行时只会
    静默回退到英文或显示空串，编辑器与 cargo 检查都不会发现。本脚本就是这条规则的
    发现方式——本地按第 14 节执行，CI 的「校验 i18n 键一致性」步骤强制。

    The locale files are plain nested maps, so the check reads indentation only and
    needs no YAML module: every `key:` line pushes or pops an indent scope and the
    dotted path of each key is collected for both files, then compared in both
    directions. Duplicate paths, empty leaf values and unsupported YAML shapes
    (lists, block scalars, unparseable lines) fail the check as well, because each
    of them would otherwise let a mismatch slip through or render empty text.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$LocalesDirectory
)

# 文件名固定，见 AGENTS.md 第 11 节；en.yml 为回退语言。
$referenceName = 'en.yml'
$targetName = 'zh-CN.yml'

function Get-LocaleKeyPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $fileName = Split-Path -Path $Path -Leaf
    $keyList = [System.Collections.Generic.List[string]]::new()
    $problemList = [System.Collections.Generic.List[string]]::new()
    $scope = [System.Collections.Generic.List[object]]::new()
    $lines = [System.IO.File]::ReadAllLines($Path)

    for ($index = 0; $index -lt $lines.Count; $index++) {
        $text = $lines[$index].TrimEnd()

        if ($text.Trim() -eq '' -or $text.TrimStart().StartsWith('#')) {
            continue
        }

        if ($index -eq 0 -and $text.Trim() -eq '---') {
            continue
        }

        if ($text -match '^\s*-\s') {
            $problemList.Add("${fileName}:$($index + 1): 暂不支持的 YAML 列表项，键一致性比较无法覆盖列表内元素。")
            continue
        }

        if ($text -notmatch '^(\s*)([^\s#][^:]*?)\s*:\s*(.*)$') {
            $problemList.Add("${fileName}:$($index + 1): 无法解析的行，键一致性比较可能失真。")
            continue
        }

        $indent = $Matches[1].Length
        $key = $Matches[2].Trim()
        $value = $Matches[3].Trim()

        if ($value -in @('|', '>', '|-', '|+', '>-', '>+')) {
            $problemList.Add("${fileName}:$($index + 1): 暂不支持的块标量（多行值），键一致性比较可能失真。")
            continue
        }

        while ($scope.Count -gt 0 -and $scope[$scope.Count - 1].Indent -ge $indent) {
            $scope.RemoveAt($scope.Count - 1)
        }

        $segments = [System.Collections.Generic.List[string]]::new()
        foreach ($entry in $scope) {
            [void]$segments.Add($entry.Key)
        }
        [void]$segments.Add($key)
        [void]$keyList.Add(($segments -join '.'))

        if ($value -eq '') {
            $scope.Add([pscustomobject]@{ Indent = $indent; Key = $key })

            # 空值且其后没有更深缩进子键 = 空叶子（键存在但没有文案）。
            $childIndent = -1
            for ($next = $index + 1; $next -lt $lines.Count; $next++) {
                $candidate = $lines[$next]
                if ($candidate.Trim() -eq '' -or $candidate.TrimStart().StartsWith('#')) {
                    continue
                }
                $childIndent = $candidate.Length - $candidate.TrimStart().Length
                break
            }

            if ($childIndent -le $indent) {
                $problemList.Add("${fileName}:$($index + 1): 键 '$key' 是空值叶子，键存在但没有文案。")
            }
        }
    }

    return @{ KeyList = $keyList; ProblemList = $problemList }
}

$referencePath = Join-Path -Path $LocalesDirectory -ChildPath $referenceName
$targetPath = Join-Path -Path $LocalesDirectory -ChildPath $targetName

foreach ($filePath in @($referencePath, $targetPath)) {
    if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
        throw "未找到翻译文件 $filePath。"
    }
}

$reference = Get-LocaleKeyPath -Path $referencePath
$target = Get-LocaleKeyPath -Path $targetPath

$problems = [System.Collections.Generic.List[string]]::new()
foreach ($problem in $reference.ProblemList) {
    [void]$problems.Add($problem)
}
foreach ($problem in $target.ProblemList) {
    [void]$problems.Add($problem)
}

foreach ($key in $reference.KeyList) {
    if (-not ($target.KeyList -contains $key)) {
        [void]$problems.Add("$targetName 缺少键 '$key'（存在于 $referenceName）。")
    }
}

foreach ($key in $target.KeyList) {
    if (-not ($reference.KeyList -contains $key)) {
        [void]$problems.Add("$referenceName 缺少键 '$key'（存在于 $targetName）。")
    }
}

foreach ($duplicate in @($reference.KeyList | Group-Object | Where-Object { $_.Count -gt 1 })) {
    [void]$problems.Add("$referenceName 重复键 '$($duplicate.Name)' 出现 $($duplicate.Count) 次，后值会静默覆盖前值。")
}

foreach ($duplicate in @($target.KeyList | Group-Object | Where-Object { $_.Count -gt 1 })) {
    [void]$problems.Add("$targetName 重复键 '$($duplicate.Name)' 出现 $($duplicate.Count) 次，后值会静默覆盖前值。")
}

if ($reference.KeyList.Count -eq 0 -or $target.KeyList.Count -eq 0) {
    if ($problems.Count -gt 0) {
        $problems | ForEach-Object { Write-Host $_ }
    }
    throw "$referenceName 解析到 $($reference.KeyList.Count) 个键，$targetName 解析到 $($target.KeyList.Count) 个键；至少一侧为 0，拒绝以空集合通过比较。"
}

if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Host $_ }
    throw "i18n 键校验失败，共 $($problems.Count) 处。"
}

Write-Host "i18n 键校验通过：$referenceName / $targetName 各 $($reference.KeyList.Count) 个键一致。"
