param(
  [string]$ArtifactDirectory = "",
  [string]$ExtractedMsiPath = "",
  [switch]$SkipOsCheck
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$ExpectedExtensionOrigin = "chrome-extension://gifnifkakikpndieohkijmjccmmikalm/"

function Assert-Release([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function Get-Sha256([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace("-", "") }
  finally {
    $sha.Dispose()
    $stream.Dispose()
  }
}

function Test-X64Pe([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  $reader = New-Object IO.BinaryReader($stream)
  try {
    Assert-Release ($reader.ReadUInt16() -eq 0x5A4D) "PE DOS header is invalid: $Path"
    $stream.Position = 0x3C
    $peOffset = $reader.ReadInt32()
    Assert-Release ($peOffset -gt 0 -and $peOffset -lt $stream.Length - 6) "PE offset is invalid: $Path"
    $stream.Position = $peOffset
    Assert-Release ($reader.ReadUInt32() -eq 0x00004550) "PE signature is invalid: $Path"
    Assert-Release ($reader.ReadUInt16() -eq 0x8664) "Release executable is not Windows x64: $Path"
  } finally {
    $reader.Dispose()
    $stream.Dispose()
  }
}

function Get-AsciiPrefix([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  try {
    $buffer = New-Object byte[] ([Math]::Min(1048576, [int]$stream.Length))
    $read = $stream.Read($buffer, 0, $buffer.Length)
    [Text.Encoding]::ASCII.GetString($buffer, 0, $read)
  } finally {
    $stream.Dispose()
  }
}

function Test-AsInvokerManifest([string]$Path) {
  $text = Get-AsciiPrefix $Path
  Assert-Release ($text.Contains("asInvoker")) "Executable manifest is not asInvoker: $Path"
  Assert-Release (-not $text.Contains("requireAdministrator")) "Executable unexpectedly requires elevation at startup: $Path"
}

if (-not $SkipOsCheck) {
  $os = [Environment]::OSVersion.Version
  Assert-Release ([Environment]::Is64BitOperatingSystem) "MediaDrop 1.0.1 requires 64-bit Windows."
  Assert-Release ($os.Major -eq 10 -and $os.Build -ge 19045) "Release builds are supported on Windows 10 22H2 and Windows 11."
}

$config = Get-Content -LiteralPath (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
Assert-Release ($config.bundle.windows.webviewInstallMode.type -eq "embedBootstrapper") "WebView2 bootstrapper is not embedded."
Assert-Release (@($config.bundle.targets) -contains "msi") "Windows MSI target is missing."
Assert-Release (@($config.bundle.externalBin).Count -eq 8) "Expected eight MediaDrop sidecars."

$sidecarLock = Get-Content -LiteralPath (Join-Path $Root "src-tauri\binaries\sidecars.lock.json") -Raw | ConvertFrom-Json
foreach ($entry in @($sidecarLock.sidecars)) {
  $binary = Join-Path $Root ("src-tauri\binaries\" + [string]$entry.file)
  Assert-Release (Test-Path -LiteralPath $binary -PathType Leaf) "Sidecar is missing: $($entry.file)"
  Test-X64Pe $binary
}

$nativeHost = Join-Path $Root "src-tauri\target\release\mediadrop-native-host.exe"
if (Test-Path -LiteralPath $nativeHost -PathType Leaf) { Test-X64Pe $nativeHost }

$componentWorker = Join-Path $Root "src-tauri\binaries\mediadrop-component-worker-x86_64-pc-windows-msvc.exe"
Test-AsInvokerManifest $componentWorker
Assert-Release (-not (Get-AsciiPrefix $componentWorker).Contains("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO")) "Production component worker contains the test engine."

$installerWorker = Join-Path $Root "installer\worker\target\production\x86_64-pc-windows-msvc\release\mediadrop-installer-worker.exe"
if (Test-Path -LiteralPath $installerWorker -PathType Leaf) {
  Test-X64Pe $installerWorker
  Test-AsInvokerManifest $installerWorker
  Assert-Release (-not (Get-AsciiPrefix $installerWorker).Contains("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO")) "Production installer worker contains the test engine."
}

$hostManifestPath = Join-Path $Root "src-tauri\native-messaging\generated\com.mab.mediadrop.json"
Assert-Release (Test-Path -LiteralPath $hostManifestPath -PathType Leaf) "Production native-host manifest is missing."
$hostManifest = Get-Content -LiteralPath $hostManifestPath -Raw | ConvertFrom-Json
Assert-Release ($hostManifest.path -eq "mediadrop-native-host.exe") "Native-host path must remain install-relative."
Assert-Release (@($hostManifest.allowed_origins).Count -eq 1) "Native-host allowed_origins must contain exactly one production origin."
Assert-Release ($hostManifest.allowed_origins[0] -eq $ExpectedExtensionOrigin) "Native-host allowed_origins contains an unexpected extension ID."

$registryFragment = Get-Content -LiteralPath (Join-Path $Root "src-tauri\windows\fragments\native-messaging.wxs") -Raw
Assert-Release ($registryFragment -match 'Root="HKLM"' -and $registryFragment -notmatch 'Root="HKCU"') "Native Messaging registry must match the per-machine MSI scope."
Assert-Release ($registryFragment -match 'Google\\Chrome\\NativeMessagingHosts') "Chromium/Opera Native Messaging registry key is missing."
Assert-Release ($registryFragment -match 'Microsoft\\Edge\\NativeMessagingHosts') "Edge Native Messaging registry key is missing."
Assert-Release ($registryFragment -match 'App Paths\\mediadrop\.exe') "Stable MediaDrop App Paths registration is missing."

$extensionManifestPath = Join-Path $Root "browser-extension\dist\manifest.json"
Assert-Release (Test-Path -LiteralPath $extensionManifestPath -PathType Leaf) "Production extension build is missing."
$extensionManifest = Get-Content -LiteralPath $extensionManifestPath -Raw | ConvertFrom-Json
Assert-Release ($extensionManifest.manifest_version -eq 3 -and -not [string]::IsNullOrWhiteSpace([string]$extensionManifest.key)) "Stable MV3 extension key is missing."

$unicodeRoot = Join-Path ([IO.Path]::GetTempPath()) ("MediaDrop Uyumluluk İğüşöç " + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $unicodeRoot -Force | Out-Null
try {
  $helperSource = Join-Path $Root "src-tauri\binaries\instaloader-helper-x86_64-pc-windows-msvc.exe"
  $helperTarget = Join-Path $unicodeRoot "instaloader-helper.exe"
  Copy-Item -LiteralPath $helperSource -Destination $helperTarget
  & $helperTarget --help *> $null
  Assert-Release ($LASTEXITCODE -eq 0) "Instagram helper failed from a path containing spaces or Turkish characters."
  Copy-Item -LiteralPath $extensionManifestPath -Destination (Join-Path $unicodeRoot "manifest.json")
  $null = Get-Content -LiteralPath (Join-Path $unicodeRoot "manifest.json") -Raw | ConvertFrom-Json
} finally {
  if (Test-Path -LiteralPath $unicodeRoot) { Remove-Item -LiteralPath $unicodeRoot -Recurse -Force }
}

if (-not [string]::IsNullOrWhiteSpace($ArtifactDirectory)) {
  $artifactRoot = [IO.Path]::GetFullPath($ArtifactDirectory)
  foreach ($pattern in @("MediaDrop-Setup-*.exe", "MediaDrop_*_x64.msi", "MediaDrop_*_x64.msi.sig", "MediaDrop-Extension-*.zip", "latest.json", "SHA256SUMS.txt", "build-info.json")) {
    Assert-Release (@(Get-ChildItem -LiteralPath $artifactRoot -Filter $pattern -File -ErrorAction SilentlyContinue).Count -eq 1) "Release artifact is missing or ambiguous: $pattern"
  }
  $setup = @(Get-ChildItem -LiteralPath $artifactRoot -Filter "MediaDrop-Setup-*.exe" -File)[0]
  Test-AsInvokerManifest $setup.FullName
  $setupVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($setup.FullName)
  Assert-Release ($setupVersion.ProductName -eq "MediaDrop") "Setup version resource is invalid."

  $releaseVersion = [string]$config.version
  $publicMsi = Join-Path $artifactRoot "MediaDrop_$releaseVersion`_x64.msi"
  $publicSetup = Join-Path $artifactRoot "MediaDrop-Setup-$releaseVersion.exe"
  $publicExtension = Join-Path $artifactRoot "MediaDrop-Extension-$releaseVersion.zip"
  $latestPath = Join-Path $artifactRoot "latest.json"
  $buildInfoPath = Join-Path $artifactRoot "build-info.json"
  foreach ($path in @($publicMsi, "$publicMsi.sig", $publicSetup, $publicExtension, $latestPath, $buildInfoPath)) {
    Assert-Release (Test-Path -LiteralPath $path -PathType Leaf) "Expected versioned release artifact is missing: $path"
  }

  $buildInfo = Get-Content -LiteralPath $buildInfoPath -Raw | ConvertFrom-Json
  Assert-Release ([string]$buildInfo.version -eq $releaseVersion) "build-info version does not match the release version."
  Assert-Release ($buildInfo.sourceTreeClean -is [bool] -and $buildInfo.sourceTreeClean) "build-info does not prove a clean source tree."
  Assert-Release ([string]$buildInfo.sourceCommit -match '^[0-9a-f]{40}$') "build-info source commit is invalid."
  Assert-Release ([string]$buildInfo.buildFingerprint -match '^[A-F0-9]{64}$') "build-info build fingerprint is invalid."
  foreach ($expected in @(
    @{ Field = "msiSha256"; Path = $publicMsi },
    @{ Field = "signatureSha256"; Path = "$publicMsi.sig" },
    @{ Field = "setupSha256"; Path = $publicSetup },
    @{ Field = "extensionSha256"; Path = $publicExtension },
    @{ Field = "sidecarLockSha256"; Path = (Join-Path $Root "src-tauri\binaries\sidecars.lock.json") },
    @{ Field = "latestSha256"; Path = $latestPath }
  )) {
    Assert-Release ([string]$buildInfo.($expected.Field) -eq (Get-Sha256 $expected.Path)) "build-info $($expected.Field) does not match $($expected.Path)."
  }

  $latest = Get-Content -LiteralPath $latestPath -Raw | ConvertFrom-Json
  Assert-Release ([string]$latest.version -eq $releaseVersion) "Generated latest.json version does not match the release version."
  Assert-Release ([string]$latest.platforms."windows-x86_64".url -match [regex]::Escape("/v$releaseVersion/MediaDrop_$releaseVersion`_x64.msi")) "Generated latest.json does not reference the staged MSI."
  Assert-Release (-not [string]::IsNullOrWhiteSpace([string]$latest.platforms."windows-x86_64".signature)) "Generated latest.json updater signature is empty."
}

if (-not [string]::IsNullOrWhiteSpace($ExtractedMsiPath)) {
  $extractRoot = [IO.Path]::GetFullPath($ExtractedMsiPath)
  $appExecutables = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "mediadrop.exe" -File)
  $hostExecutables = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "mediadrop-native-host.exe" -File)
  $componentWorkers = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "mediadrop-component-worker.exe" -File)
  Assert-Release ($appExecutables.Count -ge 1 -and $hostExecutables.Count -ge 1 -and $componentWorkers.Count -ge 1) "Extracted MSI is missing MediaDrop executables."
  $appExecutable = $appExecutables[0]
  $hostExecutable = $hostExecutables[0]
  Test-X64Pe $appExecutable.FullName
  Test-X64Pe $hostExecutable.FullName
  Test-X64Pe $componentWorkers[0].FullName
  Test-AsInvokerManifest $componentWorkers[0].FullName
  Assert-Release (@(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "manifest.json" -File | Where-Object { $_.FullName -match 'browser-extension' }).Count -ge 1) "Extracted MSI is missing the bundled extension."
  Assert-Release (@(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "THIRD_PARTY_NOTICES.md" -File).Count -ge 1) "Extracted MSI is missing third-party notices."
}

Write-Host "Compatibility OK: Windows 10 22H2/Windows 11 x64, WebView2 bootstrapper, stable extension host and portable paths." -ForegroundColor Green
