$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root "release-mediadrop.ps1") -GenerateLatestOnly
exit $LASTEXITCODE
