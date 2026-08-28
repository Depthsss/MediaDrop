# MediaDrop 4K / 2K Clip Method Diagnostic
# PowerShell 5 compatible.
#
# Usage from project root:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
#   .\tools\mediadrop_4k_method_diag.ps1 -Url "https://youtu.be/aD0_HKcg-H4" -Quality "2160p" -Start 177 -End 222
#
# Optional:
#   .\tools\mediadrop_4k_method_diag.ps1 -Url "YOUTUBE_LINK" -Quality "2160p" -Start 177 -End 222 -VideoFormatId "401" -AudioFormatId "140"

param(
  [Parameter(Mandatory = $true)]
  [string]$Url,

  [string]$Quality = "2160p",

  [double]$Start = 177,

  [double]$End = 222,

  [string]$VideoFormatId = "",

  [string]$AudioFormatId = "",

  [int]$TimeoutSec = 240
)

$ErrorActionPreference = "Continue"

$bin = Join-Path $env:LOCALAPPDATA "MediaDrop\bin"
$yt = Join-Path $bin "yt-dlp.exe"
$ffmpeg = Join-Path $bin "ffmpeg.exe"
$ffprobe = Join-Path $bin "ffprobe.exe"
$aria2c = Join-Path $bin "aria2c.exe"

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:USERPROFILE "Desktop\MediaDrop4KMethodDiag-$stamp"
$zip = "$out.zip"

New-Item -ItemType Directory -Force -Path $out | Out-Null

$duration = [Math]::Max(1.0, $End - $Start)
$preRoll = 5.0
$paddedStart = [Math]::Max(0.0, $Start - $preRoll)
$paddedDuration = $End - $paddedStart

$targetHeight = 2160
if ($Quality -match "1440|2k") {
  $targetHeight = 1440
} elseif ($Quality -match "2160|4k") {
  $targetHeight = 2160
}

$culture = [System.Globalization.CultureInfo]::InvariantCulture

function Format-Seconds {
  param([double]$Value)
  return $Value.ToString("0.000", $script:culture)
}

function Q {
  param([string]$Value)

  if ($null -eq $Value) {
    return '""'
  }

  return '"' + ($Value -replace '"', '\"') + '"'
}

function Write-Text {
  param(
    [string]$Name,
    [string]$Text
  )

  $path = Join-Path $script:out $Name
  $Text | Out-File -FilePath $path -Encoding UTF8
}

function Add-Summary {
  param([string]$Text)

  $Text | Out-File -FilePath (Join-Path $script:out "00-summary.txt") -Encoding UTF8 -Append
}

function Get-Number {
  param(
    [object]$Object,
    [string]$Name
  )

  if ($null -eq $Object) {
    return 0.0
  }

  $value = $Object.$Name

  if ($null -eq $value) {
    return 0.0
  }

  $text = "$value".Trim()

  if ($text.Length -eq 0) {
    return 0.0
  }

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
  param(
    [object]$Object,
    [string]$Name
  )

  if ($null -eq $Object) {
    return ""
  }

  $value = $Object.$Name

  if ($null -eq $value) {
    return ""
  }

  return "$value"
}

function Run-Proc {
  param(
    [string]$Name,
    [string]$Exe,
    [string[]]$ProcArgs,
    [int]$TimeoutSeconds,
    [string]$WorkDir = ""
  )

  $log = Join-Path $script:out ("$Name.log")

  if ([string]::IsNullOrWhiteSpace($WorkDir)) {
    $WorkDir = $script:out
  }

  $cmdLine = (Q $Exe) + " " + (($ProcArgs | ForEach-Object { Q $_ }) -join " ")

  @(
    "=== $Name ===",
    "Started: $(Get-Date -Format o)",
    "Timeout: $TimeoutSeconds sec",
    "WorkingDirectory: $WorkDir",
    "Command:",
    $cmdLine,
    "",
    "--- OUTPUT ---"
  ) | Out-File -FilePath $log -Encoding UTF8

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $Exe
  $psi.Arguments = (($ProcArgs | ForEach-Object { Q $_ }) -join " ")
  $psi.WorkingDirectory = $WorkDir
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

      try {
        $p.Kill()
      } catch {}

      try {
        $p.WaitForExit(5000) | Out-Null
      } catch {}
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
    "Finished: $(Get-Date -Format o)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ElapsedSeconds: $([Math]::Round($sw.Elapsed.TotalSeconds, 3))" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ExitCode: $($p.ExitCode)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "TimedOut: $(-not $finished)" | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
      Name = $Name
      ExitCode = $p.ExitCode
      TimedOut = (-not $finished)
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Log = $log
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
      Log = $log
      Stdout = ""
      Stderr = $_.Exception.ToString()
    }
  } finally {
    if ($p) {
      $p.Dispose()
    }
  }
}

function Probe-File {
  param(
    [string]$Path,
    [string]$NamePrefix
  )

  if (-not (Test-Path $Path)) {
    return
  }

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
Duration=$duration
PreRoll=$preRoll
PaddedStart=$paddedStart
PaddedDuration=$paddedDuration
TimeoutSec=$TimeoutSec
Bin=$bin
yt-dlp=$yt
ffmpeg=$ffmpeg
ffprobe=$ffprobe
aria2c=$aria2c
OS=$([System.Environment]::OSVersion.VersionString)
PowerShell=$($PSVersionTable.PSVersion)
"@

Add-Summary "MediaDrop 4K Method Diagnostic"
Add-Summary "================================"
Add-Summary "Started: $(Get-Date -Format o)"
Add-Summary "URL: $Url"
Add-Summary "Quality: $Quality"
Add-Summary "Range: $Start-$End"
Add-Summary "Padded range: $paddedStart-$End"
Add-Summary ""

Write-Host "[1/12] Tool versions"
Run-Proc "01-ytdlp-version" $yt @("--version") 30 | Out-Null
Run-Proc "02-ffmpeg-version" $ffmpeg @("-version") 30 | Out-Null
Run-Proc "03-ffprobe-version" $ffprobe @("-version") 30 | Out-Null

Write-Host "[2/12] Format list and JSON"
Run-Proc "04-list-formats" $yt @("--no-playlist", "-F", $Url) 120 | Out-Null

$jsonPath = Join-Path $out "05-info.json"

$jsonResult = Run-Proc "05-info-json" $yt @(
  "--no-playlist",
  "--dump-single-json",
  "--no-warnings",
  $Url
) 180

$jsonResult.Stdout | Out-File -FilePath $jsonPath -Encoding UTF8

$info = $null

try {
  $info = Get-Content $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
} catch {
  Write-Text "05-info-json-parse-error.txt" $_.Exception.ToString()
}

if (-not $info) {
  Add-Summary "JSON parse failed. Cannot continue method selection."

  if (Test-Path $zip) {
    Remove-Item $zip -Force
  }

  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

$formats = @($info.formats)

$videoCandidates = @()

if ($VideoFormatId.Trim().Length -gt 0) {
  $videoCandidates = @($formats | Where-Object { "$(Get-TextValue $_ 'format_id')" -eq $VideoFormatId.Trim() })
} else {
  $videoCandidates = @(
    $formats |
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
      Select-Object -First 4
  )
}

$audioCandidates = @()

if ($AudioFormatId.Trim().Length -gt 0) {
  $audioCandidates = @($formats | Where-Object { "$(Get-TextValue $_ 'format_id')" -eq $AudioFormatId.Trim() })
} else {
  $audioCandidates = @(
    $formats |
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
      Select-Object -First 2
  )
}

$hlsCandidate = @(
  $formats |
    Where-Object {
      $protocol = Get-TextValue $_ "protocol"
      $vcodec = Get-TextValue $_ "vcodec"
      $acodec = Get-TextValue $_ "acodec"
      $height = Get-Number $_ "height"

      $protocol -match "m3u8" -and
      $vcodec -and
      $vcodec -ne "none" -and
      $acodec -and
      $acodec -ne "none" -and
      $height -gt 0 -and
      $height -le 1080
    } |
    Sort-Object `
      @{ Expression = { [int](Get-Number $_ "height") }; Descending = $true },
      @{ Expression = { [int](Get-Number $_ "tbr") }; Descending = $true } |
    Select-Object -First 1
)

$candidateReport = @{
  videoCandidates = $videoCandidates | Select-Object format_id, ext, protocol, width, height, fps, vcodec, acodec, tbr, filesize, filesize_approx, fragment_base_url, fragments, init_range, index_range, url
  audioCandidates = $audioCandidates | Select-Object format_id, ext, protocol, abr, vcodec, acodec, tbr, filesize, filesize_approx, url
  hlsCandidate = $hlsCandidate | Select-Object format_id, ext, protocol, width, height, fps, vcodec, acodec, tbr, filesize, filesize_approx, url
}

$candidateReport | ConvertTo-Json -Depth 8 | Out-File -FilePath (Join-Path $out "06-selected-candidates.json") -Encoding UTF8

Add-Summary "Selected video candidates:"

foreach ($v in $videoCandidates) {
  Add-Summary ("- video {0}: {1}x{2}, ext={3}, protocol={4}, vcodec={5}, hasFragments={6}, hasInitRange={7}, hasIndexRange={8}" -f `
    (Get-TextValue $v "format_id"),
    (Get-TextValue $v "width"),
    (Get-TextValue $v "height"),
    (Get-TextValue $v "ext"),
    (Get-TextValue $v "protocol"),
    (Get-TextValue $v "vcodec"),
    [bool]$v.fragments,
    [bool]$v.init_range,
    [bool]$v.index_range
  )
}

Add-Summary "Selected audio candidates:"

foreach ($a in $audioCandidates) {
  Add-Summary ("- audio {0}: ext={1}, protocol={2}, acodec={3}, abr={4}" -f `
    (Get-TextValue $a "format_id"),
    (Get-TextValue $a "ext"),
    (Get-TextValue $a "protocol"),
    (Get-TextValue $a "acodec"),
    (Get-TextValue $a "abr")
  )
}

if ($hlsCandidate) {
  Add-Summary ("HLS baseline: {0}, {1}x{2}, protocol={3}" -f `
    (Get-TextValue $hlsCandidate "format_id"),
    (Get-TextValue $hlsCandidate "width"),
    (Get-TextValue $hlsCandidate "height"),
    (Get-TextValue $hlsCandidate "protocol")
  )
}

Add-Summary ""

if (-not $videoCandidates -or $videoCandidates.Count -eq 0 -or -not $audioCandidates -or $audioCandidates.Count -eq 0) {
  Add-Summary "No suitable video/audio candidates found."

  if (Test-Path $zip) {
    Remove-Item $zip -Force
  }

  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

$mainAudio = $audioCandidates | Select-Object -First 1

Write-Host "[3/12] HLS baseline section, if available"

if ($hlsCandidate) {
  $hlsOutPattern = "method-hls-baseline.%(ext)s"

  $hlsFormatId = Get-TextValue $hlsCandidate "format_id"

  $r = Run-Proc "10-hls-baseline-section" $yt @(
    "--no-playlist",
    "--newline",
    "--progress",
    "--no-warnings",
    "--windows-filenames",
    "--restrict-filenames",
    "--ffmpeg-location", $bin,
    "-f", $hlsFormatId,
    "--download-sections", ("*" + (Format-Seconds $paddedStart) + "-" + (Format-Seconds $End)),
    "-P", $out,
    "-o", $hlsOutPattern,
    $Url
  ) 180

  $file = Existing-Media "method-hls-baseline.*"

  if ($file) {
    Probe-File $file.FullName "method-hls-baseline"
  }

  $fileName = "none"
  if ($file) {
    $fileName = $file.Name
  }

  Add-Summary ("HLS baseline: exit={0}, timeout={1}, seconds={2}, file={3}" -f $r.ExitCode, $r.TimedOut, $r.Seconds, $fileName)
}

$methodIndex = 20

foreach ($v in $videoCandidates) {
  $vf = Get-TextValue $v "format_id"
  $af = Get-TextValue $mainAudio "format_id"
  $pair = "$vf+$af"
  $label = "v$vf-a$af"

  Add-Summary ""
  Add-Summary "Testing pair: $pair"

  Write-Host "[4/12] yt-dlp section no-force: $pair"

  $r = Run-Proc "$methodIndex-ytdlp-section-noforce-$label" $yt @(
    "--no-playlist",
    "--newline",
    "--progress",
    "--no-warnings",
    "--windows-filenames",
    "--restrict-filenames",
    "--ffmpeg-location", $bin,
    "-f", $pair,
    "--merge-output-format", "mp4",
    "--download-sections", ("*" + (Format-Seconds $paddedStart) + "-" + (Format-Seconds $End)),
    "-P", $out,
    "-o", "method-ytdlp-noforce-$label.%(ext)s",
    $Url
  ) $TimeoutSec

  $file = Existing-Media "method-ytdlp-noforce-$label.*"

  if ($file) {
    Probe-File $file.FullName "method-ytdlp-noforce-$label"
  }

  $fileName = "none"
  if ($file) {
    $fileName = $file.Name
  }

  Add-Summary ("yt-dlp section no-force {0}: exit={1}, timeout={2}, seconds={3}, file={4}" -f $pair, $r.ExitCode, $r.TimedOut, $r.Seconds, $fileName)
  $methodIndex += 1

  Write-Host "[5/12] yt-dlp section force-keyframes: $pair"

  $r = Run-Proc "$methodIndex-ytdlp-section-force-$label" $yt @(
    "--no-playlist",
    "--newline",
    "--progress",
    "--no-warnings",
    "--windows-filenames",
    "--restrict-filenames",
    "--ffmpeg-location", $bin,
    "-f", $pair,
    "--merge-output-format", "mp4",
    "--download-sections", ("*" + (Format-Seconds $paddedStart) + "-" + (Format-Seconds $End)),
    "--force-keyframes-at-cuts",
    "-P", $out,
    "-o", "method-ytdlp-force-$label.%(ext)s",
    $Url
  ) $TimeoutSec

  $file = Existing-Media "method-ytdlp-force-$label.*"

  if ($file) {
    Probe-File $file.FullName "method-ytdlp-force-$label"
  }

  $fileName = "none"
  if ($file) {
    $fileName = $file.Name
  }

  Add-Summary ("yt-dlp section force {0}: exit={1}, timeout={2}, seconds={3}, file={4}" -f $pair, $r.ExitCode, $r.TimedOut, $r.Seconds, $fileName)
  $methodIndex += 1

  Write-Host "[6/12] Resolve direct URLs: $pair"

  $urlResult = Run-Proc "$methodIndex-resolve-urls-$label" $yt @(
    "--no-playlist",
    "--no-warnings",
    "--get-url",
    "-f", $pair,
    $Url
  ) 90

  $methodIndex += 1

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
    continue
  }

  Write-Text "resolved-url-$label.txt" ("VIDEO_URL=`n$videoUrl`n`nAUDIO_URL=`n$audioUrl")

  $tempVideoMkv = Join-Path $out "temp-video-$label.mkv"
  $tempAudioMka = Join-Path $out "temp-audio-$label.mka"
  $finalMkv = Join-Path $out "method-split-copy-$label.mkv"
  $finalEncodeMp4Nvenc = Join-Path $out "method-split-encode-nvenc-$label.mp4"
  $finalEncodeMp4X264 = Join-Path $out "method-split-encode-x264-$label.mp4"
  $directMkv = Join-Path $out "method-direct-copy-$label.mkv"

  Write-Host "[7/12] ffmpeg direct remote copy merge: $pair"

  $r = Run-Proc "$methodIndex-ffmpeg-direct-copy-$label" $ffmpeg @(
    "-hide_banner",
    "-y",
    "-ss", (Format-Seconds $paddedStart),
    "-i", $videoUrl,
    "-ss", (Format-Seconds $paddedStart),
    "-i", $audioUrl,
    "-t", (Format-Seconds $paddedDuration),
    "-map", "0:v:0",
    "-map", "1:a:0",
    "-c", "copy",
    "-avoid_negative_ts", "make_zero",
    "-fflags", "+genpts",
    $directMkv
  ) $TimeoutSec

  if (Test-Path $directMkv) {
    Probe-File $directMkv "method-direct-copy-$label"
  }

  Add-Summary ("ffmpeg direct copy {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $pair, $r.ExitCode, $r.TimedOut, $r.Seconds, (Test-Path $directMkv))
  $methodIndex += 1

  Write-Host "[8/12] ffmpeg split remote video copy: $vf"

  $rVideo = Run-Proc "$methodIndex-ffmpeg-split-video-copy-$label" $ffmpeg @(
    "-hide_banner",
    "-y",
    "-ss", (Format-Seconds $paddedStart),
    "-i", $videoUrl,
    "-t", (Format-Seconds $paddedDuration),
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

  Add-Summary ("split video copy {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $vf, $rVideo.ExitCode, $rVideo.TimedOut, $rVideo.Seconds, (Test-Path $tempVideoMkv))
  $methodIndex += 1

  Write-Host "[9/12] ffmpeg split remote audio copy: $af"

  $rAudio = Run-Proc "$methodIndex-ffmpeg-split-audio-copy-$label" $ffmpeg @(
    "-hide_banner",
    "-y",
    "-ss", (Format-Seconds $paddedStart),
    "-i", $audioUrl,
    "-t", (Format-Seconds $paddedDuration),
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

  Add-Summary ("split audio copy {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $af, $rAudio.ExitCode, $rAudio.TimedOut, $rAudio.Seconds, (Test-Path $tempAudioMka))
  $methodIndex += 1

  if ((Test-Path $tempVideoMkv) -and (Test-Path $tempAudioMka)) {
    Write-Host "[10/12] local merge split outputs: $pair"

    $rMerge = Run-Proc "$methodIndex-local-merge-split-copy-$label" $ffmpeg @(
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
    ) 90

    if (Test-Path $finalMkv) {
      Probe-File $finalMkv "method-split-copy-$label"
    }

    Add-Summary ("local merge split copy {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $pair, $rMerge.ExitCode, $rMerge.TimedOut, $rMerge.Seconds, (Test-Path $finalMkv))
    $methodIndex += 1

    Write-Host "[11/12] local encode split outputs to MP4 NVENC: $pair"

    $rEncode = Run-Proc "$methodIndex-local-encode-split-nvenc-mp4-$label" $ffmpeg @(
      "-hide_banner",
      "-y",
      "-i", $tempVideoMkv,
      "-i", $tempAudioMka,
      "-map", "0:v:0",
      "-map", "1:a:0",
      "-c:v", "h264_nvenc",
      "-preset", "p5",
      "-cq", "21",
      "-c:a", "aac",
      "-b:a", "192k",
      "-movflags", "+faststart",
      $finalEncodeMp4Nvenc
    ) 180

    if (Test-Path $finalEncodeMp4Nvenc) {
      Probe-File $finalEncodeMp4Nvenc "method-split-encode-nvenc-$label"
    }

    Add-Summary ("local encode split nvenc mp4 {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $pair, $rEncode.ExitCode, $rEncode.TimedOut, $rEncode.Seconds, (Test-Path $finalEncodeMp4Nvenc))
    $methodIndex += 1

    Write-Host "[11b/12] local encode split outputs to MP4 x264: $pair"

    $rEncodeX264 = Run-Proc "$methodIndex-local-encode-split-x264-mp4-$label" $ffmpeg @(
      "-hide_banner",
      "-y",
      "-i", $tempVideoMkv,
      "-i", $tempAudioMka,
      "-map", "0:v:0",
      "-map", "1:a:0",
      "-c:v", "libx264",
      "-preset", "veryfast",
      "-crf", "20",
      "-c:a", "aac",
      "-b:a", "192k",
      "-movflags", "+faststart",
      $finalEncodeMp4X264
    ) 240

    if (Test-Path $finalEncodeMp4X264) {
      Probe-File $finalEncodeMp4X264 "method-split-encode-x264-$label"
    }

    Add-Summary ("local encode split x264 mp4 {0}: exit={1}, timeout={2}, seconds={3}, fileExists={4}" -f $pair, $rEncodeX264.ExitCode, $rEncodeX264.TimedOut, $rEncodeX264.Seconds, (Test-Path $finalEncodeMp4X264))
    $methodIndex += 1
  }
}

Write-Host "[12/12] Collect files"

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