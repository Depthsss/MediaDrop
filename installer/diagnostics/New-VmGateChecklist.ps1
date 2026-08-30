[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SetupPath,
  [Parameter(Mandatory = $true)][string]$MsiPath,
  [string]$OutputPath = (Join-Path (Get-Location) "installer-vm-gate-checklist.json"),
  [switch]$IUnderstandTheseTestsModifyDisposableWindowsVms
)

$ErrorActionPreference = "Stop"
if (-not $IUnderstandTheseTestsModifyDisposableWindowsVms) {
  throw "Use -IUnderstandTheseTestsModifyDisposableWindowsVms only on disposable clean Windows VMs."
}

function Get-Sha256([string]$Path) {
  $stream = [IO.File]::OpenRead((Resolve-Path -LiteralPath $Path).Path)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace("-", "") }
  finally { $sha.Dispose(); $stream.Dispose() }
}

$scenarios = @(
  "clean_install", "repair", "upgrade", "uninstall", "reinstall",
  "app_open_upgrade", "native_host_open_upgrade", "files_in_use", "installer_busy_1618",
  "cancel_early", "cancel_middle", "cancel_late", "parent_ui_kill", "reboot_3010_1641",
  "tauri_updater_after_wrapper", "normal_integrity_launch", "correct_browser_profile"
)
$rows = foreach ($os in @("windows_10_22h2_tr", "windows_10_22h2_en", "windows_11_tr", "windows_11_en")) {
  foreach ($account in @("administrator", "standard_user_other_admin_credential")) {
    [ordered]@{ os = $os; account = $account; scenarios = $scenarios; passed = $false; evidence = "" }
  }
}
$document = [ordered]@{
  schemaVersion = 1
  generatedAt = (Get-Date).ToUniversalTime().ToString("o")
  sourceCommit = (& git.exe rev-parse HEAD).Trim()
  warning = "Template only. Publishing remains blocked until every row has reviewed evidence."
  setup = [ordered]@{ path = (Resolve-Path -LiteralPath $SetupPath).Path; sha256 = Get-Sha256 $SetupPath }
  msi = [ordered]@{ path = (Resolve-Path -LiteralPath $MsiPath).Path; sha256 = Get-Sha256 $MsiPath }
  defenderEnabled = $true
  unicodeProfileCovered = $false
  rows = $rows
  independentlyReviewed = $false
}
[IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), (($document | ConvertTo-Json -Depth 10) + "`n"), (New-Object Text.UTF8Encoding($false)))
Write-Host "VM gate checklist created: $([IO.Path]::GetFullPath($OutputPath))" -ForegroundColor Green
Write-Warning "This file is not a pass receipt until every row and review field is completed truthfully."
