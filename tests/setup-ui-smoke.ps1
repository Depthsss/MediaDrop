param(
  [Parameter(Mandatory = $true)]
  [string]$SetupPath,
  [int]$WindowTimeoutSeconds = 30,
  [switch]$InspectOnly
)

$ErrorActionPreference = "Stop"
$ResolvedSetup = (Resolve-Path -LiteralPath $SetupPath -ErrorAction Stop).Path

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class MediaDropSetupProbe {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);

  [StructLayout(LayoutKind.Sequential)]
  public struct Point { public int X; public int Y; }

  [StructLayout(LayoutKind.Sequential)]
  public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }

  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr lParam);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] static extern IntPtr GetDlgItem(IntPtr parent, int id);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr hwnd, ref Point point);
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref Point point);
  [DllImport("user32.dll")] static extern IntPtr ChildWindowFromPointEx(IntPtr parent, Point point, uint flags);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

  public static IntPtr FindRoot(uint processId) {
    IntPtr result = IntPtr.Zero;
    EnumWindows((hwnd, _) => {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (owner == processId && IsWindowVisible(hwnd)) {
        result = hwnd;
        return false;
      }
      return true;
    }, IntPtr.Zero);
    return result;
  }

  public static bool NativeControlIsVisible(IntPtr root, int id) {
    IntPtr control = GetDlgItem(root, id);
    return control != IntPtr.Zero && IsWindowVisible(control);
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

  public static Rect Bounds(IntPtr hwnd) {
    Rect rect;
    GetWindowRect(hwnd, out rect);
    return rect;
  }

  public static string ClassName(IntPtr hwnd) {
    var text = new StringBuilder(128);
    GetClassName(hwnd, text, text.Capacity);
    return text.ToString();
  }

  public static string[] VisibleTexts(IntPtr root) {
    var values = new List<string>();
    EnumChildWindows(root, (hwnd, _) => {
      if (IsWindowVisible(hwnd)) {
        var text = new StringBuilder(256);
        GetWindowText(hwnd, text, text.Capacity);
        if (text.Length > 0) values.Add(text.ToString());
      }
      return true;
    }, IntPtr.Zero);
    return values.ToArray();
  }

  public static void Click(IntPtr hwnd) {
    Rect rect = Bounds(hwnd);
    int x = Math.Max(1, (rect.Right - rect.Left) / 2);
    int y = Math.Max(1, (rect.Bottom - rect.Top) / 2);
    IntPtr point = new IntPtr((y << 16) | (x & 0xffff));
    SendMessage(hwnd, 0x0201, new IntPtr(1), point);
    SendMessage(hwnd, 0x0202, IntPtr.Zero, point);
  }

  public static void PutBehind(IntPtr hwnd) {
    SetWindowPos(hwnd, new IntPtr(1), 0, 0, 0, 0, 0x13);
  }

  public static void Close(IntPtr hwnd) {
    PostMessage(hwnd, 0x0010, IntPtr.Zero, IntPtr.Zero);
  }
}
'@

$Process = Start-Process -FilePath $ResolvedSetup -ArgumentList "/SCREEN=welcome" -PassThru
$Root = [IntPtr]::Zero
try {
  for ($attempt = 0; $attempt -lt ($WindowTimeoutSeconds * 10) -and $Root -eq [IntPtr]::Zero; $attempt++) {
    Start-Sleep -Milliseconds 100
    $Root = [MediaDropSetupProbe]::FindRoot([uint32]$Process.Id)
  }
  if ($Root -eq [IntPtr]::Zero) { throw "Setup window did not appear." }
  [MediaDropSetupProbe]::PutBehind($Root)

  foreach ($id in @(1, 2, 3, 1028)) {
    if ([MediaDropSetupProbe]::NativeControlIsVisible($Root, $id)) {
      throw "Native NSIS control remained visible: $id"
    }
  }

  $Start = [MediaDropSetupProbe]::DeepChildAt($Root, 974, 589)
  $StartBounds = [MediaDropSetupProbe]::Bounds($Start)
  if ([MediaDropSetupProbe]::ClassName($Start) -ne "Static" -or
      ($StartBounds.Right - $StartBounds.Left) -ne 180 -or
      ($StartBounds.Bottom - $StartBounds.Top) -ne 46) {
    throw "The install action is blocked by another control."
  }

  $Toggle = [MediaDropSetupProbe]::DeepChildAt($Root, 700, 514)
  $ToggleBounds = [MediaDropSetupProbe]::Bounds($Toggle)
  if (($ToggleBounds.Right - $ToggleBounds.Left) -ne 582 -or
      ($ToggleBounds.Bottom - $ToggleBounds.Top) -ne 54) {
    throw "The extension toggle is blocked by another control."
  }

  $Close = [MediaDropSetupProbe]::DeepChildAt($Root, 1083, 32)
  $CloseBounds = [MediaDropSetupProbe]::Bounds($Close)
  if (($CloseBounds.Right - $CloseBounds.Left) -ne 38 -or
      ($CloseBounds.Bottom - $CloseBounds.Top) -ne 38) {
    throw "The close action is blocked by another control."
  }

  if (-not $InspectOnly) {
    [MediaDropSetupProbe]::Click($Start)
    Start-Sleep -Milliseconds 350
    $ProgressVisible = @([MediaDropSetupProbe]::VisibleTexts($Root) | Where-Object { $_ -match '^\d{1,3}%$' }).Count -gt 0
    if (-not $ProgressVisible) { throw "Clicking the install action did not open the custom progress screen." }
  }

  Write-Output "MediaDrop setup interaction smoke test passed."
}
finally {
  if ($Root -ne [IntPtr]::Zero) { [MediaDropSetupProbe]::Close($Root) }
  Start-Sleep -Milliseconds 400
  if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force }
}
