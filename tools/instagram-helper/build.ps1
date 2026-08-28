param(
  [string]$PythonLauncher = "py"
)

$ErrorActionPreference = "Stop"
$HelperRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent (Split-Path -Parent $HelperRoot)
$VenvRoot = Join-Path $HelperRoot ".venv"
$Python = Join-Path $VenvRoot "Scripts\python.exe"
$LockPath = Join-Path $HelperRoot "requirements.lock.txt"
$SpecPath = Join-Path $HelperRoot "instaloader-helper.spec"
$BuildRoot = Join-Path $HelperRoot "build"
$OutputPath = Join-Path $RepoRoot "src-tauri\binaries\instaloader-helper-x86_64-pc-windows-msvc.exe"

if (-not (Test-Path -LiteralPath $LockPath -PathType Leaf)) {
  throw "Instagram helper dependency lock is missing: $LockPath"
}
if (-not (Test-Path -LiteralPath $Python -PathType Leaf)) {
  & $PythonLauncher -3.10 -m venv $VenvRoot
  if ($LASTEXITCODE -ne 0) { throw "Python 3.10 virtual environment could not be created." }
}

& $Python -m pip install --disable-pip-version-check --require-hashes -r $LockPath
if ($LASTEXITCODE -ne 0) { throw "Instagram helper dependencies could not be installed." }

New-Item -ItemType Directory -Path $BuildRoot -Force | Out-Null
& $Python -m PyInstaller --clean --noconfirm --distpath (Join-Path $BuildRoot "dist") --workpath (Join-Path $BuildRoot "work") $SpecPath
if ($LASTEXITCODE -ne 0) { throw "Instagram helper build failed." }

$BuiltPath = Join-Path $BuildRoot "dist\instaloader-helper.exe"
if (-not (Test-Path -LiteralPath $BuiltPath -PathType Leaf)) {
  throw "Instagram helper output was not created: $BuiltPath"
}
Copy-Item -LiteralPath $BuiltPath -Destination $OutputPath -Force

$stream = [IO.File]::OpenRead($OutputPath)
$sha = [Security.Cryptography.SHA256]::Create()
try { $Hash = ([BitConverter]::ToString($sha.ComputeHash($stream))).Replace("-", "") }
finally {
  $sha.Dispose()
  $stream.Dispose()
}

Write-Output "$OutputPath $Hash"
