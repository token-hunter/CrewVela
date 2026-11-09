param(
    [string]$PluginRoot = (Join-Path $env:USERPROFILE '.codex/plugins/cache/openai-curated-remote/impeccable/4.3.1'),
    [string]$ConfigPath = (Join-Path $env:USERPROFILE '.codex/config.toml'),
    [switch]$Apply,
    [string]$ExpectedHooksHash = '',
    [string]$ExpectedConfigHash = ''
)

# Repairs only the reviewed Impeccable 4.3.1 Windows hooks. Default is a dry run.
# No model, download, plugin disable, or trust bypass occurs here. After Apply,
# trust the two reviewed currentHash values using Codex's native hooks/list ->
# config/batchWrite -> hooks/list flow, preserving every enabled choice.
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$PluginRoot = [IO.Path]::GetFullPath($PluginRoot)
$ConfigPath = [IO.Path]::GetFullPath($ConfigPath)
$hooksPath = Join-Path $PluginRoot 'hooks/hooks.json'
$metadata = [IO.File]::ReadAllText((Join-Path $PluginRoot '.codex-plugin/plugin.json')) | ConvertFrom-Json
if ($metadata.name -ne 'impeccable' -or $metadata.version -ne '4.3.1') {
    throw 'Refusing to modify an unreviewed plugin or version.'
}
$launcher = Join-Path $PluginRoot 'skills/impeccable/scripts/impeccable.cmd'
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) { throw 'The reviewed Windows launcher is missing.' }

function Hash-File([string]$Path) {
    Hash-Bytes ([IO.File]::ReadAllBytes($Path))
}
function Hash-Bytes([byte[]]$Bytes) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($algorithm.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}
function Replace-Checked([string]$Path, [byte[]]$Bytes, [string]$Expected, [string]$Backup) {
    if ((Hash-File $Path) -ne $Expected) { throw "File changed since review: $Path" }
    $temporary = "$Path.crewvela-$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllBytes($temporary, $Bytes)
        if ((Hash-File $Path) -ne $Expected) { throw "Concurrent edit detected: $Path" }
        [IO.File]::Replace($temporary, $Path, $Backup)
        if ((Hash-File $Backup) -ne $Expected) { throw "Backup verification failed: $Backup" }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
}

$hooksHash = Hash-File $hooksPath
$configHash = Hash-File $ConfigPath
$hooks = [IO.File]::ReadAllText($hooksPath) | ConvertFrom-Json
# The plugin runner supplies PLUGIN_ROOT as an environment variable. Resolving
# it through Join-Path avoids quoting bugs with spaces, apostrophes or $ in a
# profile path. The native .cmd invocation retains stdin/stdout/stderr; explicit
# exit propagates real native errors instead of PowerShell's boolean exit code.
$fixedCommand = '$impeccableHook = Join-Path $env:PLUGIN_ROOT ''skills/impeccable/scripts/impeccable.cmd''; if (Test-Path -LiteralPath $impeccableHook -PathType Leaf) { & $impeccableHook hook; exit $LASTEXITCODE }'
$changes = @()
foreach ($eventName in @('PostToolUse', 'Stop')) {
    $handlers = @($hooks.hooks.$eventName | ForEach-Object { $_.hooks })
    if ($handlers.Count -ne 1) { throw "Unexpected handler count for $eventName" }
    $handler = $handlers[0]
    if ($handler.type -ne 'command' -or $handler.command -notmatch '\$\{PLUGIN_ROOT\}/skills/impeccable/scripts/impeccable') {
        throw "Unexpected non-Windows command for $eventName"
    }
    if ($handler.commandWindows -ne $fixedCommand -and $handler.commandWindows -notmatch '^if exist .*impeccable\.cmd.*exit /b') {
        throw "Windows command changed outside this repair: $eventName"
    }
    $changes += [pscustomobject]@{ event = $eventName; before = $handler.commandWindows; after = $fixedCommand }
    $handler.commandWindows = $fixedCommand
}
$hooksBytes = $utf8.GetBytes(($hooks | ConvertTo-Json -Depth 100) + "`n")
$rawConfig = [IO.File]::ReadAllText($ConfigPath)
$nextConfig = $rawConfig
$removed = @()
foreach ($eventLabel in @('post_tool_use', 'stop')) {
    $key = "impeccable@openai-curated-remote:hooks/hooks.json:${eventLabel}:0:0"
    $quotedKey = [regex]::Escape($key)
    $sectionPattern = '(?ms)(^\[hooks\.state\.["'']' + $quotedKey + '["'']\]\r?\n)(.*?)(?=^\[|\z)'
    $section = [regex]::Match($nextConfig, $sectionPattern)
    if (-not $section.Success) { throw "Expected hook state section is missing: $key" }
    $body = $section.Groups[2].Value
    $fieldPattern = '(?m)^trusted_content_hash[ \t]*=[ \t]*"sha256:[a-fA-F0-9]{64}"[ \t]*\r?\n?'
    $field = [regex]::Matches($body, $fieldPattern)
    if ($field.Count -gt 1) { throw "Duplicate content trust fields: $key" }
    if ($field.Count -eq 1) {
        $removed += [pscustomobject]@{ hookKey = $key; removedLine = $field[0].Value.TrimEnd() }
        $newBody = [regex]::Replace($body, $fieldPattern, '')
        $nextConfig = $nextConfig.Substring(0, $section.Groups[2].Index) + $newBody + $nextConfig.Substring($section.Groups[2].Index + $body.Length)
    }
}
# Preserve the original BOM when present. Other config bytes, including all
# trusted_hash and enabled values, remain unchanged by this field-only edit.
$originalConfigBytes = [IO.File]::ReadAllBytes($ConfigPath)
$configBytes = $utf8.GetBytes($nextConfig)
if ($originalConfigBytes.Length -ge 3 -and $originalConfigBytes[0] -eq 239 -and $originalConfigBytes[1] -eq 187 -and $originalConfigBytes[2] -eq 191) {
    $configBytes = [byte[]](239,187,191) + $configBytes
}
$plan = [ordered]@{
    applied = $false; hooksPath = $hooksPath; configPath = $ConfigPath
    expectedHooksHash = $hooksHash; expectedConfigHash = $configHash
    repairedHooksHash = (Hash-Bytes $hooksBytes); repairedConfigHash = (Hash-Bytes $configBytes)
    commandChanges = $changes; removedConfigFields = $removed
    nativeTrustRequired = $true; supportedHookShell = 'Windows PowerShell / pwsh'
    downloadsPerformed = $false
}
if ($Apply) {
    if ([string]::IsNullOrWhiteSpace($ExpectedHooksHash) -or [string]::IsNullOrWhiteSpace($ExpectedConfigHash)) { throw 'Apply requires both hashes from the reviewed dry run.' }
    if ($hooksHash -ne $ExpectedHooksHash -or $configHash -ne $ExpectedConfigHash) { throw 'Files changed since the reviewed dry run; no files were modified.' }
    if ($env:SHELL -and $env:SHELL -notmatch '(?i)(powershell|pwsh)(\.exe)?$') { throw 'This repair has not been validated for the configured non-PowerShell hook shell.' }
    $backupRoot = Join-Path (Split-Path $ConfigPath -Parent) ('crewvela-hook-repairs/' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffffZ'))
    [IO.Directory]::CreateDirectory($backupRoot) | Out-Null
    Replace-Checked $hooksPath $hooksBytes $ExpectedHooksHash (Join-Path $backupRoot 'impeccable-hooks.json')
    if ($removed.Count -gt 0) { Replace-Checked $ConfigPath $configBytes $ExpectedConfigHash (Join-Path $backupRoot 'config.toml') }
    $plan.applied = $true
    $plan.backupRoot = $backupRoot
    $plan | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $backupRoot 'manifest.json') -Encoding UTF8
}
$plan | ConvertTo-Json -Depth 20
