param(
  [Parameter(Mandatory = $true)]
  [string]$MsiPath,
  [string]$Version = "1.0.0",
  [string]$OutputDirectory = "",
  [switch]$Preview
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$ResolvedMsi = (Resolve-Path -LiteralPath $MsiPath -ErrorAction Stop).Path
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  throw "Setup version is invalid: $Version"
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $Root "artifacts"
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$ResolvedOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
$OutputPath = Join-Path $ResolvedOutput "MediaDrop-Setup-$Version.exe"

$MakeNsisCandidates = @(
  (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"),
  (Join-Path $env:LOCALAPPDATA "tauri\NSIS\Bin\makensis.exe"),
  (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
  (Join-Path $env:ProgramFiles "NSIS\makensis.exe")
)
$MakeNsis = $MakeNsisCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $MakeNsis) {
  $MakeNsisCommand = Get-Command "makensis.exe" -ErrorAction SilentlyContinue
  if ($MakeNsisCommand) { $MakeNsis = $MakeNsisCommand.Source }
}
if (-not $MakeNsis) {
  throw "makensis.exe bulunamadı. Release scripti Tauri NSIS aracını hazırladıktan sonra tekrar deneyin."
}

$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("mediadrop-setup-" + [guid]::NewGuid().ToString("N"))
$AssetRoot = Join-Path $TempRoot "assets"
$LogoFrameRoot = Join-Path $AssetRoot "logo"
New-Item -ItemType Directory -Path $LogoFrameRoot -Force | Out-Null

function New-RoundedRectanglePath {
  param([int]$X, [int]$Y, [int]$Width, [int]$Height, [int]$Radius)
  $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
  $diameter = $Radius * 2
  $path.AddArc($X, $Y, $diameter, $diameter, 180, 90)
  $path.AddArc($X + $Width - $diameter, $Y, $diameter, $diameter, 270, 90)
  $path.AddArc($X + $Width - $diameter, $Y + $Height - $diameter, $diameter, $diameter, 0, 90)
  $path.AddArc($X, $Y + $Height - $diameter, $diameter, $diameter, 90, 90)
  $path.CloseFigure()
  return $path
}

try {
  Add-Type -AssemblyName System.Drawing

  $SourceAssetRoot = Join-Path $Root "installer\assets"
  $SourceScreens = @("welcome", "installing", "extension", "done", "error")
  foreach ($screen in $SourceScreens) {
    $source = Join-Path $SourceAssetRoot "setup-$screen.png"
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      throw "Setup screen asset is missing: $source"
    }
  }

  $FontRoot = Join-Path $Root "src\assets\fonts"
  $FontFiles = @(
    "InstrumentSans-Regular.ttf",
    "InstrumentSans-Medium.ttf",
    "InstrumentSans-SemiBold.ttf",
    "InstrumentSans-Bold.ttf"
  )
  foreach ($font in $FontFiles) {
    Copy-Item -LiteralPath (Join-Path $FontRoot $font) -Destination $AssetRoot
  }

  $fontCollection = [System.Drawing.Text.PrivateFontCollection]::new()
  try {
    $fontCollection.AddFontFile((Join-Path $FontRoot "InstrumentSans-Regular.ttf"))
    $versionFont = [System.Drawing.Font]::new(
      $fontCollection.Families[0],
      11,
      [System.Drawing.FontStyle]::Regular,
      [System.Drawing.GraphicsUnit]::Pixel
    )
    $versionBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(170, 169, 164))
    $titlebarBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(16, 17, 20))
    try {
      foreach ($screen in $SourceScreens) {
        $sourcePath = Join-Path $SourceAssetRoot "setup-$screen.png"
        $screenOutputPath = Join-Path $AssetRoot "screen-$screen.bmp"
        $sourceImage = [System.Drawing.Image]::FromFile($sourcePath)
        try {
          if ($sourceImage.Width -ne 1120 -or $sourceImage.Height -ne 650) {
            throw "Setup screen must be exactly 1120x650: $sourcePath"
          }
          $bitmap = [System.Drawing.Bitmap]::new(1120, 650, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
          $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
          try {
            $graphics.DrawImageUnscaled($sourceImage, 0, 0)
            $graphics.FillRectangle($titlebarBrush, 61, 34, 150, 20)
            $graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
            $graphics.DrawString("Kurulum · $Version", $versionFont, $versionBrush, 65, 37)
            $bitmap.Save($screenOutputPath, [System.Drawing.Imaging.ImageFormat]::Bmp)
          } finally {
            $graphics.Dispose()
            $bitmap.Dispose()
          }
        } finally {
          $sourceImage.Dispose()
        }
      }
    } finally {
      $titlebarBrush.Dispose()
      $versionBrush.Dispose()
      $versionFont.Dispose()
    }
  } finally {
    $fontCollection.Dispose()
  }

  $installingSource = [System.Drawing.Image]::FromFile((Join-Path $SourceAssetRoot "setup-installing.png"))
  $logoSource = [System.Drawing.Image]::FromFile((Join-Path $Root "src-tauri\icons\icon.png"))
  try {
    $logoBase = [System.Drawing.Bitmap]::new(170, 170, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $baseGraphics = [System.Drawing.Graphics]::FromImage($logoBase)
    try {
      $baseGraphics.DrawImage(
        $installingSource,
        [System.Drawing.Rectangle]::new(0, 0, 170, 170),
        [System.Drawing.Rectangle]::new(128, 169, 170, 170),
        [System.Drawing.GraphicsUnit]::Pixel
      )
    } finally {
      $baseGraphics.Dispose()
    }

    try {
      for ($progress = 0; $progress -le 100; $progress++) {
        $visualProgress = [Math]::Max(2, $progress)
        $fillHeight = [Math]::Max(1, [Math]::Ceiling(142 * $visualProgress / 100))
        $sourceY = [Math]::Floor($logoSource.Height * (100 - $visualProgress) / 100)
        $sourceHeight = $logoSource.Height - $sourceY
        $frame = $logoBase.Clone()
        $frameGraphics = [System.Drawing.Graphics]::FromImage($frame)
        try {
          $frameGraphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
          $frameGraphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
          $frameGraphics.DrawImage(
            $logoSource,
            [System.Drawing.Rectangle]::new(14, 14 + 142 - $fillHeight, 142, $fillHeight),
            [System.Drawing.Rectangle]::new(0, $sourceY, $logoSource.Width, $sourceHeight),
            [System.Drawing.GraphicsUnit]::Pixel
          )
          $frame.Save((Join-Path $LogoFrameRoot ("logo-{0:D3}.bmp" -f $progress)), [System.Drawing.Imaging.ImageFormat]::Bmp)
        } finally {
          $frameGraphics.Dispose()
          $frame.Dispose()
        }
      }
    } finally {
      $logoBase.Dispose()
    }
  } finally {
    $logoSource.Dispose()
    $installingSource.Dispose()
  }

  foreach ($state in @("on", "off")) {
    $toggle = [System.Drawing.Bitmap]::new(46, 26, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $graphics = [System.Drawing.Graphics]::FromImage($toggle)
    try {
      $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
      $graphics.Clear([System.Drawing.Color]::FromArgb(23, 24, 28))
      $trackColor = if ($state -eq "on") { [System.Drawing.Color]::FromArgb(223, 163, 38) } else { [System.Drawing.Color]::FromArgb(44, 45, 50) }
      $knobColor = if ($state -eq "on") { [System.Drawing.Color]::FromArgb(23, 21, 18) } else { [System.Drawing.Color]::FromArgb(179, 178, 173) }
      $trackBrush = [System.Drawing.SolidBrush]::new($trackColor)
      $knobBrush = [System.Drawing.SolidBrush]::new($knobColor)
      $trackPath = New-RoundedRectanglePath -X 0 -Y 0 -Width 46 -Height 26 -Radius 13
      try {
        $graphics.FillPath($trackBrush, $trackPath)
        $knobX = if ($state -eq "on") { 23 } else { 3 }
        $graphics.FillEllipse($knobBrush, $knobX, 3, 20, 20)
        $toggle.Save((Join-Path $AssetRoot "toggle-$state.bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp)
      } finally {
        $trackPath.Dispose()
        $knobBrush.Dispose()
        $trackBrush.Dispose()
      }
    } finally {
      $graphics.Dispose()
      $toggle.Dispose()
    }
  }

  $contract = [ordered]@{
    schemaVersion = 1
    window = [ordered]@{ width = 1120; height = 650; fixedAcrossScreens = $true }
    screens = @("welcome", "installing", "extension", "done", "error")
    progress = [ordered]@{ startsAt = 0; visualFloor = 2; logoLinked = $true }
    defaults = [ordered]@{ launchApp = $true; extensionSetup = $false }
    extensionInstall = [ordered]@{
      mode = "guidedSideload"
      staysInInstaller = $true
      opensInApp = $false
      detectsInstalledBrowsers = $true
      putsDefaultBrowserFirst = $true
      detectsNativeConnection = $true
      supportedBrowsers = @("opera_gx", "opera", "chrome", "edge")
    }
    actions = @(
      "installMsi",
      "launchApp",
      "openBrowserExtensions",
      "copyExtensionPath",
      "continueWithoutExtension",
      "retry",
      "openLog"
    )
  }
  [System.IO.File]::WriteAllText(
    (Join-Path $AssetRoot "ui-contract.json"),
    ($contract | ConvertTo-Json -Depth 4),
    [System.Text.UTF8Encoding]::new($false)
  )

  $SetupScript = Join-Path $Root "installer\setup.nsi"
  $IconPath = Join-Path $Root "src-tauri\icons\icon.ico"
  $arguments = @(
    "/INPUTCHARSET",
    "UTF8",
    "/WX",
    "/DAPP_VERSION=$Version",
    "/DMSI_PATH=$ResolvedMsi",
    "/DOUTPUT_PATH=$OutputPath",
    "/DAPP_ICON=$IconPath",
    "/DASSET_DIR=$AssetRoot"
  )
  if ($Preview) { $arguments += "/DPREVIEW_MODE=1" }
  $arguments += $SetupScript

  & $MakeNsis @arguments
  if ($LASTEXITCODE -ne 0) {
    throw "MediaDrop setup build failed with exit code $LASTEXITCODE."
  }
  if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
    throw "MediaDrop setup output was not created: $OutputPath"
  }
  Write-Output $OutputPath
} finally {
  if (Test-Path -LiteralPath $TempRoot) {
    $resolvedTemp = [System.IO.Path]::GetFullPath($TempRoot)
    $resolvedSystemTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedTemp.StartsWith($resolvedSystemTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
      throw "Refusing to remove setup temp directory outside the system temp root: $resolvedTemp"
    }
    Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
  }
}
