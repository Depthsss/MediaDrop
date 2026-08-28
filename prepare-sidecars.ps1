param(
  [switch]$VerifyOnly,
  [switch]$Force
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinaryDir = Join-Path $Root "src-tauri\binaries"
$LockPath = Join-Path $BinaryDir "sidecars.lock.json"
$CacheDir = Join-Path $Root ".cache\sidecars"

function Get-Sha256([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace("-", "") }
  finally {
    $sha.Dispose()
    $stream.Dispose()
  }
}

function Test-ValidEntry($Entry) {
  $hasBuildScript = $null -ne $Entry.PSObject.Properties["buildScript"]
  $hasDownload = $null -ne $Entry.PSObject.Properties["url"]
  if ([string]::IsNullOrWhiteSpace([string]$Entry.file) -or
      [IO.Path]::GetFileName([string]$Entry.file) -ne [string]$Entry.file -or
      $hasBuildScript -eq $hasDownload -or
      ($hasBuildScript -and [string]$Entry.buildScript -notmatch '^[^\\/:*?"<>|]+(?:[\\/][^\\/:*?"<>|]+)+\.ps1$') -or
      ($hasDownload -and ([string]$Entry.sha256 -notmatch '^[A-Fa-f0-9]{64}$' -or [string]$Entry.url -notmatch '^https://'))) {
    throw "sidecars.lock.json contains an invalid entry."
  }
}

function Get-CachePath([string]$Url) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($Url)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $key = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "") }
  finally { $sha.Dispose() }
  $extension = [IO.Path]::GetExtension(([Uri]$Url).AbsolutePath)
  Join-Path $CacheDir "$key$extension"
}

function Install-Sidecar($Entry, [string]$Target) {
  New-Item -ItemType Directory -Path $CacheDir -Force | Out-Null
  $cachePath = Get-CachePath ([string]$Entry.url)
  if (!(Test-Path -LiteralPath $cachePath -PathType Leaf)) {
    Write-Host "Downloading: $($Entry.name) $($Entry.version)" -ForegroundColor Cyan
    Invoke-WebRequest -Uri ([string]$Entry.url) -OutFile $cachePath -UseBasicParsing
  }

  $candidate = $cachePath
  $extractDir = $null
  if ([string]$Entry.archiveType -eq "zip") {
    $extractDir = Join-Path $CacheDir ("extract-" + [Guid]::NewGuid().ToString("N"))
    try {
      Expand-Archive -LiteralPath $cachePath -DestinationPath $extractDir -Force
      $matches = @(Get-ChildItem -LiteralPath $extractDir -Recurse -File |
        Where-Object { $_.Name -eq [string]$Entry.archiveMember })
      if ($matches.Count -ne 1) {
        throw "$($Entry.name) archive does not contain exactly one $($Entry.archiveMember)."
      }
      $candidate = $matches[0].FullName
      $tempTarget = "$Target.download"
      Copy-Item -LiteralPath $candidate -Destination $tempTarget -Force
      $candidate = $tempTarget
    } finally {
      if ($extractDir -and (Test-Path -LiteralPath $extractDir)) {
        Remove-Item -LiteralPath $extractDir -Recurse -Force
      }
    }
  } else {
    $tempTarget = "$Target.download"
    Copy-Item -LiteralPath $candidate -Destination $tempTarget -Force
    $candidate = $tempTarget
  }

  $actual = Get-Sha256 $candidate
  if ($actual -ne ([string]$Entry.sha256).ToUpperInvariant()) {
    Remove-Item -LiteralPath $candidate -Force -ErrorAction SilentlyContinue
    throw "$($Entry.name) SHA-256 verification failed."
  }
  Move-Item -LiteralPath $candidate -Destination $Target -Force
}

if (!(Test-Path -LiteralPath $LockPath -PathType Leaf)) {
  throw "Sidecar lock bulunamadı: $LockPath"
}

$Lock = Get-Content -LiteralPath $LockPath -Raw | ConvertFrom-Json
if ($Lock.schemaVersion -ne 1 -or $Lock.target -ne "x86_64-pc-windows-msvc") {
  throw "Desteklenmeyen sidecar lock şeması veya target."
}

New-Item -ItemType Directory -Path $BinaryDir -Force | Out-Null
$verified = 0
foreach ($entry in @($Lock.sidecars)) {
  Test-ValidEntry $entry
  $target = Join-Path $BinaryDir ([string]$entry.file)
  $locallyBuilt = $null -ne $entry.PSObject.Properties["buildScript"]
  $valid = Test-Path -LiteralPath $target -PathType Leaf
  if ($valid -and -not $locallyBuilt) {
    $valid = (Get-Sha256 $target) -eq ([string]$entry.sha256).ToUpperInvariant()
  }

  if (!$valid) {
    if ($VerifyOnly) {
      throw "$($entry.name) is missing or has an invalid checksum: $target"
    }
    if ($locallyBuilt) {
      $buildScript = [IO.Path]::GetFullPath((Join-Path $Root ([string]$entry.buildScript)))
      if (-not $buildScript.StartsWith(([IO.Path]::GetFullPath($Root) + [IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase) -or
          -not (Test-Path -LiteralPath $buildScript -PathType Leaf)) {
        throw "$($entry.name) build script is missing or outside the repository."
      }
      & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $buildScript
      if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $target -PathType Leaf)) {
        throw "$($entry.name) local build failed."
      }
      $valid = $true
      Write-Host "BUILT $($entry.name) $($entry.version) SHA256=$(Get-Sha256 $target)" -ForegroundColor Cyan
    } else {
    if ((Test-Path -LiteralPath $target) -and !$Force) {
      throw "$($entry.name) checksum differs. Use -Force to replace it."
    }
    Install-Sidecar $entry $target
    }
  }

  if (-not $locallyBuilt -and (Get-Sha256 $target) -ne ([string]$entry.sha256).ToUpperInvariant()) {
    throw "$($entry.name) failed final verification."
  }
  $verified++
  Write-Host "OK $($entry.name) $($entry.version)" -ForegroundColor Green
}

Write-Host "$verified sidecars verified." -ForegroundColor Green
