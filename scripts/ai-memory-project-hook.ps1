param(
    [Parameter(Mandatory)][ValidateSet('claude-code', 'codex', 'kimi-code', 'zcode')][string]$Agent,
    [Parameter(Mandatory)][string]$Event,
    [Parameter(Mandatory)][string]$MemoryExecutable,
    [Parameter(Mandatory)][string]$DataDir,
    [Parameter(Mandatory)][string]$ServerUrl,
    [string]$PolicyFile = ''
)

# Missing policy retains the original checkout boundary. Additional roots must
# be explicitly authorized in CrewVela and carry a project marker.
$ErrorActionPreference = 'Stop'
$defaultRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')
if ([string]::IsNullOrEmpty($PolicyFile)) {
    $configBase = [Environment]::GetFolderPath('ApplicationData')
    $configRoot = Join-Path $configBase 'CrewVela'
    # Compatibility read before the desktop has created its migration alias.
    $legacyConfigRoot = Join-Path $configBase 'cc-launcher'
    if (-not (Test-Path -LiteralPath $configRoot) -and (Test-Path -LiteralPath $legacyConfigRoot)) { $configRoot = $legacyConfigRoot }
    $PolicyFile = Join-Path $configRoot 'ai-memory/project-roots.json'
}
$raw = [Console]::In.ReadToEnd()
try {
    $roots = @($defaultRoot)
    if (Test-Path -LiteralPath $PolicyFile) {
        $policy = Get-Content -LiteralPath $PolicyFile -Raw | ConvertFrom-Json
        if ($policy.version -ne 1 -or $policy.roots -isnot [Array]) { throw 'Invalid project policy' }
        foreach ($allowed in $policy.roots) {
            if ($allowed -isnot [string] -or -not [IO.Path]::IsPathFullyQualified($allowed)) { throw 'Invalid authorized root' }
        }
        $roots = @($defaultRoot) + @($policy.roots)
    }
    $payload = $raw | ConvertFrom-Json
    if ($null -eq $payload -or $payload -is [Array] -or $payload -is [string]) {
        throw 'Expected an event object'
    }
    $eventCwd = $null
    foreach ($key in @('cwd', 'current_dir', 'working_dir', 'directory')) {
        $property = $payload.PSObject.Properties[$key]
        if ($null -ne $property -and $null -ne $property.Value) {
            if ($property.Value -isnot [string]) { throw 'Invalid event directory' }
            if (-not [string]::IsNullOrWhiteSpace($property.Value)) {
                $eventCwd = $property.Value
                break
            }
        }
    }
    if ($null -eq $eventCwd) { $eventCwd = (Get-Location).ProviderPath }
    # Never resolve a relative event path against the allowed checkout.
    if (-not [IO.Path]::IsPathFullyQualified($eventCwd)) { throw 'Relative event directory' }
    $eventCwd = [IO.Path]::GetFullPath($eventCwd).TrimEnd('\', '/')
    $comparison = [StringComparison]::OrdinalIgnoreCase
    $projectRoot = $roots | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\', '/') } |
        Where-Object { $eventCwd.Equals($_, $comparison) -or $eventCwd.StartsWith($_ + [IO.Path]::DirectorySeparatorChar, $comparison) } |
        Sort-Object Length -Descending | Select-Object -First 1
    if ($null -eq $projectRoot) {
        [Console]::Out.WriteLine('{}')
        exit 0
    }
    # A junction under this checkout must not turn into capture of another repo.
    $directory = Get-Item -LiteralPath $eventCwd
    if (-not $directory.PSIsContainer) { throw 'Event directory is not a directory' }
    while ($null -ne $directory -and $directory.FullName.StartsWith($projectRoot, $comparison)) {
        if (($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Linked event directory is outside the capture contract'
        }
        $directory = $directory.Parent
    }
    $directory = Get-Item -LiteralPath $eventCwd
    $markerFound = $false
    while ($null -ne $directory -and ($directory.FullName.Equals($projectRoot, $comparison) -or
        $directory.FullName.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, $comparison))) {
        $marker = Get-Item -LiteralPath (Join-Path $directory.FullName '.ai-memory.toml') -Force -ErrorAction SilentlyContinue
        if ($null -ne $marker) {
            if ($marker.PSIsContainer -or ($marker.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked project marker' }
            if (-not $directory.FullName.Equals($projectRoot, $comparison)) { throw 'Nested marker needs separate project authorization' }
            $markerFound = $true
            break
        }
        $directory = $directory.Parent
    }
    if (-not $markerFound) { throw 'Missing project marker' }
    function Assert-ProjectTarget([string]$target) {
        if ([string]::IsNullOrWhiteSpace($target)) { throw 'Invalid file target' }
        $absolute = if ([IO.Path]::IsPathFullyQualified($target)) {
            [IO.Path]::GetFullPath($target)
        } else { [IO.Path]::GetFullPath((Join-Path $eventCwd $target)) }
        if (-not ($absolute.Equals($projectRoot, $comparison) -or
            $absolute.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, $comparison))) {
            throw 'File target outside project'
        }
        # A new file need not exist, but every existing parent must remain here.
        $parent = $absolute
        $item = Get-Item -LiteralPath $parent -Force -ErrorAction SilentlyContinue
        while ($null -eq $item) {
            $parent = [IO.Path]::GetDirectoryName($parent)
            $item = Get-Item -LiteralPath $parent -Force -ErrorAction SilentlyContinue
        }
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked file target' }
        if (-not $item.PSIsContainer) { $item = $item.Directory }
        while ($null -ne $item -and $item.FullName.StartsWith($projectRoot, $comparison)) {
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked file target' }
            if (-not $item.FullName.Equals($projectRoot, $comparison) -and
                (Test-Path -LiteralPath (Join-Path $item.FullName '.ai-memory.toml'))) { throw 'File target belongs to a nested project' }
            $item = $item.Parent
        }
    }
    function Assert-ToolTargets($value, [int]$depth = 0) {
        if ($depth -gt 16) { throw 'Tool input nesting limit' }
        if ($value -is [Array]) {
            foreach ($entry in $value) { Assert-ToolTargets $entry ($depth + 1) }
        } elseif ($value -is [PSCustomObject]) {
            foreach ($property in $value.PSObject.Properties) {
                if ($property.Name -in @('file_path', 'filePath', 'path', 'absolute_path', 'AbsolutePath', 'notebook_path', 'TargetFile', 'workdir')) {
                    if ($property.Value -isnot [string]) { throw 'Invalid file target type' }
                    Assert-ProjectTarget $property.Value
                } elseif ($property.Name -eq 'paths') {
                    if ($property.Value -isnot [Array]) { throw 'Invalid file targets' }
                    foreach ($target in $property.Value) {
                        if ($target -isnot [string]) { throw 'Invalid file target type' }
                        Assert-ProjectTarget $target
                    }
                } else { Assert-ToolTargets $property.Value ($depth + 1) }
            }
        }
    }
    Assert-ToolTargets $payload.tool_input
    # Native hooks do not use their process cwd for these four agents.
    $payload | Add-Member -NotePropertyName cwd -NotePropertyValue $eventCwd -Force
    $raw = $payload | ConvertTo-Json -Depth 100 -Compress
} catch {
    [Console]::Error.WriteLine('CrewVela ai-memory: event skipped because its project directory could not be verified.')
    [Console]::Out.WriteLine('{}')
    exit 0
}

if (-not (Test-Path -LiteralPath $MemoryExecutable -PathType Leaf)) {
    [Console]::Error.WriteLine('CrewVela ai-memory: configured executable is missing.')
    exit 1
}
# The upstream can reroute recognized tools and apply repository markers.
# Verify that its final decision uses the selected marker and local endpoint.
try {
    $inspection = ($raw | & $MemoryExecutable --data-dir $DataDir hook --event $Event --agent $Agent --server-url $ServerUrl --check-capture) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $inspection.marker_present -ne $true -or $inspection.admits_capture -ne $true -or
        $inspection.scope_resolution -ne 'explicit' -or
        [string]::IsNullOrWhiteSpace($inspection.scope.workspace) -or [string]::IsNullOrWhiteSpace($inspection.scope.project) -or
        $inspection.server_resolution -ne 'install-default' -or $inspection.policy_state -eq 'invalid') {
        throw 'Project routing was rejected'
    }
} catch {
    [Console]::Error.WriteLine('CrewVela ai-memory: event skipped because its final project route could not be verified.')
    [Console]::Out.WriteLine('{}')
    exit 0
}
$raw | & $MemoryExecutable --data-dir $DataDir hook --event $Event --agent $Agent --server-url $ServerUrl
exit $LASTEXITCODE
