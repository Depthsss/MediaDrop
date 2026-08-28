# MediaDrop DASH Range Probe
# PowerShell 5 compatible.
#
# Purpose:
#   Checks whether YouTube 2K/4K direct video/audio URLs support HTTP byte-range.
#   Downloads only tiny byte ranges, NOT the full video.
#
# Usage:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
#   .\tools\mediadrop_dash_range_probe.ps1 -Url "https://youtu.be/aD0_HKcg-H4" -Quality "2160p"
#
# Optional exact formats:
#   .\tools\mediadrop_dash_range_probe.ps1 -Url "https://youtu.be/aD0_HKcg-H4" -VideoFormatId "401" -AudioFormatId "140"

param(
  [Parameter(Mandatory = $true)]
  [string]$Url,

  [string]$Quality = "2160p",

  [string]$VideoFormatId = "",

  [string]$AudioFormatId = "",

  [int]$VideoProbeMb = 4,

  [int]$AudioProbeMb = 1
)

$ErrorActionPreference = "Continue"

$bin = Join-Path $env:LOCALAPPDATA "MediaDrop\bin"
$yt = Join-Path $bin "yt-dlp.exe"
$ffprobe = Join-Path $bin "ffprobe.exe"

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = Join-Path $env:USERPROFILE "Desktop\MediaDropDashRangeProbe-$stamp"
$zip = "$out.zip"

New-Item -ItemType Directory -Force -Path $out | Out-Null

$targetHeight = 2160
if ($Quality -match "1440|2k") {
  $targetHeight = 1440
} elseif ($Quality -match "2160|4k") {
  $targetHeight = 2160
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

  $culture = [System.Globalization.CultureInfo]::InvariantCulture
  $parsed = 0.0

  if ([double]::TryParse($text, [System.Globalization.NumberStyles]::Any, $culture, [ref]$parsed)) {
    return $parsed
  }

  $text = $text.Replace(",", ".")

  if ([double]::TryParse($text, [System.Globalization.NumberStyles]::Any, $culture, [ref]$parsed)) {
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

      try {
        $p.Kill()
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
    "ElapsedSeconds: $([Math]::Round($sw.Elapsed.TotalSeconds, 3))" | Out-File -FilePath $log -Encoding UTF8 -Append
    "ExitCode: $($p.ExitCode)" | Out-File -FilePath $log -Encoding UTF8 -Append
    "TimedOut: $(-not $finished)" | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
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
      ExitCode = -999
      TimedOut = $false
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Stdout = ""
      Stderr = $_.Exception.ToString()
    }
  } finally {
    if ($p) {
      $p.Dispose()
    }
  }
}

function Save-Url-Redacted {
  param(
    [string]$Name,
    [string]$DirectUrl
  )

  $redacted = $DirectUrl

  $redacted = $redacted -replace '([?&](sig|lsig|signature|n|expire|ei|ip|ipbits|id|itag|source|requiressl|mh|mm|mn|ms|mv|mvi|pl|gcr|initcwndbps|bui|spc|vprv|svpuc|mime|ns|rqh|gir|clen|dur|lmt|mt|fvip|keepalive|fexp|c|txp|sparams|alr|cpn|cver|rn|rbuf|pot|range|ratebypass)=)[^&]+', '$1REDACTED'

  Write-Text $Name $redacted
}

function Get-AsciiStrings {
  param(
    [byte[]]$Bytes,
    [int]$MinLen = 4
  )

  $strings = New-Object System.Collections.Generic.List[string]
  $current = New-Object System.Text.StringBuilder

  foreach ($b in $Bytes) {
    if ($b -ge 32 -and $b -le 126) {
      [void]$current.Append([char]$b)
    } else {
      if ($current.Length -ge $MinLen) {
        $strings.Add($current.ToString())
      }

      $current.Clear() | Out-Null
    }
  }

  if ($current.Length -ge $MinLen) {
    $strings.Add($current.ToString())
  }

  return $strings
}

function Find-Ascii-Offsets {
  param(
    [byte[]]$Bytes,
    [string[]]$Needles
  )

  $result = @{}

  foreach ($needle in $Needles) {
    $needleBytes = [System.Text.Encoding]::ASCII.GetBytes($needle)
    $offsets = New-Object System.Collections.Generic.List[int]

    for ($i = 0; $i -le $Bytes.Length - $needleBytes.Length; $i++) {
      $match = $true

      for ($j = 0; $j -lt $needleBytes.Length; $j++) {
        if ($Bytes[$i + $j] -ne $needleBytes[$j]) {
          $match = $false
          break
        }
      }

      if ($match) {
        $offsets.Add($i)
      }
    }

    $result[$needle] = @($offsets)
  }

  return $result
}

function Save-Hexdump {
  param(
    [byte[]]$Bytes,
    [string]$Path,
    [int]$MaxBytes = 4096
  )

  $limit = [Math]::Min($Bytes.Length, $MaxBytes)
  $lines = New-Object System.Collections.Generic.List[string]

  for ($i = 0; $i -lt $limit; $i += 16) {
    $count = [Math]::Min(16, $limit - $i)
    $slice = $Bytes[$i..($i + $count - 1)]

    $hex = ($slice | ForEach-Object { $_.ToString("X2") }) -join " "
    $ascii = ($slice | ForEach-Object {
      if ($_ -ge 32 -and $_ -le 126) {
        [char]$_
      } else {
        "."
      }
    }) -join ""

    $lines.Add(("{0:X8}  {1,-48}  {2}" -f $i, $hex, $ascii))
  }

  $lines | Out-File -FilePath $Path -Encoding UTF8
}

function Get-Range {
  param(
    [string]$Name,
    [string]$DirectUrl,
    [int64]$StartByte,
    [int64]$EndByte
  )

  $log = Join-Path $script:out ("range-$Name.log")
  $binPath = Join-Path $script:out ("range-$Name.bin")
  $hexPath = Join-Path $script:out ("range-$Name-hex.txt")
  $stringsPath = Join-Path $script:out ("range-$Name-strings.txt")
  $boxesPath = Join-Path $script:out ("range-$Name-boxes.json")

  $rangeHeader = "bytes=$StartByte-$EndByte"

  @(
    "=== RANGE $Name ===",
    "Started: $(Get-Date -Format o)",
    "Range: $rangeHeader",
    "Output: $binPath",
    ""
  ) | Out-File -FilePath $log -Encoding UTF8

  $sw = [System.Diagnostics.Stopwatch]::StartNew()

  try {
    $request = [System.Net.HttpWebRequest]::Create($DirectUrl)
    $request.Method = "GET"
    $request.UserAgent = "Mozilla/5.0"
    $request.AddRange($StartByte, $EndByte)
    $request.Timeout = 30000
    $request.ReadWriteTimeout = 30000
    $request.AllowAutoRedirect = $true

    $response = $request.GetResponse()
    $statusCode = [int]$response.StatusCode
    $statusDescription = $response.StatusDescription
    $contentLength = $response.ContentLength
    $contentRange = $response.Headers["Content-Range"]
    $acceptRanges = $response.Headers["Accept-Ranges"]
    $contentType = $response.Headers["Content-Type"]

    $stream = $response.GetResponseStream()
    $fileStream = [System.IO.File]::Open($binPath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)

    $buffer = New-Object byte[] 8192

    while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
      $fileStream.Write($buffer, 0, $read)
    }

    $fileStream.Close()
    $stream.Close()
    $response.Close()

    $sw.Stop()

    $bytes = [System.IO.File]::ReadAllBytes($binPath)
    $strings = Get-AsciiStrings $bytes 4
    $boxHits = Find-Ascii-Offsets $bytes @("ftyp", "moov", "moof", "mdat", "sidx", "mfra", "free", "wide", "av01", "vp09", "mp4a", "dash", "emsg")

    Save-Hexdump $bytes $hexPath 8192
    $strings | Select-Object -First 300 | Out-File -FilePath $stringsPath -Encoding UTF8
    $boxHits | ConvertTo-Json -Depth 5 | Out-File -FilePath $boxesPath -Encoding UTF8

    @(
      "StatusCode: $statusCode",
      "StatusDescription: $statusDescription",
      "ContentLengthHeader: $contentLength",
      "DownloadedBytes: $($bytes.Length)",
      "Content-Range: $contentRange",
      "Accept-Ranges: $acceptRanges",
      "Content-Type: $contentType",
      "ElapsedSeconds: $([Math]::Round($sw.Elapsed.TotalSeconds, 3))",
      "",
      "BoxHits:",
      ($boxHits | ConvertTo-Json -Depth 5)
    ) | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
      Name = $Name
      Ok = $true
      StatusCode = $statusCode
      ContentRange = $contentRange
      AcceptRanges = $acceptRanges
      ContentType = $contentType
      DownloadedBytes = $bytes.Length
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Bin = $binPath
      BoxHits = $boxHits
    }
  } catch {
    $sw.Stop()

    "`nERROR:`n$($_.Exception.ToString())" | Out-File -FilePath $log -Encoding UTF8 -Append

    return [PSCustomObject]@{
      Name = $Name
      Ok = $false
      StatusCode = 0
      ContentRange = ""
      AcceptRanges = ""
      ContentType = ""
      DownloadedBytes = 0
      Seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 3)
      Bin = ""
      Error = $_.Exception.Message
    }
  }
}

Write-Text "00-env.txt" @"
URL=$Url
Quality=$Quality
TargetHeight=$targetHeight
VideoFormatId=$VideoFormatId
AudioFormatId=$AudioFormatId
VideoProbeMb=$VideoProbeMb
AudioProbeMb=$AudioProbeMb
Bin=$bin
yt-dlp=$yt
ffprobe=$ffprobe
OS=$([System.Environment]::OSVersion.VersionString)
PowerShell=$($PSVersionTable.PSVersion)
"@

Add-Summary "MediaDrop DASH Range Probe"
Add-Summary "=========================="
Add-Summary "URL: $Url"
Add-Summary "Quality: $Quality"
Add-Summary "TargetHeight: $targetHeight"
Add-Summary ""

Write-Host "[1/5] yt-dlp version"
Run-Proc "01-ytdlp-version" $yt @("--version") 30 | Out-Null

Write-Host "[2/5] Fetch JSON"
$jsonPath = Join-Path $out "02-info.json"

$jsonResult = Run-Proc "02-info-json" $yt @(
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
  Write-Text "02-info-json-parse-error.txt" $_.Exception.ToString()
}

if (-not $info) {
  Add-Summary "JSON parse failed. Test stopped."

  if (Test-Path $zip) {
    Remove-Item $zip -Force
  }

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

if (-not $video -or -not $audio) {
  Add-Summary "No suitable video/audio candidate found."

  if (Test-Path $zip) {
    Remove-Item $zip -Force
  }

  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

$vf = Get-TextValue $video "format_id"
$af = Get-TextValue $audio "format_id"
$pair = "$vf+$af"

$candidateReport = @{
  video = $video | Select-Object format_id, ext, protocol, width, height, fps, vcodec, acodec, tbr, filesize, filesize_approx, fragment_base_url, fragments, init_range, index_range
  audio = $audio | Select-Object format_id, ext, protocol, abr, vcodec, acodec, tbr, filesize, filesize_approx, fragments, init_range, index_range
}

$candidateReport | ConvertTo-Json -Depth 8 | Out-File -FilePath (Join-Path $out "03-selected-candidates.json") -Encoding UTF8

Add-Summary ("Selected video: {0}, {1}x{2}, ext={3}, protocol={4}, vcodec={5}, filesize={6}, approx={7}" -f `
  $vf,
  (Get-TextValue $video "width"),
  (Get-TextValue $video "height"),
  (Get-TextValue $video "ext"),
  (Get-TextValue $video "protocol"),
  (Get-TextValue $video "vcodec"),
  (Get-TextValue $video "filesize"),
  (Get-TextValue $video "filesize_approx")
)

Add-Summary ("Selected audio: {0}, ext={1}, protocol={2}, acodec={3}, filesize={4}, approx={5}" -f `
  $af,
  (Get-TextValue $audio "ext"),
  (Get-TextValue $audio "protocol"),
  (Get-TextValue $audio "acodec"),
  (Get-TextValue $audio "filesize"),
  (Get-TextValue $audio "filesize_approx")
)

Add-Summary ""

Write-Host "[3/5] Resolve direct URLs"

$urlResult = Run-Proc "04-resolve-urls" $yt @(
  "--no-playlist",
  "--no-warnings",
  "--get-url",
  "-f", $pair,
  $Url
) 90

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
  Add-Summary "URL resolve failed for ${pair}."

  if (Test-Path $zip) {
    Remove-Item $zip -Force
  }

  Compress-Archive -Path (Join-Path $out "*") -DestinationPath $zip -Force
  Write-Host "Hazır: $zip"
  exit
}

Save-Url-Redacted "04-video-url-redacted.txt" $videoUrl
Save-Url-Redacted "04-audio-url-redacted.txt" $audioUrl

Write-Host "[4/5] Range probe video/audio"

$videoBytes = [Math]::Max(1, $VideoProbeMb) * 1024 * 1024
$audioBytes = [Math]::Max(1, $AudioProbeMb) * 1024 * 1024

$videoRange1 = Get-Range "video-first-${VideoProbeMb}mb" $videoUrl 0 ($videoBytes - 1)
$audioRange1 = Get-Range "audio-first-${AudioProbeMb}mb" $audioUrl 0 ($audioBytes - 1)

Add-Summary ("Video range: ok={0}, status={1}, bytes={2}, contentRange={3}, acceptRanges={4}, seconds={5}" -f `
  $videoRange1.Ok,
  $videoRange1.StatusCode,
  $videoRange1.DownloadedBytes,
  $videoRange1.ContentRange,
  $videoRange1.AcceptRanges,
  $videoRange1.Seconds
)

Add-Summary ("Audio range: ok={0}, status={1}, bytes={2}, contentRange={3}, acceptRanges={4}, seconds={5}" -f `
  $audioRange1.Ok,
  $audioRange1.StatusCode,
  $audioRange1.DownloadedBytes,
  $audioRange1.ContentRange,
  $audioRange1.AcceptRanges,
  $audioRange1.Seconds
)

Write-Host "[5/5] Save final report"

$report = @{
  url = $Url
  quality = $Quality
  selected_pair = $pair
  video = @{
    id = $vf
    width = Get-TextValue $video "width"
    height = Get-TextValue $video "height"
    ext = Get-TextValue $video "ext"
    protocol = Get-TextValue $video "protocol"
    vcodec = Get-TextValue $video "vcodec"
    filesize = Get-TextValue $video "filesize"
    filesize_approx = Get-TextValue $video "filesize_approx"
    range = $videoRange1
  }
  audio = @{
    id = $af
    ext = Get-TextValue $audio "ext"
    protocol = Get-TextValue $audio "protocol"
    acodec = Get-TextValue $audio "acodec"
    filesize = Get-TextValue $audio "filesize"
    filesize_approx = Get-TextValue $audio "filesize_approx"
    range = $audioRange1
  }
}

$report | ConvertTo-Json -Depth 10 | Out-File -FilePath (Join-Path $out "05-range-report.json") -Encoding UTF8

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