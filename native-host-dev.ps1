param(
  [string]$ExtensionId = "gifnifkakikpndieohkijmjccmmikalm",
  [switch]$Unregister
)

$ErrorActionPreference = "Stop"
$DevRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$RegistryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.mab.mediadrop.dev"

if ($Unregister) {
  if (Test-Path -LiteralPath $RegistryPath) {
    Remove-Item -LiteralPath $RegistryPath
  }
  Write-Host "MediaDrop development native host unregistered."
  exit 0
}

& (Join-Path $DevRoot "build-native-host.ps1") -ExtensionId $ExtensionId
if ($LASTEXITCODE -ne 0) { throw "Development native host build failed." }

$ManifestPath = Join-Path $DevRoot "src-tauri\target\debug\com.mab.mediadrop.dev.json"
New-Item -Path $RegistryPath -Force | Out-Null
Set-Item -LiteralPath $RegistryPath -Value ([System.IO.Path]::GetFullPath($ManifestPath))

Write-Host "MediaDrop development native host registered."
Write-Host "Load browser-extension\dist after running: npm run extension:build:dev"
