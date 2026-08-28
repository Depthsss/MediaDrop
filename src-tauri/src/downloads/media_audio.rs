use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use uuid::Uuid;

const FFMPEG_MUX_TIMEOUT: Duration = Duration::from_secs(180);
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(20);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STDERR_LIMIT_BYTES: usize = 32 * 1024;
const FFPROBE_STDOUT_LIMIT_BYTES: usize = 128 * 1024;
const TEMP_PATH_ATTEMPTS: usize = 8;

#[derive(Debug)]
pub(crate) enum MediaAudioError {
    InvalidArgument(&'static str),
    MissingFile {
        kind: &'static str,
        path: PathBuf,
    },
    OutputExists(PathBuf),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    CommandSpawn {
        tool: &'static str,
        source: io::Error,
    },
    CommandWait {
        tool: &'static str,
        source: io::Error,
    },
    CommandPipeRead {
        tool: &'static str,
        stream: &'static str,
        source: io::Error,
    },
    CommandPipePanicked {
        tool: &'static str,
        stream: &'static str,
    },
    CommandTimeout {
        tool: &'static str,
        timeout: Duration,
        stderr: String,
        kill_error: Option<String>,
    },
    CommandFailed {
        tool: &'static str,
        status_code: Option<i32>,
        stderr: String,
    },
    ProbeOutputTooLarge {
        limit_bytes: usize,
    },
    ProbeJson(serde_json::Error),
    ProbeSchema(&'static str),
    MissingAudioStream,
    EmptyMuxOutput(PathBuf),
    TempPathUnavailable(PathBuf),
}

impl fmt::Display for MediaAudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(message) => formatter.write_str(message),
            Self::MissingFile { kind, path } => {
                write!(formatter, "{kind} dosyası bulunamadı: {}", path.display())
            }
            Self::OutputExists(path) => write!(
                formatter,
                "Mux çıktısı zaten mevcut; üzerine yazılmadı: {}",
                path.display()
            ),
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "{operation} başarısız oldu: {} | {source}",
                path.display()
            ),
            Self::CommandSpawn { tool, source } => {
                write!(formatter, "{tool} başlatılamadı: {source}")
            }
            Self::CommandWait { tool, source } => {
                write!(formatter, "{tool} sonucu beklenemedi: {source}")
            }
            Self::CommandPipeRead {
                tool,
                stream,
                source,
            } => write!(formatter, "{tool} {stream} çıktısı okunamadı: {source}"),
            Self::CommandPipePanicked { tool, stream } => {
                write!(
                    formatter,
                    "{tool} {stream} okuyucu thread'i beklenmedik şekilde durdu"
                )
            }
            Self::CommandTimeout {
                tool,
                timeout,
                stderr,
                kill_error,
            } => {
                write!(
                    formatter,
                    "{tool} {} saniyelik zaman sınırını aştı",
                    timeout.as_secs_f64()
                )?;
                if let Some(kill_error) = kill_error {
                    write!(formatter, "; process sonlandırma hatası: {kill_error}")?;
                }
                if !stderr.is_empty() {
                    write!(formatter, ". stderr: {stderr}")?;
                }
                Ok(())
            }
            Self::CommandFailed {
                tool,
                status_code,
                stderr,
            } => {
                write!(
                    formatter,
                    "{tool} başarısız oldu (exit code: {})",
                    status_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                )?;
                if !stderr.is_empty() {
                    write!(formatter, ". stderr: {stderr}")?;
                }
                Ok(())
            }
            Self::ProbeOutputTooLarge { limit_bytes } => write!(
                formatter,
                "ffprobe JSON çıktısı güvenli {} bayt sınırını aştı",
                limit_bytes
            ),
            Self::ProbeJson(source) => write!(formatter, "ffprobe JSON parse edilemedi: {source}"),
            Self::ProbeSchema(message) => {
                write!(formatter, "ffprobe JSON şeması geçersiz: {message}")
            }
            Self::MissingAudioStream => {
                formatter.write_str("Mux çıktısında doğrulanabilir bir audio stream bulunamadı")
            }
            Self::EmptyMuxOutput(path) => write!(
                formatter,
                "ffmpeg boş veya eksik bir mux çıktısı üretti: {}",
                path.display()
            ),
            Self::TempPathUnavailable(parent) => write!(
                formatter,
                "Mux için benzersiz geçici dosya yolu üretilemedi: {}",
                parent.display()
            ),
        }
    }
}

impl std::error::Error for MediaAudioError {}

#[derive(Debug)]
struct CapturedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: CapturedBytes,
    stderr: CapturedBytes,
}

enum ProcessEnd {
    Exited(ExitStatus),
    TimedOut(Option<String>),
    WaitFailed(io::Error),
}

struct TempOutputGuard {
    path: PathBuf,
}

impl TempOutputGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempOutputGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn ffprobe_json_has_audio_stream(json: &str) -> Result<bool, MediaAudioError> {
    let clean_json = json.trim_start_matches('\u{feff}');
    let value = serde_json::from_str::<Value>(clean_json).map_err(MediaAudioError::ProbeJson)?;
    let streams = value
        .get("streams")
        .ok_or(MediaAudioError::ProbeSchema("streams alanı eksik"))?
        .as_array()
        .ok_or(MediaAudioError::ProbeSchema("streams bir dizi değil"))?;

    for stream in streams {
        let object = stream
            .as_object()
            .ok_or(MediaAudioError::ProbeSchema("stream öğesi bir nesne değil"))?;
        let Some(codec_type) = object.get("codec_type") else {
            continue;
        };
        let codec_type = codec_type
            .as_str()
            .ok_or(MediaAudioError::ProbeSchema("codec_type bir metin değil"))?;
        if codec_type.trim().eq_ignore_ascii_case("audio") {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) fn probe_media_has_audio_stream(
    ffprobe_executable: &Path,
    media_path: &Path,
) -> Result<bool, MediaAudioError> {
    ensure_existing_file("ffprobe executable", ffprobe_executable)?;
    ensure_existing_file("probe edilecek medya", media_path)?;

    let args = build_ffprobe_audio_args(media_path);
    let output = run_command_with_timeout(
        "ffprobe",
        ffprobe_executable,
        &args,
        FFPROBE_TIMEOUT,
        FFPROBE_STDOUT_LIMIT_BYTES,
    )?;
    ensure_command_success("ffprobe", &output)?;
    if output.stdout.truncated {
        return Err(MediaAudioError::ProbeOutputTooLarge {
            limit_bytes: FFPROBE_STDOUT_LIMIT_BYTES,
        });
    }

    let json = String::from_utf8_lossy(&output.stdout.bytes);
    ffprobe_json_has_audio_stream(&json)
}

pub(crate) fn mux_separate_video_audio(
    ffmpeg_executable: &Path,
    ffprobe_executable: &Path,
    video_input: &Path,
    audio_input: &Path,
    output_path: &Path,
) -> Result<PathBuf, MediaAudioError> {
    ensure_existing_file("ffmpeg executable", ffmpeg_executable)?;
    ensure_existing_file("ffprobe executable", ffprobe_executable)?;
    ensure_existing_file("video input", video_input)?;
    ensure_existing_file("audio input", audio_input)?;

    if output_path.as_os_str().is_empty() {
        return Err(MediaAudioError::InvalidArgument(
            "Mux output yolu boş olamaz",
        ));
    }
    ensure_output_absent(output_path)?;

    let output_parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !output_parent.is_dir() {
        return Err(MediaAudioError::MissingFile {
            kind: "mux output klasörü",
            path: output_parent.to_path_buf(),
        });
    }

    let temp_output = TempOutputGuard::new(unique_temp_output_path(output_path)?);
    let args = build_ffmpeg_mux_args(video_input, audio_input, temp_output.path());
    let process_output =
        run_command_with_timeout("ffmpeg", ffmpeg_executable, &args, FFMPEG_MUX_TIMEOUT, 1)?;
    ensure_command_success("ffmpeg", &process_output)?;
    ensure_non_empty_file(temp_output.path())?;

    if !probe_media_has_audio_stream(ffprobe_executable, temp_output.path())? {
        return Err(MediaAudioError::MissingAudioStream);
    }

    commit_temp_output_no_replace(temp_output.path(), output_path)?;
    Ok(output_path.to_path_buf())
}

fn build_ffmpeg_mux_args(video_input: &Path, audio_input: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-loglevel".into(),
        "error".into(),
        "-n".into(),
        "-i".into(),
        video_input.as_os_str().to_owned(),
        "-i".into(),
        audio_input.as_os_str().to_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-f".into(),
        "mp4".into(),
        output.as_os_str().to_owned(),
    ]
}

fn build_ffprobe_audio_args(input: &Path) -> Vec<OsString> {
    vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "a".into(),
        "-show_entries".into(),
        "stream=codec_type".into(),
        "-of".into(),
        "json".into(),
        input.as_os_str().to_owned(),
    ]
}

fn ensure_existing_file(kind: &'static str, path: &Path) -> Result<(), MediaAudioError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(MediaAudioError::MissingFile {
            kind,
            path: path.to_path_buf(),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            Err(MediaAudioError::MissingFile {
                kind,
                path: path.to_path_buf(),
            })
        }
        Err(source) => Err(MediaAudioError::Io {
            operation: "Dosya bilgisi okuma",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_output_absent(path: &Path) -> Result<(), MediaAudioError> {
    match path.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(MediaAudioError::OutputExists(path.to_path_buf())),
        Err(source) => Err(MediaAudioError::Io {
            operation: "Mux output yolunu kontrol etme",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_non_empty_file(path: &Path) -> Result<(), MediaAudioError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() > 0 => Ok(()),
        Ok(_) => Err(MediaAudioError::EmptyMuxOutput(path.to_path_buf())),
        Err(source) => Err(MediaAudioError::Io {
            operation: "Mux output bilgisini okuma",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn unique_temp_output_path(output_path: &Path) -> Result<PathBuf, MediaAudioError> {
    let parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let output_name = output_path
        .file_name()
        .ok_or(MediaAudioError::InvalidArgument(
            "Mux output dosya adı eksik",
        ))?;

    for _ in 0..TEMP_PATH_ATTEMPTS {
        let mut temp_name = OsString::from(".");
        temp_name.push(output_name);
        temp_name.push(format!(".mediadrop-mux-{}.tmp", Uuid::new_v4()));
        let candidate = parent.join(temp_name);
        match candidate.try_exists() {
            Ok(false) => return Ok(candidate),
            Ok(true) => continue,
            Err(source) => {
                return Err(MediaAudioError::Io {
                    operation: "Geçici mux output yolunu kontrol etme",
                    path: candidate,
                    source,
                });
            }
        }
    }

    Err(MediaAudioError::TempPathUnavailable(parent.to_path_buf()))
}

fn run_command_with_timeout(
    tool: &'static str,
    executable: &Path,
    args: &[OsString],
    timeout: Duration,
    stdout_limit: usize,
) -> Result<ProcessOutput, MediaAudioError> {
    if timeout.is_zero() {
        return Err(MediaAudioError::InvalidArgument(
            "Process timeout sıfır olamaz",
        ));
    }

    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_command_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|source| MediaAudioError::CommandSpawn { tool, source })?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(MediaAudioError::InvalidArgument(
            "Child stdout pipe oluşturulamadı",
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(MediaAudioError::InvalidArgument(
            "Child stderr pipe oluşturulamadı",
        ));
    };
    let stdout_reader = spawn_bounded_tail_reader(stdout, stdout_limit);
    let stderr_reader = spawn_bounded_tail_reader(stderr, STDERR_LIMIT_BYTES);

    let started_at = Instant::now();
    let process_end = loop {
        match child.try_wait() {
            Ok(Some(status)) => break ProcessEnd::Exited(status),
            Ok(None) if started_at.elapsed() >= timeout => {
                let kill_error = child.kill().err().map(|error| error.to_string());
                let _ = child.wait();
                break ProcessEnd::TimedOut(kill_error);
            }
            Ok(None) => thread::sleep(PROCESS_POLL_INTERVAL),
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                break ProcessEnd::WaitFailed(source);
            }
        }
    };

    let stdout_result = join_bounded_reader(stdout_reader, tool, "stdout");
    let stderr_result = join_bounded_reader(stderr_reader, tool, "stderr");

    match process_end {
        ProcessEnd::Exited(status) => Ok(ProcessOutput {
            status,
            stdout: stdout_result?,
            stderr: stderr_result?,
        }),
        ProcessEnd::TimedOut(kill_error) => {
            let stderr = stderr_result
                .map(|capture| display_capture(&capture))
                .unwrap_or_default();
            Err(MediaAudioError::CommandTimeout {
                tool,
                timeout,
                stderr,
                kill_error,
            })
        }
        ProcessEnd::WaitFailed(source) => Err(MediaAudioError::CommandWait { tool, source }),
    }
}

fn spawn_bounded_tail_reader<R>(
    reader: R,
    limit: usize,
) -> thread::JoinHandle<io::Result<CapturedBytes>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || read_bounded_tail(reader, limit))
}

fn read_bounded_tail(mut reader: impl Read, limit: usize) -> io::Result<CapturedBytes> {
    let mut bytes = Vec::with_capacity(limit.min(8 * 1024));
    let mut total_read = 0usize;
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        total_read = total_read.saturating_add(read);
        if limit == 0 {
            continue;
        }
        if read >= limit {
            bytes.clear();
            bytes.extend_from_slice(&chunk[read - limit..read]);
            continue;
        }
        let overflow = bytes.len().saturating_add(read).saturating_sub(limit);
        if overflow > 0 {
            bytes.drain(..overflow);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    Ok(CapturedBytes {
        truncated: total_read > bytes.len(),
        bytes,
    })
}

fn join_bounded_reader(
    handle: thread::JoinHandle<io::Result<CapturedBytes>>,
    tool: &'static str,
    stream: &'static str,
) -> Result<CapturedBytes, MediaAudioError> {
    match handle.join() {
        Ok(Ok(capture)) => Ok(capture),
        Ok(Err(source)) => Err(MediaAudioError::CommandPipeRead {
            tool,
            stream,
            source,
        }),
        Err(_) => Err(MediaAudioError::CommandPipePanicked { tool, stream }),
    }
}

fn ensure_command_success(
    tool: &'static str,
    output: &ProcessOutput,
) -> Result<(), MediaAudioError> {
    if output.status.success() {
        return Ok(());
    }
    Err(MediaAudioError::CommandFailed {
        tool,
        status_code: output.status.code(),
        stderr: display_capture(&output.stderr),
    })
}

fn display_capture(capture: &CapturedBytes) -> String {
    let text = String::from_utf8_lossy(&capture.bytes).trim().to_string();
    if capture.truncated && !text.is_empty() {
        format!("[yalnızca son {} bayt] {text}", capture.bytes.len())
    } else {
        text
    }
}

#[cfg(target_os = "windows")]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_command_window(_command: &mut Command) {}

#[cfg(target_os = "windows")]
fn commit_temp_output_no_replace(source: &Path, target: &Path) -> Result<(), MediaAudioError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    ensure_output_absent(target)?;
    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: Both pointers reference live, NUL-terminated UTF-16 buffers for
    // the duration of the call. MOVEFILE_REPLACE_EXISTING is intentionally
    // omitted so an output created by another process is never clobbered.
    let moved = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved != 0 {
        return Ok(());
    }

    if target.try_exists().unwrap_or(false) {
        Err(MediaAudioError::OutputExists(target.to_path_buf()))
    } else {
        Err(MediaAudioError::Io {
            operation: "Mux outputunu atomik taşıma",
            path: target.to_path_buf(),
            source: io::Error::last_os_error(),
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn commit_temp_output_no_replace(source: &Path, target: &Path) -> Result<(), MediaAudioError> {
    ensure_output_absent(target)?;
    fs::hard_link(source, target).map_err(|source| {
        if target.try_exists().unwrap_or(false) {
            MediaAudioError::OutputExists(target.to_path_buf())
        } else {
            MediaAudioError::Io {
                operation: "Mux outputunu no-replace olarak bağlama",
                path: target.to_path_buf(),
                source,
            }
        }
    })?;
    let _ = fs::remove_file(source);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("mediadrop-media-audio-{label}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn parser_finds_only_root_stream_codec_type_audio() {
        let json = r#"{
            "streams": [
                {"codec_type": "video"},
                {"codec_type": "audio", "tags": {"language": "und"}}
            ]
        }"#;
        assert!(ffprobe_json_has_audio_stream(json).unwrap());
    }

    #[test]
    fn parser_does_not_accept_nested_or_unrelated_audio_text() {
        let json = r#"{
            "streams": [
                {"codec_type": "video", "tags": {"description": "audio"}},
                {"tags": {"codec_type": "audio"}}
            ],
            "format": {"format_name": "audio"}
        }"#;
        assert!(!ffprobe_json_has_audio_stream(json).unwrap());
    }

    #[test]
    fn parser_accepts_empty_stream_array_as_no_audio() {
        assert!(!ffprobe_json_has_audio_stream(r#"{"streams": []}"#).unwrap());
    }

    #[test]
    fn parser_rejects_missing_or_wrong_stream_schema() {
        assert!(matches!(
            ffprobe_json_has_audio_stream(r#"{"format": {}}"#),
            Err(MediaAudioError::ProbeSchema(_))
        ));
        assert!(matches!(
            ffprobe_json_has_audio_stream(r#"{"streams": {}}"#),
            Err(MediaAudioError::ProbeSchema(_))
        ));
        assert!(matches!(
            ffprobe_json_has_audio_stream(r#"{"streams": ["audio"]}"#),
            Err(MediaAudioError::ProbeSchema(_))
        ));
    }

    #[test]
    fn parser_rejects_malformed_json() {
        assert!(matches!(
            ffprobe_json_has_audio_stream("not-json"),
            Err(MediaAudioError::ProbeJson(_))
        ));
    }

    #[test]
    fn mux_argument_policy_maps_video_and_audio_without_clobbering() {
        let args = strings(build_ffmpeg_mux_args(
            Path::new("video input.mp4"),
            Path::new("audio input.m4a"),
            Path::new("temporary output.tmp"),
        ));
        assert_eq!(
            args,
            vec![
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-n",
                "-i",
                "video input.mp4",
                "-i",
                "audio input.m4a",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-shortest",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                "temporary output.tmp",
            ]
        );
        assert!(!args.iter().any(|arg| arg == "-y"));
    }

    #[test]
    fn ffprobe_argument_policy_requests_only_audio_codec_type_json() {
        assert_eq!(
            strings(build_ffprobe_audio_args(Path::new("mux output.mp4"))),
            vec![
                "-v",
                "error",
                "-select_streams",
                "a",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "json",
                "mux output.mp4",
            ]
        );
    }

    #[test]
    fn bounded_reader_keeps_only_the_tail_and_marks_truncation() {
        let capture = read_bounded_tail(Cursor::new(b"0123456789"), 4).unwrap();
        assert_eq!(capture.bytes, b"6789");
        assert!(capture.truncated);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn bundled_ffmpeg_muxes_real_video_and_audio_and_ffprobe_confirms_stream() {
        let binaries = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let ffmpeg = binaries.join("ffmpeg-x86_64-pc-windows-msvc.exe");
        let ffprobe = binaries.join("ffprobe-x86_64-pc-windows-msvc.exe");
        assert!(ffmpeg.is_file(), "bundled ffmpeg is missing");
        assert!(ffprobe.is_file(), "bundled ffprobe is missing");

        let dir = TestDir::new("real-mux");
        let video = dir.path().join("video-only.mp4");
        let audio = dir.path().join("audio-only.m4a");
        let output = dir.path().join("muxed.mp4");

        let video_result = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=32x32:d=0.25",
                "-an",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&video)
            .output()
            .unwrap();
        assert!(
            video_result.status.success(),
            "video fixture generation failed: {}",
            String::from_utf8_lossy(&video_result.stderr)
        );

        let audio_result = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:duration=0.25",
                "-vn",
                "-c:a",
                "aac",
                "-b:a",
                "64k",
            ])
            .arg(&audio)
            .output()
            .unwrap();
        assert!(
            audio_result.status.success(),
            "audio fixture generation failed: {}",
            String::from_utf8_lossy(&audio_result.stderr)
        );

        let muxed = mux_separate_video_audio(&ffmpeg, &ffprobe, &video, &audio, &output).unwrap();
        assert_eq!(muxed, output);
        assert!(fs::metadata(&output).unwrap().len() > 0);
        assert!(probe_media_has_audio_stream(&ffprobe, &output).unwrap());
    }
}
