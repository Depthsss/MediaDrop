[CmdletBinding()]
param(
  [string]$ExecutableName = "MediaDrop-Setup-1.0.0.exe"
)

$ErrorActionPreference = "Stop"
if ($ExecutableName -notmatch '^[A-Za-z0-9._-]+\.exe$') {
  throw "ExecutableName must be a plain executable filename."
}

$key = "HKCU:\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\$ExecutableName"
if (Test-Path -LiteralPath $key) {
  Remove-Item -LiteralPath $key -Recurse -Force
}
Write-Host "LocalDumps override removed for $ExecutableName" -ForegroundColor Green
