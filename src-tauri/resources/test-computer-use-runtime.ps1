param([string]$RuntimePath = (Join-Path $PSScriptRoot 'computer-use-runtime.ps1'))
$ErrorActionPreference = 'Stop'
# Parse and load definitions only: never invoke runtime startup or desktop APIs.
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($RuntimePath, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
$names = @('Render-OrcaTree','Compress-OrcaRenderedBrowserTabs','Test-OrcaDirectRenderedBrowserTabLine','Test-OrcaActiveRenderedBrowserTabLine','Get-OrcaRenderedElementIndex')
foreach ($name in $names) {
    $definition = $ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}, $true)
    Invoke-Expression $definition.Extent.Text
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
function New-OrcaElementRecord($Node, $Index, $Frame) {
    [pscustomobject]@{ index=$Index; name=$Node.Name; controlType=$Node.Role; localizedControlType=$Node.Role; value=''; actions=@(); automationId=''; isSelected=$Node.Selected }
}
function Get-OrcaMeaningfulActions($Actions) { @() }
function Format-OrcaSnapshotText($Text) { [string]$Text }
function Format-OrcaValueSegment { '' }
function Get-OrcaTextSnippets { @() }
function Test-OrcaPlainTextSubtree { $false }
function Test-OrcaSuppressChildren { $false }
function New-TestNode($Id, $Role, $Children=@(), $Selected=$false) {
    $node = [pscustomobject]@{ Id=$Id; Name="node-$Id"; Role=$Role; Children=@($Children); Selected=$Selected }
    $node | Add-Member ScriptMethod GetRuntimeId { @($this.Id) }
    $node | Add-Member ScriptMethod FindAll { param($Scope,$Condition) $this.Children }
    $node
}
$tabs = @(1..12 | ForEach-Object { New-TestNode $_ 'tab item' @() ($_ -eq 12) })
$tabGroup = New-TestNode 30 'group' $tabs
$buttons = @(40..51 | ForEach-Object { New-TestNode $_ 'button' })
$root = New-TestNode 100 'group' (@($tabGroup) + $buttons)
$MaxNodes=128; $MaxDepth=16
$tree = Render-OrcaTree $root $null $true
$indexes = @($tree.elements | ForEach-Object { $_.index })
if (@($indexes | Select-Object -Unique).Count -ne $indexes.Count) { throw 'Duplicate element indexes after tab compression' }
if (@($tree.elements | Where-Object controlType -eq 'tab item').Count -ne 1) { throw 'Tab compression fixture failed' }
if (@($tree.elements | Where-Object controlType -eq 'button').Count -ne 12) { throw 'Sibling records lost' }
foreach ($line in $tree.lines) {
    if ($line.TrimStart() -match '^(\d+) ') {
        if (@($tree.elements | Where-Object index -eq ([int]$Matches[1])).Count -ne 1) { throw "Ambiguous element line: $line" }
    }
}
# Compile P/Invoke definitions but call only the pure event construction method.
$source = [IO.File]::ReadAllText($RuntimePath)
$match = [regex]::Match($source, '(?s)Add-Type -TypeDefinition @"\r?\n(.*?)\r?\n"@')
Add-Type -TypeDefinition $match.Groups[1].Value
$text = 'A' + [char]0x4E2D + [char]0xD83D + [char]0xDE00 + '{+}'
$inputs = [OrcaDesktopWin32]::UnicodeInputs($text)
if ($inputs.Count -ne $text.Length * 2) { throw 'Unicode event count incorrect' }
for ($i=0; $i -lt $text.Length; $i++) {
    $down=$inputs[$i*2]; $up=$inputs[$i*2+1]
    if ($down.type -ne 1 -or $down.data.keyboard.virtualKey -ne 0 -or $down.data.keyboard.scanCode -ne [int]$text[$i] -or $down.data.keyboard.flags -ne 4 -or $up.data.keyboard.flags -ne 6) { throw 'Unicode UTF-16 event mismatch' }
}
$screenshot = $ast.Find({param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-OrcaScreenshot'},$true).Extent.Text
if ($screenshot.Contains('CopyFromScreen') -or -not $screenshot.Contains('::PrintWindow') -or -not $screenshot.Contains('screenshot_failed')) { throw 'Screenshot must use window capture and report failure' }
'PASS: tree index uniqueness, Unicode event construction, window capture contract (no desktop API invoked)'
