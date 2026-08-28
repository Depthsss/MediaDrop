# MediaDrop 4K Light Diagnostic
# PowerShell 5 compatible.
#
# Usage:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
#   .\tools\mediadrop_4k_light_diag.ps1 -Url "https://youtu.be/aD0_HKcg-H4" -Quality "2160p" -Start 177
#
# Optional exact format:
#   .\tools\mediadrop_4k_light_diag.ps1 -Url "https://youtu.be/aD0_HKcg-H4" -Start 177 -VideoFormatId "401" -AudioFormatId "140"

param(
  [Parameter(Mandatory = $true)]
  [string]$Url,

  [string]$Quality = "2160p",

  [double]$Start = 177,

  [double]$Seconds = 8,

  [string]$VideoFormatId = "",

  [string]$AudioFormatId = "",

  [int]$TimeoutSec = 75
)

$ErrorActionPreference = "Continue"

$bin = Join-Path $env:LOCALAPPDATA "MediaDrop\bin"
$yt = Join-Path $bin "yt-dlp.exe"
$ffmpeg = Join-Path $bin "ffmpeg.exe"
$ffprobe = Join-Path $bin "ffprobe.exe"

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:USERPROFILE "Desktop\MediaDrop4KLightDiag-$stamp"
$zip = "$out.zip"

New-Item -ItemType Directory -Force -Path $out | Out-Null

$testSeconds = [Math]::Max(4.0, [Math]::Min($Seconds, 10.0))
$End = $Start + $testSeconds

$preRoll = 2.0
$paddedStart = [Math]::Max(0.0, $Start - $preRoll)
$paddedDuration = $End - $paddedStart

$targetHeight = 2160
if ($Quality -match "1440|2k") {
  $targetHeight = 1440
} elseif ($Quality -match "2160|4k") {
  $targetHeight = 2160
}

$culture = [System.Globalization.CultureInfo]::InvariantCulture

function FS {
  param([double]$Value)
  return $Value.ToString("0.000", $script:culture)
}

function Q {
  param([string]$Value)
  if ($null -eq $Value) { return '""' }
  return '"' + ($Value -replace '"', '\"') + '"'
}

function Write-Text {
  param([string]$Name, [string]$Text)
  $path = Join-Path $script:out $Name
  $Text | Out-File -FilePath $path -Encoding UTF8
}

function Add-Summary {
  param([string]$Text)
  $Text | Out-File -FilePath (Join-Path $script:out "00-summary.txt") -Encoding UTF8 -Append
}

function Get-Number {
  param([object]$Object, [string]$Name)

  if ($null -eq $Object) { return 0.0 }

  $value = $Object.$Name
  if ($null -eq $value) { return 0.0 }

  $text = "$value".Trim()
  if ($text.Length -eq 0) { return 0.0 }

  $parsed = 0.0

  if ([double]::TryParse($text, [System.Globalization.NumberStyles]::Any, $script:culture, [ref]$parsed)) {
    return $parsed
  }

  $text = $text.Replace(",", ".")

  if ([double]::TryParse($text, [System.Globalization.NumberStyles]::Any, $script:culture, [ref]$parsed)) {
    return $parsed
  }

  return 0.0
}

function Get-TextValue {
  param([object]$Object, [string]$Name)

  if ($null -eq $Object) { return "" }

  $value = $Object.$Name
  if ($null -eq $value) { return "" }

  return "$value"
}

function Run-Proc {
  param(
    [string]$Name,
    [string]$Exe,
    [string[]]$ProcArgs,
    [int]$TimeoutSeconds
  )

  $log = Join-Path $script:out ("$Name.log")
  $cmdLine = (Q $Exe) + " " + (($ProcArgs | ForEach-Object { Q $_ }) -join " ")

  @(
    "=== $Name ===",
    "Started: $(Get-Date -Format o)",
    "Timeout: $TimeoutSeconds sec",
    "Command:",
    $cmdLine,
    "",
    "--- OUTPUT ---"
  ) | Out-File -FilePath $log -Encoding UTF8

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $Exe
  $psi.Arguments = (($ProcArgs | ForEach-Object { Q $_ }) -join " ")
  $psi.WorkingDirectory = $script:out
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true

  $p = New-Object System.Diagnostics.Process
  $p.StartInfo = $psi
  $sw = [System.Diagnostics.Stopwatch]::StartNew()

  try {
    [void]$p.Start()

    $stdoutTask = $p.StandardOutput.ReadToEndAsync()
    $stderrTask = $p.StandardError.ReadToEndAsync()

    $finished = $p.WaitForExit($TimeoutSeconds * 1000)
    $sw.Stop()

    if (-not $finished) {
      "`n--- TIMEOUT: killing PID $($p.Id) ---" | Out-File -FilePath $log -Encoding UTF8 -Append

      try {
        & taskkill.exe /PID $p.Id /T /F | Out-File -FilePath $log -Encoding UTF8 -Append
      } catch {}

      try { $p.Kill() } catch {}
      try { $p.WaitForExit(5000) | Out-Null } catch {}
    }

    $stdout = $stdoutTask.Result
    $stderr = $stderrTask.Result

    if ($stdout) {
      "`n--- STDOUT ---`n$stdout" | Out-File -FilePath $log -Encoding UTF8 -Append
    }

    if ($stderr) {
      "`n--- STDERR ---`n$stderr" | Out-File -FilePath $log -Encoding UTF8 -Append
    }

    "`n--- EXIT ---" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ElapsedSeconds: $([Math]::Round($sw.Elapsed.TotalSeconds, 3))" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ExitCode: $($p.ExitCode)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "TimedOut: $(-not $finished)" | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
      Name = $Name
      ExitCode = $p.ExitCode
      TimedOut = (-not $finished)
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Stdout = $stdout
      Stderr = $stderr
    }
  } catch {
    $sw.Stop()

    "`n--- SCRIPT ERROR ---`n$($_.Exception.ToString())" | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
      Name = $Name
      ExitCode = -999
      TimedOut = $false
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Stdout = ""
      Stderr = $_.Exception.ToString()
    }
  } finally {
    if ($p) { $p.Dispose() }
  }
}

function Probe-File {
  param([string]$Path, [string]$NamePrefix)

  if (-not (Test-Path $Path)) { return }

  $safe = ($NamePrefix -replace '[^a-zA-Z0-9._-]', '_')

  Run-Proc "probe-$safe" $script:ffprobe @(
    "-hide_banner",
    "-v", "error",
    "-show_entries", "format=duration,size,format_name:stream=index,codec_type,codec_name,width,height,duration",
    "-of", "json",
    $Path
  ) 30 | Out-Null
}

function Existing-Media {
  param([string]$Pattern)

  return Get-ChildItem -Path $script:out -File -Filter $Pattern -ErrorAction SilentlyContinue |
    Sort-Object Length -Descending |
    Select-Object -First 1
}

Write-Text "00-env.txt" @"
URL=$Url
Quality=$Quality
TargetHeight=$targetHeight
Start=$Start
End=$End
TestSeconds=$testSeconds
PreRoll=$preRoll
PaddedStart=$paddedStart
PaddedDuration=$paddedDuration
TimeoutSec=$TimeoutSec
Bin=$bin
yt-dlp=$yt
ffmpeg=$ffmpeg
ffprobe=$ffprobe
OS=$([System.Environment]::OSVersion.VersionString)
PowerShell=$($PSVersionTable.PSVersion)
"@

Add-Summary "MediaDrop 4K Light Diagnostic"
Add-Summary "=============================="
Add-Summary "URL: $Url"
Add-Summary "Quality: $Quality"
Add-Summary "Range: $Start-$End"
Add-Summary "Padded range: $paddedStart-$End"
Add-Summary ""

Write-Host "[1/8] Tool versions"
Run-Proc "01-ytdlp-version" $yt @("--version") 30 | Out-Null
Run-Proc "02-ffmpeg-version" $ffmpeg @("-version") 30 | Out-Null

Write-Host "[2/8] Fetch video JSON"
$jsonPath = Join-Path $out "03-info.json"

$jsonResult = Run-Proc "03-info-json" $yt @(
  "--no-playlist",
  "--dump-single-json",
  "--no-warnings",
  $Url
) 120

$jsonResult.Stdout | Out-File -FilePath $jsonPath -Encoding UTF8

$info = $null

try {
  $info = Get-Content $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
} catch {
  Write-Text "03-info-json-parse-error.txt" $_.Exception.ToString()
}

if (-not $info) {
  Add-Summary "JSON parse failed. Test stopped."
  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

$formats = @($info.formats)

if ($VideoFormatId.Trim().Length -gt 0) {
  $video = $formats | Where-Object { "$(Get-TextValue $_ 'format_id')" -eq $VideoFormatId.Trim() } | Select-Object -First 1
} else {
  $video = $formats |
    Where-Object {
      $vcodec = Get-TextValue $_ "vcodec"
      $acodec = Get-TextValue $_ "acodec"
      $height = Get-Number $_ "height"

      $vcodec -and
      $vcodec -ne "none" -and
      ($acodec -eq "" -or $acodec -eq "none") -and
      $height -ge 1440
    } |
    Sort-Object `
      @{ Expression = { [Math]::Abs((Get-Number $_ "height") - $script:targetHeight) }; Ascending = $true },
      @{ Expression = { if ((Get-TextValue $_ "ext") -eq "mp4") { 0 } elseif ((Get-TextValue $_ "ext") -eq "webm") { 1 } else { 2 } }; Ascending = $true },
      @{ Expression = { if ((Get-TextValue $_ "vcodec") -match "av01") { 0 } elseif ((Get-TextValue $_ "vcodec") -match "vp9") { 1 } else { 2 } }; Ascending = $true },
      @{ Expression = { [int](Get-Number $_ "tbr") }; Descending = $true } |
    Select-Object -First 1
}

if ($AudioFormatId.Trim().Length -gt 0) {
  $audio = $formats | Where-Object { "$(Get-TextValue $_ 'format_id')" -eq $AudioFormatId.Trim() } | Select-Object -First 1
} else {
  $audio = $formats |
    Where-Object {
      $vcodec = Get-TextValue $_ "vcodec"
      $acodec = Get-TextValue $_ "acodec"

      $acodec -and
      $acodec -ne "none" -and
      ($vcodec -eq "" -or $vcodec -eq "none")
    } |
    Sort-Object `
      @{ Expression = { if ((Get-TextValue $_ "format_id") -eq "140") { 0 } elseif ((Get-TextValue $_ "ext") -eq "m4a") { 1 } else { 2 } }; Ascending = $true },
      @{ Expression = { [int](Get-Number $_ "abr") }; Descending = $true } |
    Select-Object -First 1
}

$hls = $formats |
  Where-Object {
    $protocol = Get-TextValue $_ "protocol"
    $vcodec = Get-TextValue $_ "vcodec"
    $acodec = Get-TextValue $_ "acodec"
    $height = Get-Number $_ "height"

    $protocol -match "m3u8" -and
    $vcodec -and $vcodec -ne "none" -and
    $acodec -and $acodec -ne "none" -and
    $height -gt 0 -and
    $height -le 1080
  } |
  Sort-Object `
    @{ Expression = { [int](Get-Number $_ "height") }; Descending = $true },
    @{ Expression = { [int](Get-Number $_ "tbr") }; Descending = $true } |
  Select-Object -First 1

$candidateReport = @{
  video = $video | Select-Object format_id, ext, protocol, width, height, fps, vcodec, acodec, tbr, filesize, filesize_approx, fragment_base_url, fragments, init_range, index_range, url
  audio = $audio | Select-Object format_id, ext, protocol, abr, vcodec, acodec, tbr, filesize, filesize_approx, url
  hls = $hls | Select-Object format_id, ext, protocol, width, height, fps, vcodec, acodec, tbr, filesize, filesize_approx, url
}

$candidateReport | ConvertTo-Json -Depth 8 | Out-File -FilePath (Join-Path $out "04-selected-candidates.json") -Encoding UTF8

if (-not $video -or -not $audio) {
  Add-Summary "No suitable 2K/4K video or audio candidate found."
  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

$vf = Get-TextValue $video "format_id"
$af = Get-TextValue $audio "format_id"
$pair = "$vf+$af"
$label = "v$vf-a$af"

Add-Summary ("Selected video: {0}, {1}x{2}, ext={3}, protocol={4}, vcodec={5}" -f `
  $vf,
  (Get-TextValue $video "width"),
  (Get-TextValue $video "height"),
  (Get-TextValue $video "ext"),
  (Get-TextValue $video "protocol"),
  (Get-TextValue $video "vcodec")
)

Add-Summary ("Selected audio: {0}, ext={1}, protocol={2}, acodec={3}" -f `
  $af,
  (Get-TextValue $audio "ext"),
  (Get-TextValue $audio "protocol"),
  (Get-TextValue $audio "acodec")
)

if ($hls) {
  Add-Summary ("HLS baseline: {0}, {1}x{2}, protocol={3}" -f `
    (Get-TextValue $hls "format_id"),
    (Get-TextValue $hls "width"),
    (Get-TextValue $hls "height"),
    (Get-TextValue $hls "protocol")
  )
}

Add-Summary ""

Write-Host "[3/8] Short HLS baseline"

if ($hls) {
  $hlsFormatId = Get-TextValue $hls "format_id"

  $rHls = Run-Proc "10-hls-baseline-short" $yt @(
    "--no-playlist",
    "--newline",
    "--progress",
    "--no-warnings",
    "--windows-filenames",
    "--restrict-filenames",
    "--ffmpeg-location", $bin,
    "-f", $hlsFormatId,
    "--download-sections", ("*" + (FS $paddedStart) + "-" + (FS $End)),
    "-P", $out,
    "-o", "hls-baseline.%(ext)s",
    $Url
  ) 75

  $hlsFile = Existing-Media "hls-baseline.*"

  if ($hlsFile) {
    Probe-File $hlsFile.FullName "hls-baseline"
  }

  $hlsName = "none"
  if ($hlsFile) { $hlsName = $hlsFile.Name }

  Add-Summary ("HLS baseline: exit={0}, timeout={1}, seconds={2}, file={3}" -f $rHls.ExitCode, $rHls.TimedOut, $rHls.Seconds, $hlsName)
}

Write-Host "[4/8] 4K yt-dlp section no-force"

$rYtdlp = Run-Proc "20-ytdlp-section-noforce-$label" $yt @(
  "--no-playlist",
  "--newline",
  "--progress",
  "--no-warnings",
  "--windows-filenames",
  "--restrict-filenames",
  "--ffmpeg-location", $bin,
  "-f", $pair,
  "--merge-output-format", "mp4",
  "--download-sections", ("*" + (FS $paddedStart) + "-" + (FS $End)),
  "-P", $out,
  "-o", "ytdlp-section-$label.%(ext)s",
  $Url
) $TimeoutSec

$ytdlpFile = Existing-Media "ytdlp-section-$label.*"

if ($ytdlpFile) {
  Probe-File $ytdlpFile.FullName "ytdlp-section-$label"
}

$ytdlpName = "none"
if ($ytdlpFile) { $ytdlpName = $ytdlpFile.Name }

Add-Summary ("yt-dlp section {0}: exit={1}, timeout={2}, seconds={3}, file={4}" -f $pair, $rYtdlp.ExitCode, $rYtdlp.TimedOut, $rYtdlp.Seconds, $ytdlpName)

Write-Host "[5/8] Resolve direct URLs"

$urlResult = Run-Proc "30-resolve-urls-$label" $yt @(
  "--no-playlist",
  "--no-warnings",
  "--get-url",
  "-f", $pair,
  $Url
) 75

$rawLines = $urlResult.Stdout -split "`r?`n"
$lines = @($rawLines | Where-Object { $_.Trim() -match '^https?://' } | ForEach-Object { $_.Trim() })

$videoUrl = ""
$audioUrl = ""

if ($lines.Count -ge 2) {
  $videoUrl = $lines[0]
  $audioUrl = $lines[1]
} elseif ($lines.Count -eq 1) {
  $videoUrl = $lines[0]
}

if (-not $videoUrl -or -not $audioUrl) {
  Add-Summary "resolve urls ${pair}: failed; videoUrl=$([bool]$videoUrl), audioUrl=$([bool]$audioUrl)"
} else {
  Write-Text "resolved-url-$label.txt" ("VIDEO_URL=`n$videoUrl`n`nAUDIO_URL=`n$audioUrl")

  $tempVideoMkv = Join-Path $out "temp-video-$label.mkv"
  $tempAudioMka = Join-Path $out "temp-audio-$label.mka"
  $finalMkv = Join-Path $out "split-merge-$label.mkv"

  Write-Host "[6/8] 4K ffmpeg split video short"

  $rVideo = Run-Proc "40-ffmpeg-split-video-$label" $ffmpeg @(
    "-hide_banner",
    "-y",
    "-ss", (FS $paddedStart),
    "-i", $videoUrl,
    "-t", (FS $paddedDuration),
    "-map", "0:v:0",
    "-an",
    "-c:v", "copy",
    "-avoid_negative_ts", "make_zero",
    "-fflags", "+genpts",
    $tempVideoMkv
  ) $TimeoutSec

  if (Test-Path $tempVideoMkv) {
    Probe-File $tempVideoMkv "temp-video-$label"
  }

  Add-Summary ("split video {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $vf, $rVideo.ExitCode, $rVideo.TimedOut, $rVideo.Seconds, (Test-Path $tempVideoMkv))

  Write-Host "[7/8] 4K ffmpeg split audio short"

  $rAudio = Run-Proc "50-ffmpeg-split-audio-$label" $ffmpeg @(
    "-hide_banner",
    "-y",
    "-ss", (FS $paddedStart),
    "-i", $audioUrl,
    "-t", (FS $paddedDuration),
    "-map", "0:a:0",
    "-vn",
    "-c:a", "copy",
    "-avoid_negative_ts", "make_zero",
    "-fflags", "+genpts",
    $tempAudioMka
  ) $TimeoutSec

  if (Test-Path $tempAudioMka) {
    Probe-File $tempAudioMka "temp-audio-$label"
  }

  Add-Summary ("split audio {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $af, $rAudio.ExitCode, $rAudio.TimedOut, $rAudio.Seconds, (Test-Path $tempAudioMka))

  if ((Test-Path $tempVideoMkv) -and (Test-Path $tempAudioMka)) {
    Write-Host "[8/8] Local merge split outputs"

    $rMerge = Run-Proc "60-local-merge-split-$label" $ffmpeg @(
      "-hide_banner",
      "-y",
      "-i", $tempVideoMkv,
      "-i", $tempAudioMka,
      "-map", "0:v:0",
      "-map", "1:a:0",
      "-c", "copy",
      "-avoid_negative_ts", "make_zero",
      "-fflags", "+genpts",
      $finalMkv
    ) 60

    if (Test-Path $finalMkv) {
      Probe-File $finalMkv "split-merge-$label"
    }

    Add-Summary ("local merge {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $pair, $rMerge.ExitCode, $rMerge.TimedOut, $rMerge.Seconds, (Test-Path $finalMkv))
  }
}

Get-ChildItem -Path $out -Force |
  Select-Object Name, Length, LastWriteTime |
  Format-Table -AutoSize |
  Out-String |
  Out-File -FilePath (Join-Path $out "99-files.txt") -Encoding UTF8

Add-Summary ""
Add-Summary "Finished: $(Get-Date -Format o)"
Add-Summary "Zip: $zip"

if (Test-Path $zip) {
  Remove-Item $zip -Force
}

Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force

Write-Host ""
Write-Host "Hazır: $zip"
Write-Host "Bu zip'i ChatGPT'ye yükle."