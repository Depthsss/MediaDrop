[CmdletBinding()]
param(
  [switch]$GenerateLatestOnly,
  [switch]$PreflightOnly,
  [switch]$SkipPublish
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$SourceRepo = "Depthsss/MediaDrop"
$ReleaseRepo = "Depthsss/MediaDrop-Releases"
$ProductionExtensionId = "gifnifkakikpndieohkijmjccmmikalm"
$SigningKeyPath = Join-Path $env:USERPROFILE ".tauri\mediadrop.key"
$NotesPath = Join-Path $Root "release-notes.md"
$ArtifactRoot = Join-Path $Root "artifacts"
$PreviousSigningKey = $env:TAURI_SIGNING_PRIVATE_KEY
$PreviousSigningPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD

function Assert-Release([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function Invoke-Checked([string]$Command, [string[]]$Arguments, [int[]]$AllowedExitCodes = @(0)) {
  Write-Host "`n> $Command $($Arguments -join ' ')" -ForegroundColor DarkGray
  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    & $Command @Arguments | ForEach-Object { Write-Host $_ }
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  if ($AllowedExitCodes -notcontains $exitCode) {
    throw "$Command failed with exit code $exitCode."
  }
}

function Invoke-MsiAdministrativeExtract([string]$MsiPath, [string]$TargetDirectory) {
  $arguments = "/a `"$MsiPath`" /qn TARGETDIR=`"$TargetDirectory`""
  $process = Start-Process -FilePath "msiexec.exe" -ArgumentList $arguments -Wait -PassThru -WindowStyle Hidden
  if (@(0, 3010) -notcontains $process.ExitCode) {
    throw "MSI administrative extraction failed with exit code $($process.ExitCode)."
  }
}

function Get-CheckedOutput([string]$Command, [string[]]$Arguments) {
  $previousPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    $output = & $Command @Arguments 2>&1
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousPreference
  }
  if ($exitCode -ne 0) { throw "$Command failed with exit code $exitCode.`n$($output -join "`n")" }
  ($output -join "`n").Trim()
}

function Write-Utf8Json([string]$Path, $Value) {
  $json = $Value | ConvertTo-Json -Depth 20
  [IO.File]::WriteAllText($Path, "$json`n", (New-Object Text.UTF8Encoding($false)))
}

function Get-ChromeExtensionId([string]$PublicKey) {
  $bytes = [Convert]::FromBase64String($PublicKey)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $digest = $sha.ComputeHash($bytes) } finally { $sha.Dispose() }
  $alphabet = "abcdefghijklmnop"
  $builder = New-Object Text.StringBuilder
  foreach ($byte in $digest[0..15]) {
    $null = $builder.Append($alphabet[($byte -shr 4) -band 15])
    $null = $builder.Append($alphabet[$byte -band 15])
  }
  $builder.ToString()
}

function Get-ReleaseMetadata {
  $package = Get-Content -LiteralPath (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
  $packageLockText = Get-Content -LiteralPath (Join-Path $Root "package-lock.json") -Raw
  $tauri = Get-Content -LiteralPath (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
  $extension = Get-Content -LiteralPath (Join-Path $Root "browser-extension\manifest.json") -Raw | ConvertFrom-Json
  $cargoText = Get-Content -LiteralPath (Join-Path $Root "src-tauri\Cargo.toml") -Raw
  $cargoLockText = Get-Content -LiteralPath (Join-Path $Root "src-tauri\Cargo.lock") -Raw
  $cargoMatch = [regex]::Match($cargoText, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
  $cargoLockMatch = [regex]::Match($cargoLockText, '(?ms)^\[\[package\]\]\s*\r?\nname\s*=\s*"mediadrop"\s*\r?\nversion\s*=\s*"([^"]+)"')
  $packageLockMatch = [regex]::Match($packageLockText, '(?s)^\s*\{.*?"version"\s*:\s*"([^"]+)"')
  $packageLockRootMatch = [regex]::Match($packageLockText, '(?s)"packages"\s*:\s*\{\s*""\s*:\s*\{.*?"version"\s*:\s*"([^"]+)"')
  Assert-Release ($cargoMatch.Success -and $cargoLockMatch.Success -and $packageLockMatch.Success -and $packageLockRootMatch.Success) "Cargo or npm lock version could not be read."

  $version = [string]$package.version
  $versions = @(
    $version,
    [string]$packageLockMatch.Groups[1].Value,
    [string]$packageLockRootMatch.Groups[1].Value,
    [string]$tauri.version,
    [string]$extension.version,
    [string]$cargoMatch.Groups[1].Value,
    [string]$cargoLockMatch.Groups[1].Value
  )
  Assert-Release ($version -match '^\d+\.\d+\.\d+$') "Canonical package version is invalid."
  Assert-Release (@($versions | Where-Object { $_ -ne $version }).Count -eq 0) "Release versions do not all match package.json ($version)."

  $derivedExtensionId = Get-ChromeExtensionId ([string]$extension.key)
  Assert-Release ($derivedExtensionId -eq $ProductionExtensionId) "Extension public key does not derive the stable production ID."

  [pscustomobject]@{
    Version = $version
    Tag = "v$version"
    Notes = (Get-Content -LiteralPath $NotesPath -Raw).Trim()
    ExtensionId = $derivedExtensionId
  }
}

function Assert-ReleaseNotes([string]$Notes) {
  Assert-Release (-not [string]::IsNullOrWhiteSpace($Notes)) "release-notes.md is empty."
  Assert-Release ($Notes -notmatch '(?i)TODO|placeholder|bug fixes and improvements') "release-notes.md still contains placeholder text."
}

function Ensure-Command([string]$Name, [string]$InstallHint) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { throw "$Name is required. $InstallHint" }
}

function Install-WingetPackage([string]$Id) {
  Ensure-Command "winget.exe" "Install App Installer from Microsoft Store."
  Invoke-Checked "winget.exe" @(
    "install", "--id", $Id, "--exact", "--silent",
    "--accept-package-agreements", "--accept-source-agreements"
  ) @(0, -1978335189)
}

function Resolve-Gitleaks {
  $command = Get-Command "gitleaks.exe" -ErrorAction SilentlyContinue
  if ($command) { return $command.Source }
  $wingetLink = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links\gitleaks.exe"
  if (Test-Path -LiteralPath $wingetLink -PathType Leaf) { return $wingetLink }
  Install-WingetPackage "Gitleaks.Gitleaks"
  Assert-Release (Test-Path -LiteralPath $wingetLink -PathType Leaf) "Gitleaks installation completed but its executable was not found."
  $wingetLink
}

function Ensure-CargoAudit {
  if (-not (Get-Command "cargo-audit.exe" -ErrorAction SilentlyContinue)) {
    Invoke-Checked "cargo.exe" @("install", "cargo-audit", "--locked")
  }
}

function Ensure-Nsis {
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"),
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\Bin\makensis.exe"),
    (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
    (Join-Path $env:ProgramFiles "NSIS\makensis.exe")
  )
  if (@($candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) }).Count -eq 0 -and
      -not (Get-Command "makensis.exe" -ErrorAction SilentlyContinue)) {
    Install-WingetPackage "NSIS.NSIS"
  }
}

function Assert-TrackedSourceIsPublishable {
  $trackedText = Get-CheckedOutput "git.exe" @("ls-files", "src-tauri/binaries/*.exe")
  $trackedSidecars = @($trackedText -split "`r?`n" | Where-Object { $_ })
  Assert-Release ($trackedSidecars.Count -eq 0) "Generated sidecar executables are still tracked by Git."
  $trackedLatest = Get-CheckedOutput "git.exe" @("ls-files", "latest.json")
  Assert-Release ([string]::IsNullOrWhiteSpace($trackedLatest)) "Generated latest.json is still tracked by Git."

  $oversized = @()
  $large = @()
  foreach ($line in @(Get-CheckedOutput "git.exe" @("rev-list", "--objects", "HEAD") -split "`r?`n")) {
    if (-not $line) { continue }
    $objectId = ($line -split ' ', 2)[0]
    $size = [int64](Get-CheckedOutput "git.exe" @("cat-file", "-s", $objectId))
    if ($size -gt 100MB) { $oversized += $line }
    elseif ($size -gt 50MB) { $large += $line }
  }
  Assert-Release ($oversized.Count -eq 0) "Git history contains objects larger than GitHub's 100 MiB limit: $($oversized -join ', ')"
  if ($large.Count -gt 0) { Write-Warning "Git history contains objects larger than 50 MiB: $($large -join ', ')" }
}

function Assert-SourceGitState($Metadata) {
  Assert-Release ((Get-CheckedOutput "git.exe" @("branch", "--show-current")) -eq "main") "Release must run from main."
  Assert-Release ([string]::IsNullOrWhiteSpace((Get-CheckedOutput "git.exe" @("status", "--porcelain", "--untracked-files=all")))) "Working tree must be clean."
  $origin = Get-CheckedOutput "git.exe" @("remote", "get-url", "origin")
  $allowedOrigins = @(
    "https://github.com/$SourceRepo.git",
    "https://github.com/$SourceRepo",
    "git@github.com:$SourceRepo.git",
    "ssh://git@github.com/$SourceRepo.git"
  )
  Assert-Release ($allowedOrigins -contains $origin) "origin must point exactly to $SourceRepo."
  Invoke-Checked "git.exe" @("fetch", "origin", "main", "--tags")
  $head = Get-CheckedOutput "git.exe" @("rev-parse", "HEAD")
  Assert-Release ($head -eq (Get-CheckedOutput "git.exe" @("rev-parse", "origin/main"))) "Local main must exactly match origin/main."
  if ($Metadata.Version -eq "1.0.0") {
    Assert-Release ((Get-CheckedOutput "git.exe" @("rev-list", "--count", "HEAD")) -eq "1") "MediaDrop 1.0.0 public main must have one clean initial commit."
  }
  Assert-TrackedSourceIsPublishable

  $sourceTag = & git.exe ls-remote --tags origin "refs/tags/$($Metadata.Tag)" 2>$null
  Assert-Release ($LASTEXITCODE -eq 0 -and [string]::IsNullOrWhiteSpace(($sourceTag -join ""))) "Source tag $($Metadata.Tag) already exists."
}

function Ensure-GitHubState($Metadata) {
  if (-not (Get-Command "gh.exe" -ErrorAction SilentlyContinue)) { Install-WingetPackage "GitHub.cli" }
  & gh.exe auth status *> $null
  if ($LASTEXITCODE -ne 0) {
    Write-Host "GitHub login is required; opening the browser login flow." -ForegroundColor Yellow
    Invoke-Checked "gh.exe" @("auth", "login", "--web")
  }

  & gh.exe release view $Metadata.Tag -R $ReleaseRepo *> $null
  Assert-Release ($LASTEXITCODE -ne 0) "$($Metadata.Tag) already exists in $ReleaseRepo."

  $head = Get-CheckedOutput "git.exe" @("rev-parse", "HEAD")
  $runsJson = Get-CheckedOutput "gh.exe" @(
    "run", "list", "-R", $SourceRepo, "--workflow", "ci.yml", "--commit", $head,
    "--status", "completed", "--limit", "20", "--json", "conclusion,headSha"
  )
  $runs = @($runsJson | ConvertFrom-Json)
  Assert-Release (@($runs | Where-Object { $_.headSha -eq $head -and $_.conclusion -eq "success" }).Count -ge 1) "Source CI has not passed for HEAD $head."
}

function Invoke-SecurityChecks([string]$GitleaksPath) {
  Invoke-Checked "npm.cmd" @("audit", "--audit-level=high")
  Invoke-Checked "cargo.exe" @("audit", "--file", (Join-Path $Root "src-tauri\Cargo.lock"))
  Invoke-Checked $GitleaksPath @("dir", "--no-banner", "--redact=100", $Root)
  Invoke-Checked $GitleaksPath @("git", "--no-banner", "--redact=100", "--log-opts=HEAD", $Root)
}

function Invoke-Preflight([bool]$IncludeGitHub) {
  Ensure-Command "git.exe" "Install Git for Windows."
  Ensure-Command "node.exe" "Install the current Node.js LTS."
  Ensure-Command "npm.cmd" "Install the current Node.js LTS."
  Ensure-Command "cargo.exe" "Install the stable Rust MSVC toolchain."
  Ensure-Command "rustc.exe" "Install the stable Rust MSVC toolchain."
  Ensure-Command "py.exe" "Install 64-bit Python 3.10."
  Ensure-Command "powershell.exe" "Windows PowerShell 5.1 is required."
  Assert-Release (Test-Path -LiteralPath $SigningKeyPath -PathType Leaf) "Updater signing key is missing: $SigningKeyPath"

  $metadata = Get-ReleaseMetadata
  Assert-ReleaseNotes $metadata.Notes
  $sidecarLock = Get-Content -LiteralPath (Join-Path $Root "src-tauri\binaries\sidecars.lock.json") -Raw | ConvertFrom-Json
  Assert-Release ($sidecarLock.schemaVersion -eq 1 -and @($sidecarLock.sidecars).Count -eq 7) "Sidecar lock is incomplete."
  foreach ($entry in @($sidecarLock.sidecars)) {
    Assert-Release (-not [string]::IsNullOrWhiteSpace([string]$entry.license)) "Sidecar license metadata is incomplete."
    Assert-Release ([string]$entry.sourceUrl -match '^https://') "Sidecar source metadata is incomplete."
  }

  if ($IncludeGitHub) {
    Assert-SourceGitState $metadata
    Ensure-GitHubState $metadata
  }

  Ensure-CargoAudit
  $gitleaks = Resolve-Gitleaks
  Invoke-SecurityChecks $gitleaks
  Write-Host "`nPreflight OK: MediaDrop $($metadata.Version)" -ForegroundColor Green
  $metadata
}

function Find-BuiltMsi([string]$Version) {
  $msiRoot = Join-Path $Root "src-tauri\target\release\bundle\msi"
  $matches = @(Get-ChildItem -LiteralPath $msiRoot -Filter "*.msi" -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match [regex]::Escape($Version) } |
    Sort-Object LastWriteTimeUtc -Descending)
  Assert-Release ($matches.Count -ge 1) "Tauri MSI output was not found for $Version."
  $msi = $matches[0]
  $signature = "$($msi.FullName).sig"
  Assert-Release (Test-Path -LiteralPath $signature -PathType Leaf) "Tauri updater signature is missing: $signature"
  [pscustomobject]@{ Msi = $msi.FullName; Signature = $signature }
}

function Write-Latest([string]$Version, [string]$Notes, [string]$MsiName, [string]$SignaturePath, [string]$OutputPath) {
  $signature = [IO.File]::ReadAllText($SignaturePath).Trim()
  Assert-Release (-not [string]::IsNullOrWhiteSpace($signature)) "Updater signature is empty."
  Write-Utf8Json $OutputPath ([ordered]@{
    version = $Version
    notes = $Notes
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ", [Globalization.CultureInfo]::InvariantCulture)
    platforms = [ordered]@{
      "windows-x86_64" = [ordered]@{
        signature = $signature
        url = "https://github.com/$ReleaseRepo/releases/download/v$Version/$MsiName"
      }
    }
  })
}

function Set-SigningEnvironment {
  Write-Host "`nEnter the Tauri updater signing-key password." -ForegroundColor Yellow
  $secure = Read-Host "Password" -AsSecureString
  $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
  try { $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
  finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer) }
  $env:TAURI_SIGNING_PRIVATE_KEY = $SigningKeyPath
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $plain
  $plain = $null
}

function Write-BuildInfo($Metadata, [string]$OutputPath) {
  $sidecars = Get-Content -LiteralPath (Join-Path $Root "src-tauri\binaries\sidecars.lock.json") -Raw | ConvertFrom-Json
  Write-Utf8Json $OutputPath ([ordered]@{
    version = $Metadata.Version
    sourceCommit = (Get-CheckedOutput "git.exe" @("rev-parse", "HEAD"))
    builtAt = (Get-Date).ToUniversalTime().ToString("o")
    target = "x86_64-pc-windows-msvc"
    minimumWindows = "Windows 10 22H2 (build 19045)"
    node = (Get-CheckedOutput "node.exe" @("--version"))
    rustc = (Get-CheckedOutput "rustc.exe" @("--version"))
    cargo = (Get-CheckedOutput "cargo.exe" @("--version"))
    sidecars = @($sidecars.sidecars | ForEach-Object { [ordered]@{ name = $_.name; version = $_.version; file = $_.file } })
  })
}

function Write-Checksums([string]$Directory, [string]$OutputPath) {
  $files = @(Get-ChildItem -LiteralPath $Directory -File | Where-Object { $_.FullName -ne $OutputPath } | Sort-Object Name)
  $lines = foreach ($file in $files) { "$(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256 | Select-Object -ExpandProperty Hash) *$($file.Name)" }
  [IO.File]::WriteAllLines($OutputPath, $lines, (New-Object Text.UTF8Encoding($false)))
}

function Test-Checksums([string]$Directory) {
  $checksumPath = Join-Path $Directory "SHA256SUMS.txt"
  foreach ($line in Get-Content -LiteralPath $checksumPath) {
    if ($line -notmatch '^([A-F0-9]{64}) \*(.+)$') { throw "Invalid SHA256SUMS.txt line." }
    $target = Join-Path $Directory $Matches[2]
    Assert-Release (Test-Path -LiteralPath $target -PathType Leaf) "Downloaded release asset is missing: $($Matches[2])"
    Assert-Release ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq $Matches[1]) "Downloaded release asset checksum failed: $($Matches[2])"
  }
}

function Invoke-ReleaseBuild($Metadata) {
  if (Test-Path -LiteralPath $ArtifactRoot) { Remove-Item -LiteralPath $ArtifactRoot -Recurse -Force }
  New-Item -ItemType Directory -Path $ArtifactRoot -Force | Out-Null

  Invoke-Checked "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "tools\instagram-helper\build.ps1"))
  Invoke-Checked "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "prepare-sidecars.ps1"), "-VerifyOnly")
  Invoke-Checked "npm.cmd" @("ci")
  Invoke-Checked "npm.cmd" @("run", "verify:frontend")
  Invoke-Checked "cargo.exe" @("fmt", "--manifest-path", (Join-Path $Root "src-tauri\Cargo.toml"), "--package", "mediadrop", "--", "--check")
  Invoke-Checked "cargo.exe" @("test", "--locked", "--manifest-path", (Join-Path $Root "src-tauri\Cargo.toml"))
  Invoke-Checked "npm.cmd" @("run", "extension:build")

  $extensionZip = Join-Path $ArtifactRoot "MediaDrop-Extension-$($Metadata.Version).zip"
  Compress-Archive -Path (Join-Path $Root "browser-extension\dist\*") -DestinationPath $extensionZip -CompressionLevel Optimal

  Invoke-Checked "powershell.exe" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "build-native-host.ps1"),
    "-Production", "-ExtensionId", $Metadata.ExtensionId
  )
  $nativeHost = Join-Path $Root "src-tauri\target\release\mediadrop-native-host.exe"
  Invoke-Checked $nativeHost @("--self-test")
  Invoke-Checked "powershell.exe" @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "verify-release-compatibility.ps1"))

  Set-SigningEnvironment
  Invoke-Checked "npm.cmd" @("run", "tauri", "--", "build", "--bundles", "msi")
  $bundle = Find-BuiltMsi $Metadata.Version
  $publicMsiName = "MediaDrop_$($Metadata.Version)_x64.msi"
  $publicMsi = Join-Path $ArtifactRoot $publicMsiName
  $publicSignature = "$publicMsi.sig"
  Copy-Item -LiteralPath $bundle.Msi -Destination $publicMsi
  Copy-Item -LiteralPath $bundle.Signature -Destination $publicSignature

  Ensure-Nsis
  Invoke-Checked "powershell.exe" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "build-setup.ps1"),
    "-MsiPath", $publicMsi, "-Version", $Metadata.Version, "-OutputDirectory", $ArtifactRoot
  )

  $extractRoot = Join-Path $ArtifactRoot "msi-extract"
  New-Item -ItemType Directory -Path $extractRoot -Force | Out-Null
  Invoke-MsiAdministrativeExtract $publicMsi $extractRoot

  Copy-Item -LiteralPath (Join-Path $Root "THIRD_PARTY_NOTICES.md") -Destination (Join-Path $ArtifactRoot "THIRD_PARTY_NOTICES.md")
  Write-BuildInfo $Metadata (Join-Path $ArtifactRoot "build-info.json")
  Write-Latest $Metadata.Version $Metadata.Notes $publicMsiName $publicSignature (Join-Path $ArtifactRoot "latest.json")
  Write-Checksums $ArtifactRoot (Join-Path $ArtifactRoot "SHA256SUMS.txt")
  Invoke-Checked "powershell.exe" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $Root "verify-release-compatibility.ps1"),
    "-ArtifactDirectory", $ArtifactRoot, "-ExtractedMsiPath", $extractRoot
  )
  Remove-Item -LiteralPath $extractRoot -Recurse -Force
  Write-Checksums $ArtifactRoot (Join-Path $ArtifactRoot "SHA256SUMS.txt")
  Test-Checksums $ArtifactRoot

  Write-Host "`nRelease artifacts are ready: $ArtifactRoot" -ForegroundColor Green
  @(Get-ChildItem -LiteralPath $ArtifactRoot -File | Sort-Object Name | Select-Object -ExpandProperty FullName)
}

function Invoke-GenerateLatest($Metadata) {
  New-Item -ItemType Directory -Path $ArtifactRoot -Force | Out-Null
  $publicMsiName = "MediaDrop_$($Metadata.Version)_x64.msi"
  $publicMsi = Join-Path $ArtifactRoot $publicMsiName
  $publicSignature = "$publicMsi.sig"
  if (-not (Test-Path -LiteralPath $publicMsi -PathType Leaf) -or -not (Test-Path -LiteralPath $publicSignature -PathType Leaf)) {
    $bundle = Find-BuiltMsi $Metadata.Version
    Copy-Item -LiteralPath $bundle.Msi -Destination $publicMsi -Force
    Copy-Item -LiteralPath $bundle.Signature -Destination $publicSignature -Force
  }
  $latest = Join-Path $ArtifactRoot "latest.json"
  Write-Latest $Metadata.Version $Metadata.Notes $publicMsiName $publicSignature $latest
  Write-Host "latest.json generated: $latest" -ForegroundColor Green
}

function Publish-Release($Metadata, [string[]]$Assets) {
  Write-Host "`nMediaDrop $($Metadata.Version) will be published as an unsigned Windows release." -ForegroundColor Yellow
  Write-Host "Windows SmartScreen may show a warning." -ForegroundColor Yellow
  $answer = Read-Host "Type Y to create the draft and publish after verification"
  if ($answer -notmatch '(?i)^y(es)?$') {
    Write-Host "Publishing cancelled; verified local artifacts were kept." -ForegroundColor Yellow
    return
  }

  $arguments = @(
    "release", "create", $Metadata.Tag, "-R", $ReleaseRepo,
    "--title", "MediaDrop $($Metadata.Version)", "--notes-file", $NotesPath,
    "--target", "main", "--draft"
  ) + $Assets
  Invoke-Checked "gh.exe" $arguments

  $downloadRoot = Join-Path ([IO.Path]::GetTempPath()) ("mediadrop-release-verify-" + [Guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $downloadRoot -Force | Out-Null
  try {
    Invoke-Checked "gh.exe" @("release", "download", $Metadata.Tag, "-R", $ReleaseRepo, "--dir", $downloadRoot)
    Test-Checksums $downloadRoot

    Invoke-Checked "git.exe" @("tag", "-a", $Metadata.Tag, "-m", "MediaDrop $($Metadata.Version)")
    Invoke-Checked "git.exe" @("push", "origin", $Metadata.Tag)
    Invoke-Checked "gh.exe" @("release", "edit", $Metadata.Tag, "-R", $ReleaseRepo, "--draft=false", "--latest")

    $publicLatest = Join-Path $downloadRoot "public-latest.json"
    Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$ReleaseRepo/releases/latest/download/latest.json" -OutFile $publicLatest
    $latest = Get-Content -LiteralPath $publicLatest -Raw | ConvertFrom-Json
    Assert-Release ($latest.version -eq $Metadata.Version) "Public updater endpoint returned the wrong version."
    Assert-Release ([string]$latest.platforms."windows-x86_64".url -match [regex]::Escape($Metadata.Tag)) "Public updater endpoint returned the wrong MSI URL."
  } finally {
    if (Test-Path -LiteralPath $downloadRoot) { Remove-Item -LiteralPath $downloadRoot -Recurse -Force }
  }

  $releaseUrl = "https://github.com/$ReleaseRepo/releases/tag/$($Metadata.Tag)"
  Start-Process $releaseUrl
  Write-Host "`nPublished: $releaseUrl" -ForegroundColor Green
}

try {
  Set-Location $Root
  if ($GenerateLatestOnly) {
    $metadata = Get-ReleaseMetadata
    Assert-ReleaseNotes $metadata.Notes
    Invoke-GenerateLatest $metadata
    exit 0
  }

  $metadata = Invoke-Preflight (-not $SkipPublish)
  if ($PreflightOnly) { exit 0 }
  $assets = Invoke-ReleaseBuild $metadata
  if ($SkipPublish) {
    Write-Host "Publishing skipped by request." -ForegroundColor Yellow
  } else {
    Publish-Release $metadata $assets
  }
} catch {
  Write-Host "`nHATA: $($_.Exception.Message)" -ForegroundColor Red
  if ($_.ScriptStackTrace) { Write-Host $_.ScriptStackTrace -ForegroundColor DarkGray }
  exit 1
} finally {
  if ($null -eq $PreviousSigningKey) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue }
  else { $env:TAURI_SIGNING_PRIVATE_KEY = $PreviousSigningKey }
  if ($null -eq $PreviousSigningPassword) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue }
  else { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $PreviousSigningPassword }
}
