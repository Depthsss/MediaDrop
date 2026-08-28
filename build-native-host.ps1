param(
  [switch]$Production,
  [string]$ExtensionId = "",
  [switch]$SkipCompile
)

$ErrorActionPreference = "Stop"
$BuildRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$TauriRoot = Join-Path $BuildRoot "src-tauri"
$ProductionExtensionId = "gifnifkakikpndieohkijmjccmmikalm"
$RequestedExtensionId = $ExtensionId.Trim()
if ($Production -and $RequestedExtensionId -and $RequestedExtensionId -ne $ProductionExtensionId) {
  throw "Production extension ID must match the public manifest key."
}
$ResolvedExtensionId = if ($RequestedExtensionId) { $RequestedExtensionId } else { $ProductionExtensionId }

if ($ResolvedExtensionId -notmatch '^[a-p]{32}$') {
  $mode = if ($Production) { "production" } else { "development" }
  throw "A valid $mode extension ID (32 characters, a-p) is required."
}

$HostName = if ($Production) { "com.mab.mediadrop" } else { "com.mab.mediadrop.dev" }
$Profile = if ($Production) { "release" } else { "debug" }
$ManifestName = "$HostName.json"
$TargetDirectory = Join-Path $TauriRoot "target\$Profile"
$HostExecutable = Join-Path $TargetDirectory "mediadrop-native-host.exe"

if (-not $SkipCompile) {
  $cargoArgs = @("build", "--locked", "--bin", "mediadrop-native-host")
  if ($Production) { $cargoArgs += "--release" }
  $PreviousTauriConfig = $env:TAURI_CONFIG
  try {
    $env:TAURI_CONFIG = '{"bundle":{"externalBin":[]}}'
    & cargo @cargoArgs --manifest-path (Join-Path $TauriRoot "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Native host build failed." }
  } finally {
    if ($null -eq $PreviousTauriConfig) {
      Remove-Item -LiteralPath Env:TAURI_CONFIG -ErrorAction SilentlyContinue
    } else {
      $env:TAURI_CONFIG = $PreviousTauriConfig
    }
  }
}

if (!(Test-Path -LiteralPath $HostExecutable -PathType Leaf)) {
  throw "Native host executable was not found: $HostExecutable"
}

$Manifest = [ordered]@{
  name = $HostName
  description = "MediaDrop browser companion bridge"
  path = "mediadrop-native-host.exe"
  type = "stdio"
  allowed_origins = @("chrome-extension://$ResolvedExtensionId/")
}
$ManifestJson = $Manifest | ConvertTo-Json -Depth 4
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$TargetManifest = Join-Path $TargetDirectory $ManifestName
[System.IO.File]::WriteAllText($TargetManifest, "$ManifestJson`n", $Utf8NoBom)

$GeneratedDirectory = Join-Path $TauriRoot "native-messaging\generated"
New-Item -ItemType Directory -Path $GeneratedDirectory -Force | Out-Null
$GeneratedManifest = Join-Path $GeneratedDirectory $ManifestName
[System.IO.File]::WriteAllText($GeneratedManifest, "$ManifestJson`n", $Utf8NoBom)

Write-Output $GeneratedManifest
