param(
  [Parameter(Mandatory = $true)]
  [string]$MsiPath,
  [string]$Version = "1.0.1",
  [string]$OutputDirectory = "",
  [switch]$Preview,
  [switch]$LifecycleTest
)

$ErrorActionPreference = "Stop"
if ($Preview -and $LifecycleTest) { throw "Preview and LifecycleTest are separate setup variants." }
$TestEngine = [bool]($Preview -or $LifecycleTest)
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

function Get-Sha256Hex {
  param([Parameter(Mandatory = $true)][string]$Path)
  $algorithm = [System.Security.Cryptography.SHA256]::Create()
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    return [System.BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace("-", "").ToLowerInvariant()
  } finally {
    $stream.Dispose()
    $algorithm.Dispose()
  }
}

function Get-MsiMetadata {
  param([Parameter(Mandatory = $true)][string]$Path)

  $installer = $null
  $database = $null
  $summary = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.GetType().InvokeMember(
      "OpenDatabase",
      [System.Reflection.BindingFlags]::InvokeMethod,
      $null,
      $installer,
      @($Path, 0)
    )

    function Read-MsiProperty([string]$Name) {
      $view = $null
      $record = $null
      try {
        $query = "SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$Name'"
        $view = $database.GetType().InvokeMember(
          "OpenView",
          [System.Reflection.BindingFlags]::InvokeMethod,
          $null,
          $database,
          @($query)
        )
        $view.GetType().InvokeMember("Execute", [System.Reflection.BindingFlags]::InvokeMethod, $null, $view, $null) | Out-Null
        $record = $view.GetType().InvokeMember("Fetch", [System.Reflection.BindingFlags]::InvokeMethod, $null, $view, $null)
        if ($null -eq $record) { throw "MSI property is missing: $Name" }
        return [string]$record.GetType().InvokeMember(
          "StringData",
          [System.Reflection.BindingFlags]::GetProperty,
          $null,
          $record,
          @(1)
        )
      } finally {
        if ($null -ne $record) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($record) | Out-Null }
        if ($null -ne $view) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($view) | Out-Null }
      }
    }

    $summary = $database.GetType().InvokeMember(
      "SummaryInformation",
      [System.Reflection.BindingFlags]::GetProperty,
      $null,
      $database,
      @(0)
    )
    $template = [string]$summary.GetType().InvokeMember(
      "Property",
      [System.Reflection.BindingFlags]::GetProperty,
      $null,
      $summary,
      @(7)
    )
    return [pscustomobject]@{
      ProductName = Read-MsiProperty "ProductName"
      Manufacturer = Read-MsiProperty "Manufacturer"
      ProductVersion = Read-MsiProperty "ProductVersion"
      UpgradeCode = Read-MsiProperty "UpgradeCode"
      Template = $template
    }
  } finally {
    if ($null -ne $summary) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary) | Out-Null }
    if ($null -ne $database) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) | Out-Null }
    if ($null -ne $installer) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) | Out-Null }
  }
}

function Assert-WorkerBinary {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][bool]$AllowTestEngine,
    [Parameter(Mandatory = $true)][string]$ExpectedVersion,
    [Parameter(Mandatory = $true)][string]$ExpectedMsiSha256
  )
  $stream = [System.IO.File]::OpenRead($Path)
  $reader = [System.IO.BinaryReader]::new($stream)
  try {
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Installer worker is not a PE executable: $Path" }
    $stream.Position = 0x3C
    $peOffset = $reader.ReadInt32()
    if ($peOffset -lt 0x40 -or $peOffset -gt ($stream.Length - 96)) {
      throw "Installer worker has an invalid PE header: $Path"
    }
    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) { throw "Installer worker PE signature is invalid: $Path" }
    if ($reader.ReadUInt16() -ne 0x8664) { throw "Installer worker must be x64: $Path" }
    $stream.Position = $peOffset + 92
    if ($reader.ReadUInt16() -ne 2) { throw "Installer worker must use the Windows GUI subsystem: $Path" }
  } finally {
    $reader.Dispose()
    $stream.Dispose()
  }

  $binaryText = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($Path))
  if (-not $binaryText.Contains("asInvoker")) { throw "Installer worker manifest is not asInvoker: $Path" }
  if ($binaryText.Contains("requireAdministrator")) { throw "Installer worker unexpectedly requires elevation at startup: $Path" }
  if ($binaryText -match '(?i)VCRUNTIME\d*\.dll|MSVCP\d*\.dll') {
    throw "Installer worker depends on the dynamic MSVC runtime: $Path"
  }
  if (-not $AllowTestEngine -and $binaryText.Contains("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO")) {
    throw "Production installer worker contains the test engine: $Path"
  }
  if (-not $binaryText.Contains($ExpectedMsiSha256.ToLowerInvariant())) {
    throw "Installer worker is not bound to the selected MSI hash: $Path"
  }
  $versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
  if ($versionInfo.ProductName -ne "MediaDrop" -or
      $versionInfo.ProductVersion -ne $ExpectedVersion -or
      $versionInfo.FileDescription -ne "MediaDrop Kurulum Hizmeti") {
    throw "Installer worker version resource does not match MediaDrop $ExpectedVersion."
  }
}

function Assert-SetupBinary {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$ExpectedVersion
  )
  $stream = [IO.File]::OpenRead($Path)
  $reader = [IO.BinaryReader]::new($stream)
  try {
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Setup is not a PE executable: $Path" }
    $prefix = New-Object byte[] ([Math]::Min(1048576, [int]$stream.Length))
    $stream.Position = 0
    $read = $stream.Read($prefix, 0, $prefix.Length)
    $text = [Text.Encoding]::ASCII.GetString($prefix, 0, $read)
    if (-not $text.Contains("asInvoker")) { throw "Setup manifest is not asInvoker: $Path" }
    if ($text.Contains("requireAdministrator")) { throw "Setup unexpectedly requires elevation at startup: $Path" }
  } finally {
    $reader.Dispose()
    $stream.Dispose()
  }
  $versionInfo = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
  if ($versionInfo.ProductName -ne "MediaDrop" -or
      $versionInfo.ProductVersion -ne $ExpectedVersion -or
      $versionInfo.FileDescription -ne "MediaDrop offline installer") {
    throw "Setup version resource does not match MediaDrop $ExpectedVersion."
  }
}

$MsiItem = Get-Item -LiteralPath $ResolvedMsi
$MsiMetadata = if ($TestEngine) {
  [pscustomobject]@{
    ProductName = "MediaDrop"
    Manufacturer = "mab"
    ProductVersion = $Version
    UpgradeCode = "{8585B38D-5F90-4110-B089-6B89A3FB6339}"
    Template = "x64;0"
  }
} else {
  Get-MsiMetadata -Path $ResolvedMsi
}
if ($MsiMetadata.ProductName -ne "MediaDrop" -or
    $MsiMetadata.Manufacturer -ne "mab" -or
    $MsiMetadata.ProductVersion -ne $Version -or
    $MsiMetadata.UpgradeCode -ne "{8585B38D-5F90-4110-B089-6B89A3FB6339}" -or
    $MsiMetadata.Template -ne "x64;0") {
  throw "MSI identity does not match the MediaDrop $Version x64 product contract."
}
$env:MEDIADROP_MSI_SIZE = [string]$MsiItem.Length
$env:MEDIADROP_MSI_SHA256 = Get-Sha256Hex -Path $ResolvedMsi
$env:MEDIADROP_MSI_PRODUCT_NAME = $MsiMetadata.ProductName
$env:MEDIADROP_MSI_MANUFACTURER = $MsiMetadata.Manufacturer
$env:MEDIADROP_MSI_PRODUCT_VERSION = $MsiMetadata.ProductVersion
$env:MEDIADROP_MSI_UPGRADE_CODE = $MsiMetadata.UpgradeCode
$env:MEDIADROP_MSI_TEMPLATE = $MsiMetadata.Template
$env:RUSTFLAGS = (($env:RUSTFLAGS + " -C target-feature=+crt-static").Trim())
$WorkerBuildKind = "production"
if ($TestEngine) { $WorkerBuildKind = "test-engine" }
$WorkerTargetRoot = Join-Path $Root "installer\worker\target\$WorkerBuildKind"
$CargoArguments = @(
  "build",
  "--manifest-path", (Join-Path $Root "installer\worker\Cargo.toml"),
  "--target-dir", $WorkerTargetRoot,
  "--target", "x86_64-pc-windows-msvc",
  "--release",
  "--locked"
)
if ($TestEngine) { $CargoArguments += @("--features", "test-engine") }
& cargo.exe @CargoArguments
if ($LASTEXITCODE -ne 0) {
  throw "Installer worker build failed with exit code $LASTEXITCODE."
}
$WorkerPath = Join-Path $WorkerTargetRoot "x86_64-pc-windows-msvc\release\mediadrop-installer-worker.exe"
if (-not (Test-Path -LiteralPath $WorkerPath -PathType Leaf)) {
  throw "Installer worker output was not created: $WorkerPath"
}
Assert-WorkerBinary -Path $WorkerPath -AllowTestEngine $TestEngine -ExpectedVersion $Version -ExpectedMsiSha256 $env:MEDIADROP_MSI_SHA256

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
            if ($screen -eq "extension") {
              $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
              $graphics.FillRectangle($titlebarBrush, 970, 8, 93, 48)

              $minimizePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(170, 169, 164), 1.4)
              $secondaryBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(37, 38, 43))
              $panelBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(23, 24, 28))
              $statusBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(32, 33, 38))
              $secondaryBorder = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(52, 53, 59), 1)
              $primaryBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(245, 199, 93))
              $copyPath = New-RoundedRectanglePath -X 610 -Y 566 -Width 132 -Height 46 -Radius 9
              $revealPath = New-RoundedRectanglePath -X 752 -Y 566 -Width 132 -Height 46 -Radius 9
              $primaryPath = New-RoundedRectanglePath -X 894 -Y 566 -Width 170 -Height 46 -Radius 9
              try {
                $graphics.DrawLine($minimizePen, 1033, 31, 1043, 31)
                $graphics.FillRectangle($statusBrush, 494, 488, 556, 47)
                $graphics.FillRectangle($panelBrush, 606, 562, 462, 54)
                $graphics.FillPath($secondaryBrush, $copyPath)
                $graphics.DrawPath($secondaryBorder, $copyPath)
                $graphics.FillPath($secondaryBrush, $revealPath)
                $graphics.DrawPath($secondaryBorder, $revealPath)
                $graphics.FillPath($primaryBrush, $primaryPath)
              } finally {
                $primaryPath.Dispose()
                $revealPath.Dispose()
                $copyPath.Dispose()
                $primaryBrush.Dispose()
                $secondaryBorder.Dispose()
                $statusBrush.Dispose()
                $panelBrush.Dispose()
                $secondaryBrush.Dispose()
                $minimizePen.Dispose()
              }
            }
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

  for ($frame = 0; $frame -le 6; $frame++) {
    $ratio = $frame / 6.0
    $toggle = [System.Drawing.Bitmap]::new(46, 26, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $graphics = [System.Drawing.Graphics]::FromImage($toggle)
    try {
      $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
      $graphics.Clear([System.Drawing.Color]::FromArgb(23, 24, 28))
      $trackColor = [System.Drawing.Color]::FromArgb(
        [int][Math]::Round(44 + ((223 - 44) * $ratio)),
        [int][Math]::Round(45 + ((163 - 45) * $ratio)),
        [int][Math]::Round(50 + ((38 - 50) * $ratio))
      )
      $knobColor = [System.Drawing.Color]::FromArgb(
        [int][Math]::Round(179 + ((23 - 179) * $ratio)),
        [int][Math]::Round(178 + ((21 - 178) * $ratio)),
        [int][Math]::Round(173 + ((18 - 173) * $ratio))
      )
      $trackBrush = [System.Drawing.SolidBrush]::new($trackColor)
      $knobBrush = [System.Drawing.SolidBrush]::new($knobColor)
      $trackPath = New-RoundedRectanglePath -X 0 -Y 0 -Width 46 -Height 26 -Radius 13
      try {
        $graphics.FillPath($trackBrush, $trackPath)
        $knobX = 3 + [int][Math]::Round(20 * $ratio)
        $graphics.FillEllipse($knobBrush, $knobX, 3, 20, 20)
        $toggle.Save((Join-Path $AssetRoot "toggle-$frame.bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp)
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

  $hoverButtons = @(
    @{ Name = "minimize"; Screen = "welcome"; X = 1022; Y = 13; Width = 38; Height = 38 },
    @{ Name = "close"; Screen = "welcome"; X = 1064; Y = 13; Width = 38; Height = 38 },
    @{ Name = "start"; Screen = "welcome"; X = 884; Y = 566; Width = 180; Height = 46 },
    @{ Name = "cancel"; Screen = "installing"; X = 981; Y = 566; Width = 83; Height = 46 },
    @{ Name = "browser_0"; Screen = "extension"; X = 482; Y = 228; Width = 136; Height = 45 },
    @{ Name = "browser_1"; Screen = "extension"; X = 630; Y = 228; Width = 136; Height = 45 },
    @{ Name = "browser_2"; Screen = "extension"; X = 778; Y = 228; Width = 136; Height = 45 },
    @{ Name = "browser_3"; Screen = "extension"; X = 926; Y = 228; Width = 136; Height = 45 },
    @{ Name = "extension_later"; Screen = "extension"; X = 482; Y = 566; Width = 116; Height = 46 },
    @{ Name = "extension_copy"; Screen = "extension"; X = 610; Y = 566; Width = 132; Height = 46 },
    @{ Name = "extension_reveal"; Screen = "extension"; X = 752; Y = 566; Width = 132; Height = 46 },
    @{ Name = "extension_primary"; Screen = "extension"; X = 894; Y = 566; Width = 170; Height = 46 },
    @{ Name = "summary"; Screen = "done"; X = 482; Y = 566; Width = 126; Height = 46 },
    @{ Name = "finish"; Screen = "done"; X = 884; Y = 566; Width = 180; Height = 46 },
    @{ Name = "log"; Screen = "error"; X = 482; Y = 566; Width = 146; Height = 46 },
    @{ Name = "give_up"; Screen = "error"; X = 789; Y = 566; Width = 85; Height = 46 },
    @{ Name = "retry"; Screen = "error"; X = 884; Y = 566; Width = 180; Height = 46 }
  )
  $hoverExpandX = @(0, 0, 1, 1, 2)
  $hoverExpandY = @(0, 0, 0, 1, 1)
  foreach ($button in $hoverButtons) {
    $sourceImage = [System.Drawing.Image]::FromFile((Join-Path $AssetRoot "screen-$($button.Screen).bmp"))
    try {
      $canvasWidth = $button.Width + 4
      $canvasHeight = $button.Height + 2
      for ($frame = 1; $frame -le 4; $frame++) {
        $hover = [System.Drawing.Bitmap]::new($canvasWidth, $canvasHeight, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
        $graphics = [System.Drawing.Graphics]::FromImage($hover)
        try {
          $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
          $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
          $graphics.DrawImage(
            $sourceImage,
            [System.Drawing.Rectangle]::new(0, 0, $canvasWidth, $canvasHeight),
            [System.Drawing.Rectangle]::new($button.X - 2, $button.Y - 1, $canvasWidth, $canvasHeight),
            [System.Drawing.GraphicsUnit]::Pixel
          )
          $expandX = $hoverExpandX[$frame]
          $expandY = $hoverExpandY[$frame]
          $graphics.DrawImage(
            $sourceImage,
            [System.Drawing.Rectangle]::new(2 - $expandX, 1 - $expandY, $button.Width + (2 * $expandX), $button.Height + (2 * $expandY)),
            [System.Drawing.Rectangle]::new($button.X, $button.Y, $button.Width, $button.Height),
            [System.Drawing.GraphicsUnit]::Pixel
          )
          $hover.Save((Join-Path $AssetRoot "hover-$($button.Name)-$frame.bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp)
        } finally {
          $graphics.Dispose()
          $hover.Dispose()
        }
      }
    } finally {
      $sourceImage.Dispose()
    }
  }

  $contract = [ordered]@{
    schemaVersion = 1
    window = [ordered]@{ width = 1120; height = 650; fixedAcrossScreens = $true }
    screens = @("welcome", "installing", "extension", "done", "error")
    progress = [ordered]@{ startsAt = 0; visualFloor = 2; logoLinked = $true }
    motion = [ordered]@{
      hoverFeedback = $true
      hoverStyle = "buttonScale"
      hoverFrames = 4
      toggleFrames = 7
      respectsReducedMotion = $true
    }
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
      "revealExtensionFolder",
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
    "/DWORKER_PATH=$WorkerPath",
    "/DOUTPUT_PATH=$OutputPath",
    "/DAPP_ICON=$IconPath",
    "/DASSET_DIR=$AssetRoot"
  )
  if ($TestEngine) { $arguments += "/DUI_TEST_MODE=1" }
  if ($Preview) { $arguments += "/DPREVIEW_MODE=1" }
  if ($LifecycleTest) { $arguments += "/DLIFECYCLE_TEST_MODE=1" }
  $arguments += $SetupScript

  & $MakeNsis @arguments
  if ($LASTEXITCODE -ne 0) {
    throw "MediaDrop setup build failed with exit code $LASTEXITCODE."
  }
  if (-not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
    throw "MediaDrop setup output was not created: $OutputPath"
  }
  Assert-SetupBinary -Path $OutputPath -ExpectedVersion $Version
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
