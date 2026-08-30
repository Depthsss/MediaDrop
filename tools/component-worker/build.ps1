param([switch]$VerifyOnly)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
$TargetRoot = Join-Path $Root "installer\worker\target\component"
$Output = Join-Path $Root "src-tauri\binaries\mediadrop-component-worker-x86_64-pc-windows-msvc.exe"

if ($VerifyOnly) {
  if (-not (Test-Path -LiteralPath $Output -PathType Leaf)) {
    throw "Component worker is missing: $Output"
  }
  Write-Host "OK component worker" -ForegroundColor Green
  exit 0
}

$previousRustFlags = $env:RUSTFLAGS
try {
  $env:RUSTFLAGS = (($env:RUSTFLAGS + " -C target-feature=+crt-static").Trim())
  & cargo.exe build --manifest-path (Join-Path $Root "installer\worker\Cargo.toml") `
    --target-dir $TargetRoot --target x86_64-pc-windows-msvc --release --locked `
    --no-default-features --features component-mode
  if ($LASTEXITCODE -ne 0) {
    throw "Component worker build failed with exit code $LASTEXITCODE."
  }
} finally {
  $env:RUSTFLAGS = $previousRustFlags
}

$Built = Join-Path $TargetRoot "x86_64-pc-windows-msvc\release\mediadrop-installer-worker.exe"
if (-not (Test-Path -LiteralPath $Built -PathType Leaf)) {
  throw "Component worker output was not created: $Built"
}
New-Item -ItemType Directory -Path (Split-Path -Parent $Output) -Force | Out-Null
Copy-Item -LiteralPath $Built -Destination $Output -Force
Write-Host "BUILT component worker" -ForegroundColor Green
