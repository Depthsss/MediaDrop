param(
  [string]$PublicDirectory = "",
  [string]$BackupDirectory = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Parent = Split-Path -Parent $Root
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
if ([string]::IsNullOrWhiteSpace($PublicDirectory)) {
  $PublicDirectory = Join-Path $Parent "MediaDrop-Public-1.0.0"
}
if ([string]::IsNullOrWhiteSpace($BackupDirectory)) {
  $BackupDirectory = Join-Path $Parent "MediaDrop-Private-Backup-$Stamp"
}
$PublicDirectory = [IO.Path]::GetFullPath($PublicDirectory)
$BackupDirectory = [IO.Path]::GetFullPath($BackupDirectory)

if (Test-Path -LiteralPath $PublicDirectory) { throw "Public output already exists: $PublicDirectory" }
if (Test-Path -LiteralPath $BackupDirectory) { throw "Backup output already exists: $BackupDirectory" }

New-Item -ItemType Directory -Path $BackupDirectory -Force | Out-Null
& git.exe bundle create (Join-Path $BackupDirectory "mediadrop-private-history.bundle") --all
if ($LASTEXITCODE -ne 0) { throw "Private Git history backup failed." }

$allSourceFiles = @(& git.exe ls-files --cached --others --exclude-standard)
if ($LASTEXITCODE -ne 0) { throw "Source file inventory failed." }
$allSourceFiles = @($allSourceFiles | Where-Object { $_ })
$publicSourceFiles = @($allSourceFiles | Where-Object {
  $_ -and
  $_ -ne "latest.json" -and
  $_ -notmatch '^src-tauri/binaries/.*\.exe$'
})

$workingBackup = Join-Path $BackupDirectory "working-tree"
New-Item -ItemType Directory -Path $workingBackup -Force | Out-Null
New-Item -ItemType Directory -Path $PublicDirectory -Force | Out-Null
foreach ($relative in $allSourceFiles) {
  $source = Join-Path $Root $relative
  if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { continue }
  $destination = Join-Path $workingBackup $relative
  New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
  Copy-Item -LiteralPath $source -Destination $destination
}
foreach ($relative in $publicSourceFiles) {
  $source = Join-Path $Root $relative
  if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { continue }
  $destination = Join-Path $PublicDirectory $relative
  New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
  Copy-Item -LiteralPath $source -Destination $destination
}

$largeFiles = @(Get-ChildItem -LiteralPath $PublicDirectory -Recurse -File | Where-Object { $_.Length -gt 50MB })
$blockedFiles = @($largeFiles | Where-Object { $_.Length -gt 100MB })
if ($blockedFiles.Count -gt 0) { throw "Public source contains files larger than 100 MiB: $($blockedFiles.FullName -join ', ')" }
if ($largeFiles.Count -gt 0) { Write-Warning "Public source contains files larger than 50 MiB: $($largeFiles.FullName -join ', ')" }

$gitleaks = Get-Command "gitleaks.exe" -ErrorAction SilentlyContinue
if (-not $gitleaks) { throw "Install Gitleaks before creating public history: winget install --id Gitleaks.Gitleaks -e" }
& $gitleaks.Source dir --no-banner --redact=100 $PublicDirectory
if ($LASTEXITCODE -ne 0) { throw "Secret scan failed; public Git history was not created." }

Push-Location $PublicDirectory
try {
  & git.exe init
  if ($LASTEXITCODE -ne 0) { throw "Public repository initialization failed." }
  & git.exe checkout -b main
  if ($LASTEXITCODE -ne 0) { throw "Public main branch creation failed." }
  & git.exe add --all
  if ($LASTEXITCODE -ne 0) { throw "Public source staging failed." }
  & git.exe -c "user.name=Depthsss" -c "user.email=41898282+github-actions[bot]@users.noreply.github.com" commit -m "MediaDrop 1.0.0"
  if ($LASTEXITCODE -ne 0) { throw "Public initial commit failed." }
  if ((& git.exe rev-list --count HEAD) -ne "1") { throw "Public repository must contain exactly one commit." }
} finally {
  Pop-Location
}

Write-Host "Private backup: $BackupDirectory" -ForegroundColor Green
Write-Host "Clean one-commit public source: $PublicDirectory" -ForegroundColor Green
Write-Host "No remote was added and nothing was pushed." -ForegroundColor Yellow
