param([long]$WindowHandle = 0, [int]$ProcessId = 0, [string]$OutputPath = "", [string]$ExpectedTitle = "")

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class NotificationWindowProbe {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static IntPtr FindForProcess(uint processId, string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((hwnd, lParam) => {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (owner != processId) return true;
      var text = new StringBuilder(256);
      GetWindowText(hwnd, text, text.Capacity);
      if (text.ToString() != title) return true;
      found = hwnd;
      return false;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[NotificationWindowProbe]::SetProcessDPIAware() | Out-Null

$windowTitle = ([string][char]0x901A) + ([string][char]0x77E5)
$handle = if ($WindowHandle -gt 0) {
  [IntPtr]$WindowHandle
} elseif ($ProcessId -gt 0) {
  [NotificationWindowProbe]::FindForProcess([uint32]$ProcessId, $windowTitle)
} else {
  [IntPtr]::Zero
}
if ($handle -eq [IntPtr]::Zero) { throw "Notification window not found" }
if ([NotificationWindowProbe]::IsHungAppWindow($handle)) { throw "Notification window is unresponsive (UI thread blocked)" }
$automationRoot = [System.Windows.Automation.AutomationElement]::FromHandle($handle)
$title = ([string][char]0x4EFB) + ([string][char]0x52A1) + ([string][char]0x5B8C) + ([string][char]0x6210)
if ($ExpectedTitle) { $title = $ExpectedTitle }
$titleCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $title)
$renderedTitle = $automationRoot.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $titleCondition)
if ($null -eq $renderedTitle) {
  $elements = $automationRoot.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  for ($i = 0; $i -lt [Math]::Min($elements.Count, 80); $i++) {
    $element = $elements.Item($i)
    if ($element.Current.Name) { Write-Output ("element={0} name={1} type={2}" -f $i, $element.Current.Name, $element.Current.ControlType.ProgrammaticName) }
  }
  $contentRendered = $false
} else {
  $contentRendered = $true
}
$rect = New-Object NotificationWindowProbe+RECT
if (-not [NotificationWindowProbe]::GetWindowRect($handle, [ref]$rect)) { throw "Cannot read notification window bounds" }
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
$workArea = [System.Windows.Forms.Screen]::FromHandle($handle).WorkingArea
$expectedMargin = 22 * $width / 520
$positionCorrect = [Math]::Abs(($workArea.Right - $rect.Right) - $expectedMargin) -le 4 -and [Math]::Abs(($workArea.Bottom - $rect.Bottom) - $expectedMargin) -le 4
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
$bright = 0
$sampled = 0
for ($x = 0; $x -lt $width; $x += 4) {
  for ($y = 0; $y -lt $height; $y += 4) {
    $pixel = $bitmap.GetPixel($x, $y)
    if ($pixel.R -gt 240 -and $pixel.G -gt 240 -and $pixel.B -gt 240) { $bright++ }
    $sampled++
  }
}
if ($OutputPath) { $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png) }
$graphics.Dispose()
$bitmap.Dispose()
$ratio = if ($sampled) { $bright / $sampled } else { 1 }
$hung = [NotificationWindowProbe]::IsHungAppWindow($handle)
Write-Output ("handle={0} window={1}x{2} brightRatio={3:N3} hung={4} contentRendered={5}" -f $handle, $width, $height, $ratio, $hung, $contentRendered)
Write-Output ("positionCorrect={0} bounds={1},{2},{3},{4}" -f $positionCorrect, $rect.Left, $rect.Top, $rect.Right, $rect.Bottom)
if ($hung -or $ratio -gt 0.75 -or -not $contentRendered) { throw "Notification window is blank or unresponsive" }
if (-not $positionCorrect) { throw "Notification is not at the work area's bottom-right corner" }
