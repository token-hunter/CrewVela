[CmdletBinding()]
param(
  [string]$DataDirectory,
  [string]$Executable
)
$ErrorActionPreference = 'Stop'
$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $DataDirectory) { $DataDirectory = Join-Path $projectRoot '.test-data' }
if (-not $Executable) { $Executable = Join-Path $projectRoot 'src-tauri\target\release\CrewVela.exe' }
$testRoot = [System.IO.Path]::GetFullPath($DataDirectory)
$exePath = [System.IO.Path]::GetFullPath($Executable)
if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) { throw '请先执行 npm ci 和 npm run tauri -- build --no-bundle。' }
# Do not alter HOME/AppData, copy old profiles, delete files, or change global settings.
Write-Host "启动 CrewVela 新用户隔离测试：$testRoot"
$testProcess = Start-Process -FilePath $exePath -ArgumentList @('--test-data-dir', ('"' + $testRoot + '"')) -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru
Write-Output ([PSCustomObject]@{ ProcessId=$testProcess.Id; DataDirectory=$testRoot; Executable=$exePath })
