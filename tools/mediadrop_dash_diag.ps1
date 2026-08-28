param(
  [string]$Url = "",
  [string]$Quality = "2160p",
  [double]$Start = 177,
  [double]$End = 222
)

$ErrorActionPreference = "Continue"

if ([string]::IsNullOrWhiteSpace($Url)) {
  $Url = Read-Host "4K/2K YouTube linkini gir"
}

$height = 2160
if ($Quality -match "1440|2K") { $height = 1440 }
elseif ($Quality -match "1080") { $height = 1080 }
elseif ($Quality -match "720") { $height = 720 }

$bin = Join-Path $env:LOCALAPPDATA "MediaDrop\bin"
$yt = Join-Path $bin "yt-dlp.exe"
$ffmpeg = Join-Path $bin "ffmpeg.exe"
$ffprobe = Join-Path $bin "ffprobe.exe"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:USERPROFILE "Desktop\MediaDropDashDiag-$stamp"
$zip = "$out.zip"
New-Item -ItemType Directory -Force -Path $out | Out-Null

function Write-Text($name, $text) {
  $path = Join-Path $out $name
  $text | Out-File -FilePath $path -Encoding UTF8
}

function Q([string]$s) {
  if ($null -eq $s) { return '""' }
  return '"' + ($s -replace '"','\"') + '"'
}

function Run-Proc($name, $exe, [string[]]$procArgs, [int]$timeoutSec) {
  $log = Join-Path $out ("$name.log")
  $cmdLine = (Q $exe) + " " + (($procArgs | ForEach-Object { Q $_ }) -join " ")
  @(
    "=== $name ===",
    "Started: $(Get-Date -Format o)",
    "Timeout: $timeoutSec sec",
    "Command:",
    $cmdLine,
    "",
    "--- OUTPUT ---"
  ) | Out-File -FilePath $log -Encoding UTF8

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = (($procArgs | ForEach-Object { Q $_ }) -join " ")
  $psi.WorkingDirectory = $out
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true

  $p = New-Object System.Diagnostics.Process
  $p.StartInfo = $psi
  try {
    [void]$p.Start()
    $stdoutTask = $p.StandardOutput.ReadToEndAsync()
    $stderrTask = $p.StandardError.ReadToEndAsync()
    $finished = $p.WaitForExit($timeoutSec * 1000)
    if (-not $finished) {
      "`n--- TIMEOUT: killing PID $($p.Id) ---" | Out-File -FilePath $log -Encoding UTF8 -Append
      try { & taskkill.exe /PID $p.Id /T /F | Out-File -FilePath $log -Encoding UTF8 -Append } catch {}
      try { $p.Kill() } catch {}
      try { $p.WaitForExit(5000) | Out-Null } catch {}
    }
    $stdout = $stdoutTask.Result
    $stderr = $stderrTask.Result
    if ($stdout) { "`n--- STDOUT ---`n$stdout" | Out-File -FilePath $log -Encoding UTF8 -Append }
    if ($stderr) { "`n--- STDERR ---`n$stderr" | Out-File -FilePath $log -Encoding UTF8 -Append }
    "`n--- EXIT ---" | Out-File -FilePath $log -Encoding UTF8 -Append
    "Finished: $(Get-Date -Format o)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ExitCode: $($p.ExitCode)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "TimedOut: $(-not $finished)" | Out-File -FilePath $log -Encoding UTF8 -Append
  } catch {
    "`n--- SCRIPT ERROR ---`n$($_.Exception.ToString())" | Out-File -FilePath $log -Encoding UTF8 -Append
  } finally {
    if ($p) { $p.Dispose() }
  }
}

Write-Text "00-env.txt" @"
Url=$Url
Quality=$Quality
Height=$height
Start=$Start
End=$End
Bin=$bin
yt-dlp=$yt
ffmpeg=$ffmpeg
ffprobe=$ffprobe
Output=$out
OS=$([System.Environment]::OSVersion.VersionString)
PowerShell=$($PSVersionTable.PSVersion)
"@

Write-Host "[1/6] yt-dlp -F"
Run-Proc "01-list-formats" $yt @("--no-playlist", "-F", $Url) 180

Write-Host "[2/6] yt-dlp JSON"
Run-Proc "02-info-json" $yt @("--no-playlist", "--no-warnings", "-J", $Url) 240

$jsonLog = Join-Path $out "02-info-json.log"
$jsonOut = Join-Path $out "03-format-summary.json"
try {
  $raw = Get-Content $jsonLog -Raw
  $jsonStart = $raw.IndexOf("`n--- STDOUT ---")
  if ($jsonStart -ge 0) {
    $jsonText = $raw.Substring($jsonStart + "`n--- STDOUT ---".Length)
    $exitIdx = $jsonText.IndexOf("`n--- EXIT ---")
    if ($exitIdx -ge 0) { $jsonText = $jsonText.Substring(0, $exitIdx) }
    $info = $jsonText | ConvertFrom-Json
    $formats = @($info.formats) | Where-Object { $_.height -and $_.vcodec -and $_.vcodec -ne "none" } | Sort-Object height, tbr
    $summary = $formats | Where-Object { [int]$_.height -ge [Math]::Max(720, $height - 800) } | Select-Object `
      format_id, ext, height, width, fps, protocol, vcodec, acodec, tbr, filesize, filesize_approx, url, manifest_url, `
      @{Name="has_fragments";Expression={ $null -ne $_.fragments }}, `
      @{Name="fragments_count";Expression={ if ($_.fragments) { @($_.fragments).Count } else { 0 } }}, `
      @{Name="has_init_range";Expression={ $null -ne $_.init_range }}, `
      @{Name="has_index_range";Expression={ $null -ne $_.index_range }}
    $summary | ConvertTo-Json -Depth 8 | Out-File -FilePath $jsonOut -Encoding UTF8
  }
} catch {
  Write-Text "03-format-summary-error.txt" $_.Exception.ToString()
}

$selectorDash = "bestvideo[height<=$height]+bestaudio[ext=m4a]/bestvideo[height<=$height]+bestaudio/best[height<=$height]"
$selectorHls = "best[protocol*=m3u8][height<=$height][vcodec!=none][acodec!=none]/best[protocol*=m3u8][height<=1080][vcodec!=none][acodec!=none]"

Write-Host "[3/6] print DASH URLs"
Run-Proc "04-print-dash-urls" $yt @("--no-playlist", "--no-warnings", "-g", "-f", $selectorDash, $Url) 90

Write-Host "[4/6] print HLS URL"
Run-Proc "05-print-hls-url" $yt @("--no-playlist", "--no-warnings", "-g", "-f", $selectorHls, $Url) 90

Write-Host "[5/6] DASH download-sections small test"
Run-Proc "06-dash-section-test" $yt @(
  "--no-playlist", "--newline", "--progress", "--no-warnings", "--windows-filenames", "--restrict-filenames",
  "--ffmpeg-location", $bin,
  "-f", $selectorDash,
  "--merge-output-format", "mp4",
  "--download-sections", "*$Start-$End",
  "-P", $out,
  "-o", "dash-section.%(ext)s",
  $Url
) 180

Write-Host "[6/6] HLS download-sections control test"
Run-Proc "07-hls-section-test" $yt @(
  "--no-playlist", "--newline", "--progress", "--no-warnings", "--windows-filenames", "--restrict-filenames",
  "--ffmpeg-location", $bin,
  "-f", $selectorHls,
  "--merge-output-format", "mp4",
  "--download-sections", "*$Start-$End",
  "-P", $out,
  "-o", "hls-section.%(ext)s",
  $Url
) 180

Get-ChildItem -Path $out -Force | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String | Out-File -FilePath (Join-Path $out "99-files.txt") -Encoding UTF8

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
Write-Host "Hazır: $zip"
Write-Host "Bu zip'i ChatGPT'ye yükle."
