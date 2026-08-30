[CmdletBinding()]
param(
  [string]$ExecutableName = "MediaDrop-Setup-1.0.0.exe",
  [string]$DumpDirectory = (Join-Path $env:LOCALAPPDATA "MediaDrop\InstallerDiagnostics\LegacyDumps")
)

$ErrorActionPreference = "Stop"
if ($ExecutableName -notmatch '^[A-Za-z0-9._-]+\.exe$') {
  throw "ExecutableName must be a plain executable filename."
}

$resolvedDumpDirectory = [IO.Path]::GetFullPath($DumpDirectory)
New-Item -ItemType Directory -Path $resolvedDumpDirectory -Force | Out-Null
$key = "HKCU:\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\$ExecutableName"
New-Item -Path $key -Force | Out-Null
New-ItemProperty -Path $key -Name DumpFolder -PropertyType ExpandString -Value $resolvedDumpDirectory -Force | Out-Null
New-ItemProperty -Path $key -Name DumpType -PropertyType DWord -Value 2 -Force | Out-Null
New-ItemProperty -Path $key -Name DumpCount -PropertyType DWord -Value 10 -Force | Out-Null

Write-Host "Full user-mode dumps enabled for $ExecutableName" -ForegroundColor Green
Write-Host "Dump directory: $resolvedDumpDirectory"
Write-Warning "Dumps can contain private process memory. Do not commit or upload them without review."
