param(
  [Parameter(Mandatory = $true)]
  [string]$SetupPath,
  [int]$WindowTimeoutSeconds = 30,
  [switch]$InspectOnly,
  [switch]$Lifecycle
)

$ErrorActionPreference = "Stop"
$ResolvedSetup = (Resolve-Path -LiteralPath $SetupPath -ErrorAction Stop).Path
$ActionLog = Join-Path ([System.IO.Path]::GetTempPath()) ("mediadrop-setup-actions-" + [guid]::NewGuid().ToString("N") + ".log")

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class MediaDropSetupProbe {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);

  [StructLayout(LayoutKind.Sequential)]
  public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }

  [StructLayout(LayoutKind.Sequential)]
  public struct Point { public int X; public int Y; }

  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr lParam);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll")] static extern IntPtr GetDlgItem(IntPtr parent, int id);
  [DllImport("user32.dll")] static extern bool IsWindowEnabled(IntPtr hwnd);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref Point point);
  [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr hwnd, ref Point point);
  [DllImport("user32.dll")] static extern IntPtr ChildWindowFromPointEx(IntPtr parent, Point point, uint flags);
  [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr64(IntPtr hwnd, int index);
  [DllImport("user32.dll", EntryPoint = "GetWindowLongW")] static extern IntPtr GetWindowLongPtr32(IntPtr hwnd, int index);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

  static IntPtr GetWindowLongPtr(IntPtr hwnd, int index) {
    return IntPtr.Size == 8 ? GetWindowLongPtr64(hwnd, index) : GetWindowLongPtr32(hwnd, index);
  }

  public static bool HasVisibleStyle(IntPtr hwnd) {
    return (GetWindowLongPtr(hwnd, -16).ToInt64() & 0x10000000L) != 0;
  }

  public static IntPtr FindRoot(uint processId) {
    IntPtr result = IntPtr.Zero;
    EnumWindows((hwnd, _) => {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (owner != processId) return true;
      var title = new StringBuilder(128);
      GetWindowText(hwnd, title, title.Capacity);
      if (title.ToString() != "MediaDrop Kurulum") return true;
      if (!IsWindowVisible(hwnd)) return true;
      result = hwnd;
      return false;
    }, IntPtr.Zero);
    return result;
  }

  public static bool NativeControlIsEnabled(IntPtr root, int id) {
    IntPtr control = GetDlgItem(root, id);
    return control != IntPtr.Zero && IsWindowEnabled(control);
  }

  public static IntPtr FindStaticByBounds(IntPtr root, int x, int y, int width, int height) {
    Rect rootRect;
    GetWindowRect(root, out rootRect);
    IntPtr result = IntPtr.Zero;
    EnumChildWindows(root, (hwnd, _) => {
      if (!HasVisibleStyle(hwnd)) return true;
      var className = new StringBuilder(64);
      GetClassName(hwnd, className, className.Capacity);
      if (className.ToString() != "Static") return true;
      Rect rect;
      GetWindowRect(hwnd, out rect);
      if (Math.Abs((rect.Left - rootRect.Left) - x) <= 2 &&
          Math.Abs((rect.Top - rootRect.Top) - y) <= 2 &&
          rect.Right - rect.Left == width && rect.Bottom - rect.Top == height) {
        result = hwnd;
        return false;
      }
      return true;
    }, IntPtr.Zero);
    return result;
  }

  public static string ClassName(IntPtr hwnd) {
    var className = new StringBuilder(64);
    GetClassName(hwnd, className, className.Capacity);
    return className.ToString();
  }

  public static string DescribeWindow(IntPtr hwnd, IntPtr root) {
    Rect rect;
    Rect rootRect;
    GetWindowRect(hwnd, out rect);
    GetWindowRect(root, out rootRect);
    return String.Format("{0} at {1},{2},{3},{4}", ClassName(hwnd), rect.Left - rootRect.Left, rect.Top - rootRect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top);
  }

  public static string DescribeStatics(IntPtr root) {
    Rect rootRect;
    GetWindowRect(root, out rootRect);
    var values = new List<string>();
    EnumChildWindows(root, (hwnd, _) => {
      if (ClassName(hwnd) != "Static") return true;
      Rect rect;
      GetWindowRect(hwnd, out rect);
      values.Add(String.Format("{0},{1},{2},{3},visible={4}", rect.Left - rootRect.Left, rect.Top - rootRect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top, HasVisibleStyle(hwnd)));
      return true;
    }, IntPtr.Zero);
    return String.Join(";", values.ToArray());
  }

  public static IntPtr DeepChildAt(IntPtr root, int x, int y) {
    Point screen = new Point { X = x, Y = y };
    ClientToScreen(root, ref screen);
    IntPtr current = root;
    for (int depth = 0; depth < 8; depth++) {
      Point local = screen;
      ScreenToClient(current, ref local);
      IntPtr child = ChildWindowFromPointEx(current, local, 0x1 | 0x2);
      if (child == IntPtr.Zero || child == current) break;
      current = child;
    }
    return current;
  }

  public static void Click(IntPtr hwnd) {
    Rect rect;
    GetWindowRect(hwnd, out rect);
    int x = Math.Max(1, (rect.Right - rect.Left) / 2);
    int y = Math.Max(1, (rect.Bottom - rect.Top) / 2);
    IntPtr point = new IntPtr((y << 16) | (x & 0xffff));
    SendMessage(hwnd, 0x0201, new IntPtr(1), point);
    SendMessage(hwnd, 0x0202, IntPtr.Zero, point);
  }

  public static void ClickAt(IntPtr root, int x, int y) {
    Click(DeepChildAt(root, x, y));
  }
}
'@

function Start-HiddenProbe {
  param([string]$Screen, [string]$HoverTarget = "")
  $arguments = "/SCREEN=$Screen /OFFSCREENUITEST=1 /UIACTIONLOG=$ActionLog"
  if ($Lifecycle) { $arguments += " /WORKERSCENARIO=cancel" }
  if (-not [string]::IsNullOrWhiteSpace($HoverTarget)) { $arguments += " /HOVERUITEST=$HoverTarget" }
  $process = Start-Process -FilePath $ResolvedSetup -ArgumentList $arguments -PassThru
  $root = [IntPtr]::Zero
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 10) -and $root -eq [IntPtr]::Zero; $attempt++) {
    Start-Sleep -Milliseconds 100
    $root = [MediaDropSetupProbe]::FindRoot([uint32]$process.Id)
  }
  if ($root -eq [IntPtr]::Zero) {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    throw "Hidden setup window was not created for screen: $Screen"
  }
  [pscustomobject]@{ Process = $process; Root = $root }
}

function Stop-Probe {
  param($Probe)
  if ($null -ne $Probe -and -not $Probe.Process.HasExited) {
    Stop-Process -Id $Probe.Process.Id -Force
    $Probe.Process.WaitForExit()
  }
}

function Get-Hit {
  param($Probe, [int]$X, [int]$Y, [int]$Width, [int]$Height, [string]$Name)
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 20); $attempt++) {
    $hit = [MediaDropSetupProbe]::FindStaticByBounds($Probe.Root, $X, $Y, $Width, $Height)
    if ($hit -ne [IntPtr]::Zero) { return $hit }
    Start-Sleep -Milliseconds 50
  }
  throw "$Name action is missing or blocked. Controls: $([MediaDropSetupProbe]::DescribeStatics($Probe.Root))"
}

function Assert-RealHit {
  param($Probe, [IntPtr]$Expected, [int]$X, [int]$Y, [string]$Name)
  $actual = [IntPtr]::Zero
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 20); $attempt++) {
    $actual = [MediaDropSetupProbe]::DeepChildAt($Probe.Root, $X, $Y)
    if ($actual -eq $Expected) { return }
    Start-Sleep -Milliseconds 50
  }
  throw "$Name is covered by $([MediaDropSetupProbe]::DescribeWindow($actual, $Probe.Root)); expected handle $Expected, actual handle $actual."
}

function Wait-Action {
  param([string]$Action, [int]$Count = 1)
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 20); $attempt++) {
    if (Test-Path -LiteralPath $ActionLog) {
      $matches = @(Get-Content -LiteralPath $ActionLog | Where-Object { $_ -eq $Action }).Count
      if ($matches -ge $Count) { return }
    }
    Start-Sleep -Milliseconds 50
  }
  $seen = if (Test-Path -LiteralPath $ActionLog) { (Get-Content -LiteralPath $ActionLog) -join "," } else { "none" }
  throw "Setup action was not handled: $Action. Seen: $seen"
}

function Wait-ActionPrefix {
  param([string]$Prefix, [int]$Count = 1)
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 40); $attempt++) {
    if (Test-Path -LiteralPath $ActionLog) {
      $matches = @(Get-Content -LiteralPath $ActionLog | Where-Object { $_.StartsWith($Prefix) }).Count
      if ($matches -ge $Count) { return }
    }
    Start-Sleep -Milliseconds 25
  }
  $seen = if (Test-Path -LiteralPath $ActionLog) { (Get-Content -LiteralPath $ActionLog) -join "," } else { "none" }
  throw "Setup state was not observed: $Prefix. Seen: $seen"
}

function Wait-Exit {
  param($Probe)
  if (-not $Probe.Process.WaitForExit(5000)) { throw "Setup did not close after its exit action." }
}

$probe = $null
$testFailed = $false
try {
  if (-not $Lifecycle) {
    $probe = Start-HiddenProbe "extension" "browser_1"
    $browser = Get-Hit $probe 630 228 136 45 "Second browser"
    $browserHover = Get-Hit $probe 628 227 140 47 "Second browser hover animation"
    Assert-RealHit $probe $browser 698 250 "Second browser while hovered"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 698, 250)
    Wait-Action "browser_1"
    Stop-Probe $probe
    $probe = $null

    $probe = Start-HiddenProbe "extension" "extension_primary"
    Wait-Action "startup_hidden"
    Wait-Action "startup_ready"
    $primary = Get-Hit $probe 894 566 170 46 "Extension primary"
    $primaryHover = Get-Hit $probe 892 565 174 48 "Extension primary hover animation"
    Assert-RealHit $probe $primary 979 589 "Extension primary while hovered"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 979, 589)
    Wait-Action "extension_primary"
    Wait-Action "copy_extension_address:opera:extensions"
    Wait-Action "clipboard:opera:extensions"
    Wait-Action "browser_launch:C:\Program Files\Opera GX\opera.exe|-noautoupdate --|"
    Assert-RealHit $probe $primary 979 589 "Extension confirmation while hovered"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 979, 589)
    Wait-Action "extension_primary" 2
    Wait-Action "screen:done"
    Stop-Probe $probe
    $probe = $null
    if (Test-Path -LiteralPath $ActionLog) { Remove-Item -LiteralPath $ActionLog -Force }
  }

  $probe = Start-HiddenProbe "welcome" "minimize"
  $minimize = Get-Hit $probe 1022 13 38 38 "Minimize"
  $minimizeHover = Get-Hit $probe 1020 12 42 40 "Minimize hover animation"
  Assert-RealHit $probe $minimize 1041 32 "Minimize while hovered"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 32)
  Wait-Action "minimize"
  Stop-Probe $probe
  $probe = $null

  $probe = Start-HiddenProbe "welcome" "install"
  $duplicateArguments = "/SCREEN=welcome /OFFSCREENUITEST=1 /UIACTIONLOG=$ActionLog"
  if ($Lifecycle) { $duplicateArguments += " /WORKERSCENARIO=cancel" }
  $duplicate = Start-Process -FilePath $ResolvedSetup -ArgumentList $duplicateArguments -PassThru
  if (-not $duplicate.WaitForExit(5000)) {
    Stop-Process -Id $duplicate.Id -Force
    throw "A second setup instance was not rejected by the per-user mutex."
  }
  if ($probe.Process.HasExited) {
    throw "The active setup exited while the duplicate instance was rejected."
  }
  $start = Get-Hit $probe 884 566 180 46 "Install"
  $hoverVisual = Get-Hit $probe 882 565 184 48 "Install hover animation"
  foreach ($id in @(1, 2, 3, 1028)) {
    if ([MediaDropSetupProbe]::NativeControlIsEnabled($probe.Root, $id)) {
      throw "Native NSIS control remained interactive: $id"
    }
  }

  $welcomeToggle = Get-Hit $probe 482 487 582 54 "Extension toggle"
  Assert-RealHit $probe $welcomeToggle 773 514 "Extension toggle row"
  Assert-RealHit $probe $welcomeToggle 1041 515 "Extension toggle switch"
  Assert-RealHit $probe $start 974 589 "Install"
  $close = Get-Hit $probe 1064 13 38 38 "Close"
  Assert-RealHit $probe $close 1083 32 "Close"
  if ($InspectOnly) {
    Write-Output "MediaDrop setup interaction inspection passed."
    return
  }

  [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 515)
  Wait-Action "toggle_welcome_extension"
  Wait-Action "toggle_motion_complete"
  Assert-RealHit $probe $welcomeToggle 1041 515 "Extension toggle switch after state change"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 515)
  Wait-Action "toggle_welcome_extension" 2
  Wait-Action "toggle_motion_complete" 2
  if ($Lifecycle) {
    [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 515)
    Wait-Action "toggle_welcome_extension" 3
    Wait-Action "toggle_motion_complete" 3
  }
  [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
  Wait-Action "start"
  $cancel = Get-Hit $probe 981 566 83 46 "Cancel"
  Assert-RealHit $probe $cancel 1022 589 "Cancel"

  if ($Lifecycle) {
    Wait-ActionPrefix "status:installing:"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 1022, 589)
    Wait-Action "cancel"
    Start-Sleep -Milliseconds 40
    if (@(Get-Content -LiteralPath $ActionLog | Where-Object { $_ -eq "screen:error" }).Count -ne 0) {
      throw "Cancel displayed a terminal error before rollback completed."
    }
    Wait-ActionPrefix "status:cancel_pending:"
    Wait-ActionPrefix "status:rolling_back:"
    Wait-ActionPrefix "status:failed:"
    Wait-Action "screen:error"

    $retry = Get-Hit $probe 884 566 180 46 "Retry"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
    Wait-Action "retry"
    Wait-ActionPrefix "session:" 2
    Wait-ActionPrefix "status:succeeded:100"
    Wait-Action "screen:extension"
    $copy = Get-Hit $probe 610 566 132 46 "Copy extension path after install"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 676, 589)
    Wait-Action "copy_extension_path"
    Wait-Action "clipboard:C:\Program Files\MediaDrop\browser-extension"
    $reveal = Get-Hit $probe 752 566 132 46 "Reveal extension folder after install"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 818, 589)
    Wait-Action "reveal_extension_path"
    $primary = Get-Hit $probe 894 566 170 46 "Open browser after install"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 979, 589)
    Wait-Action "extension_primary"
    $later = Get-Hit $probe 482 566 116 46 "Continue without extension"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 540, 589)
    Wait-Action "extension_later"
    Wait-Action "screen:done"
    $finish = Get-Hit $probe 884 566 180 46 "Finish"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
    Wait-Action "finish"
    Wait-Exit $probe
    $probe = $null

    $actions = @(Get-Content -LiteralPath $ActionLog)
    $cancelIndex = [Array]::IndexOf($actions, "cancel")
    $pendingIndex = [Array]::FindIndex($actions, [Predicate[string]]{ param($value) $value.StartsWith("status:cancel_pending:") })
    $rollbackIndex = [Array]::FindIndex($actions, [Predicate[string]]{ param($value) $value.StartsWith("status:rolling_back:") })
    $failedIndex = [Array]::FindIndex($actions, [Predicate[string]]{ param($value) $value.StartsWith("status:failed:") })
    $errorIndex = [Array]::IndexOf($actions, "screen:error")
    if (-not ($cancelIndex -lt $pendingIndex -and $pendingIndex -lt $rollbackIndex -and $rollbackIndex -lt $failedIndex -and $failedIndex -lt $errorIndex)) {
      throw "Cancel/rollback/terminal ordering is invalid: $($actions -join ',')"
    }
    $sessions = @($actions | Where-Object { $_.StartsWith("session:") } | ForEach-Object { $_.Substring(8) })
    if ($sessions.Count -ne 2 -or $sessions[0] -eq $sessions[1]) {
      throw "Retry did not create a fresh installer session: $($sessions -join ',')"
    }
    foreach ($session in $sessions) {
      $sessionPath = Join-Path $env:LOCALAPPDATA "MediaDrop\InstallerSessions\$session"
      if (Test-Path -LiteralPath $sessionPath) { throw "Terminal session was not cleaned: $sessionPath" }
    }
    if (@(Get-Process -Name "mediadrop-installer-worker" -ErrorAction SilentlyContinue).Count -ne 0) {
      throw "Installer worker remained alive after lifecycle completion."
    }

    $installingCount = @($actions | Where-Object { $_.StartsWith("status:installing:") }).Count
    $rollbackCount = @($actions | Where-Object { $_.StartsWith("status:rolling_back:") }).Count
    $failedCount = @($actions | Where-Object { $_.StartsWith("status:failed:") }).Count
    $probe = Start-HiddenProbe "welcome"
    $start = Get-Hit $probe 884 566 180 46 "Install"
    Assert-RealHit $probe $start 974 589 "Install for close lifecycle"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
    Wait-Action "start" 2
    Wait-ActionPrefix "status:installing:" ($installingCount + 1)
    $close = Get-Hit $probe 1064 13 38 38 "Close"
    [MediaDropSetupProbe]::ClickAt($probe.Root, 1083, 32)
    Wait-Action "close"
    Start-Sleep -Milliseconds 40
    if ($probe.Process.HasExited) { throw "Closing during install bypassed safe cancellation." }
    Wait-ActionPrefix "status:rolling_back:" ($rollbackCount + 1)
    Wait-ActionPrefix "status:failed:" ($failedCount + 1)
    Wait-Action "screen:error" 2
    [MediaDropSetupProbe]::ClickAt($probe.Root, 1083, 32)
    Wait-Action "close" 2
    Wait-Exit $probe
    $probe = $null

    $allSessions = @(Get-Content -LiteralPath $ActionLog | Where-Object { $_.StartsWith("session:") } | ForEach-Object { $_.Substring(8) })
    if ($allSessions.Count -ne 3 -or @($allSessions | Select-Object -Unique).Count -ne 3) {
      throw "Close lifecycle did not use an isolated session: $($allSessions -join ',')"
    }
    foreach ($session in $allSessions) {
      $sessionPath = Join-Path $env:LOCALAPPDATA "MediaDrop\InstallerSessions\$session"
      if (Test-Path -LiteralPath $sessionPath) { throw "Closed lifecycle session was not cleaned: $sessionPath" }
    }
    if (@(Get-Process -Name "mediadrop-installer-worker" -ErrorAction SilentlyContinue).Count -ne 0) {
      throw "Installer worker remained alive after close cancellation."
    }
    Write-Output "MediaDrop setup broker lifecycle smoke test passed."
    return
  }

  [MediaDropSetupProbe]::ClickAt($probe.Root, 1022, 589)
  Wait-Action "cancel"
  $log = Get-Hit $probe 482 566 146 46 "Open log"
  Assert-RealHit $probe $log 555 589 "Open log"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 555, 589)
  Wait-Action "open_log"
  $retry = Get-Hit $probe 884 566 180 46 "Retry"
  Assert-RealHit $probe $retry 974 589 "Retry"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
  Wait-Action "retry"
  $cancel = Get-Hit $probe 981 566 83 46 "Cancel after retry"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1022, 589)
  Wait-Action "cancel" 2
  Wait-Action "screen:error" 2
  $close = Get-Hit $probe 1064 13 38 38 "Close"
  Assert-RealHit $probe $close 1083 32 "Close on error"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1083, 32)
  Wait-Action "close"
  Wait-Exit $probe
  $probe = $null

  $probe = Start-HiddenProbe "error"
  $giveUp = Get-Hit $probe 789 566 85 46 "Give up"
  Assert-RealHit $probe $giveUp 831 589 "Give up"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 831, 589)
  Wait-Action "give_up"
  Wait-Exit $probe
  $probe = $null

  $probe = Start-HiddenProbe "done"
  $extensionToggle = Get-Hit $probe 482 414 582 59 "Extension toggle"
  Assert-RealHit $probe $extensionToggle 1041 443 "Done extension toggle"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 443)
  Wait-Action "toggle_done_extension"
  Wait-Action "toggle_motion_complete" 3
  $launchToggle = Get-Hit $probe 482 355 582 59 "Launch toggle"
  Assert-RealHit $probe $launchToggle 1041 390 "Launch toggle"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 1041, 390)
  Wait-Action "toggle_launch_app"
  Wait-Action "toggle_motion_complete" 4
  $finish = Get-Hit $probe 884 566 180 46 "Finish"
  Assert-RealHit $probe $finish 974 589 "Finish"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
  Wait-Action "finish"
  $copy = Get-Hit $probe 610 566 132 46 "Copy extension path"
  Assert-RealHit $probe $copy 676 589 "Copy extension path"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 676, 589)
  Wait-Action "copy_extension_path"
  $reveal = Get-Hit $probe 752 566 132 46 "Reveal extension folder"
  Assert-RealHit $probe $reveal 818 589 "Reveal extension folder"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 818, 589)
  Wait-Action "reveal_extension_path"
  $primary = Get-Hit $probe 894 566 170 46 "Extension primary"
  Assert-RealHit $probe $primary 979 589 "Extension primary"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 979, 589)
  Wait-Action "extension_primary"
  $later = Get-Hit $probe 482 566 116 46 "Extension later"
  Assert-RealHit $probe $later 540 589 "Extension later"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 540, 589)
  Wait-Action "extension_later"
  $finish = Get-Hit $probe 884 566 180 46 "Finish"
  Assert-RealHit $probe $finish 974 589 "Finish after extension guide"
  [MediaDropSetupProbe]::ClickAt($probe.Root, 974, 589)
  Wait-Action "finish" 2
  Wait-Exit $probe
  $probe = $null

  Write-Output "MediaDrop setup hidden interaction smoke test passed."
}
catch {
  $testFailed = $true
  throw
}
finally {
  Stop-Probe $probe
  if ($testFailed) {
    Write-Warning "Setup action log retained for diagnosis: $ActionLog"
  } elseif (Test-Path -LiteralPath $ActionLog) {
    Remove-Item -LiteralPath $ActionLog -Force
  }
}
