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

if (-not $SkipOsCheck) {
  $os = [Environment]::OSVersion.Version
  Assert-Release ([Environment]::Is64BitOperatingSystem) "MediaDrop 1.0.0 requires 64-bit Windows."
  Assert-Release ($os.Major -eq 10 -and $os.Build -ge 19045) "Release builds are supported on Windows 10 22H2 and Windows 11."
}

$config = Get-Content -LiteralPath (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
Assert-Release ($config.bundle.windows.webviewInstallMode.type -eq "embedBootstrapper") "WebView2 bootstrapper is not embedded."
Assert-Release (@($config.bundle.targets) -contains "msi") "Windows MSI target is missing."
Assert-Release (@($config.bundle.externalBin).Count -eq 7) "Expected seven MediaDrop sidecars."

$sidecarLock = Get-Content -LiteralPath (Join-Path $Root "src-tauri\binaries\sidecars.lock.json") -Raw | ConvertFrom-Json
foreach ($entry in @($sidecarLock.sidecars)) {
  $binary = Join-Path $Root ("src-tauri\binaries\" + [string]$entry.file)
  Assert-Release (Test-Path -LiteralPath $binary -PathType Leaf) "Sidecar is missing: $($entry.file)"
  Test-X64Pe $binary
}

$nativeHost = Join-Path $Root "src-tauri\target\release\mediadrop-native-host.exe"
if (Test-Path -LiteralPath $nativeHost -PathType Leaf) { Test-X64Pe $nativeHost }

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
}

if (-not [string]::IsNullOrWhiteSpace($ExtractedMsiPath)) {
  $extractRoot = [IO.Path]::GetFullPath($ExtractedMsiPath)
  $appExecutables = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "mediadrop.exe" -File)
  $hostExecutables = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "mediadrop-native-host.exe" -File)
  Assert-Release ($appExecutables.Count -ge 1 -and $hostExecutables.Count -ge 1) "Extracted MSI is missing MediaDrop executables."
  $appExecutable = $appExecutables[0]
  $hostExecutable = $hostExecutables[0]
  Test-X64Pe $appExecutable.FullName
  Test-X64Pe $hostExecutable.FullName
  Assert-Release (@(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "manifest.json" -File | Where-Object { $_.FullName -match 'browser-extension' }).Count -ge 1) "Extracted MSI is missing the bundled extension."
  Assert-Release (@(Get-ChildItem -LiteralPath $extractRoot -Recurse -Filter "THIRD_PARTY_NOTICES.md" -File).Count -ge 1) "Extracted MSI is missing third-party notices."
}

Write-Host "Compatibility OK: Windows 10 22H2/Windows 11 x64, WebView2 bootstrapper, stable extension host and portable paths." -ForegroundColor Green
