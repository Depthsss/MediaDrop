use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use base64::{engine::general_purpose, Engine as _};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use tauri::{Emitter, Manager};

mod core;
mod companion;
mod downloads;
mod infra;
mod instagram;
mod media_cache;
mod platform;
mod util;
use core::error::{ApiError, ApiResult, STRUCTURED_ERROR_PREFIX};
pub(crate) use core::media::{
    AuthorIdentity, CanonicalInstagramIdentity, InstagramAuthorDiagnostics, MediaAnalysis,
    MediaDownloadFailure, MediaDownloadFile, MediaDownloadResult, MediaItem, TwitterPostMetadata,
    TwitterQuoteContext,
};
use downloads::job_manager::{
    begin_download_job, begin_download_job_owned, cleanup_stale_download_job_dirs,
    current_download_job_id, current_download_job_stop, download_job_snapshot_for,
    download_job_result_target_for, ensure_download_job_history_owner, ensure_download_job_owner,
    record_download_job_operation, record_download_job_result, record_download_job_terminal,
    request_download_job_stop, update_download_job_progress, DownloadJobGuard, DownloadJobStop,
    DownloadResultKind,
};
use downloads::media_audio::{mux_separate_video_audio, probe_media_has_audio_stream};
use infra::temp_artifact::{cleanup_owned_temp_artifacts, TempArtifact};
use infra::http_client::{
    cloud_report_client, http_range_client, instagram_avatar_client, media_client,
    twitter_avatar_client, twitter_profile_client,
};
use infra::config::{
    atomic_replace_config_file, mediadrop_config_dir, mediadrop_config_io,
    try_read_mediadrop_config_unlocked, update_mediadrop_config,
};
#[cfg(test)]
use instagram::parser::find_instagram_avatar_url;
use instagram::parser::{
    collect_gallery_items_from_value, extension_from_url, gallery_stdout_to_inventory,
    gallery_stdout_to_items, json_text, json_u32, media_content_kind, non_empty_string,
    propagate_media_item_metadata,
    supported_image_extension, supported_video_extension,
};
use instagram::story::{
    apply_story_policy, canonical_owner_story_items, instagram_highlight_url,
    instagram_story_profile_url, instagram_story_request, resolve_instagram_share_story_target,
    InstagramStoryRequest,
};
#[cfg(test)]
use media_cache::preview::validate_preview_url_with_resolver;
use media_cache::preview::{
    cached_media_preview_path, prepare_media_preview,
    validate_audio_preview_magic, validate_declared_audio_type, validate_declared_media_type,
    validate_preview_magic, validate_preview_url, SafePreviewDnsResolver,
};
use media_cache::registry::{
    begin_registered_media_refresh, media_error_allows_registry_refresh, register_media_analysis,
    registered_media_analysis, replace_registered_media_analysis, require_media_registry_identity,
    RegisteredMediaAnalysis, MEDIA_ANALYSIS_TTL_MS,
};
#[cfg(test)]
use media_cache::registry::claim_registry_refresh;
use platform::windows::{
    cancel_download, close_window, get_window_position, minimize_window, pause_download,
    resize_window_height, reveal_download, reveal_file_in_explorer, reveal_path, set_window_position,
    show_download_complete_notification, show_main_window, start_dragging,
};
pub(crate) use util::url::{
    host_matches, is_instagram_url, is_supported_media_url, is_tiktok_url, is_twitter_url,
    is_youtube_url, unsupported_media_link_message,
};

const FFMPEG_DIR: &str = r"C:\ffmpeg";
const TARGET_TRIPLE: &str = "x86_64-pc-windows-msvc";
const MEDIADROP_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const MEDIADROP_IDENTIFIER: &str = "com.mab.mediadrop";
const EXTENSION_SETUP_LAUNCH_ARG: &str = "--extension-setup";
const CLOUD_REPORT_ENDPOINT: &str =
    "https://mediadrop-reports-cloud.mediadrop-reports.workers.dev/api/report";
const DOWNLOAD_STALL_NOTICE_AFTER: Duration = Duration::from_secs(90);
const FULL_DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const FULL_POSTPROCESS_STALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CLIP_DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const CLIP_POSTPROCESS_STALL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const DIAGNOSTIC_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const WATCHDOG_FILE_SCAN_INTERVAL: Duration = Duration::from_millis(1500);
const THUMBNAIL_TEMP_PREFIX: &str = "mediadrop-thumb-";
const MAX_MEDIA_DOWNLOAD_BYTES: usize = 300 * 1024 * 1024;
const MEDIA_MAGIC_PREFIX_BYTES: usize = 64;
const MEDIA_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_YTDLP_STDERR_BYTES: usize = 256 * 1024;
const MAX_MEDIA_PREVIEW_REDIRECTS: usize = 4;
const YTDLP_STREAM_RESOLVE_TIMEOUT: Duration = Duration::from_secs(12);
const YOUTUBE_PREVIEW_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
const YOUTUBE_PREVIEW_RESOLVE_BUDGET: Duration = Duration::from_secs(5);
const YOUTUBE_ANALYSIS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const YOUTUBE_ANALYSIS_CACHE_CAPACITY: usize = 8;
const MAX_YOUTUBE_ANALYSIS_CACHE_BYTES: usize = 4 * 1024 * 1024;
const MAX_INSTAGRAM_PUBLIC_HTML_BYTES: usize = 4 * 1024 * 1024;
const MAX_INSTAGRAM_AVATAR_BYTES: usize = 1024 * 1024;
const MAX_INSTAGRAM_AVATAR_REDIRECTS: usize = 3;
const INSTAGRAM_AVATAR_CACHE_TTL_MS: u128 = 6 * 60 * 60 * 1000;
const INSTAGRAM_AVATAR_CACHE_CAPACITY: usize = 128;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const PAUSED_SIGNAL: &str = "__MEDIADROP_PAUSED__";
const CANCELLED_SIGNAL: &str = "__MEDIADROP_CANCELLED__";
const YTDLP_FINAL_PATH_MARKER: &str = "__MEDIADROP_FINAL_PATH__";
const YTDLP_PROGRESS_MARKER: &str = "__MEDIADROP_PROGRESS__";
const YTDLP_PROGRESS_TEMPLATE: &str = "download:__MEDIADROP_PROGRESS__\t%(progress.status)s\t%(progress.downloaded_bytes)s\t%(progress.total_bytes)s\t%(progress.total_bytes_estimate)s\t%(progress.speed)s";
const YTDLP_PHASE_DOWNLOAD: u64 = 0;
const YTDLP_PHASE_POSTPROCESS: u64 = 1;
const TWITTER_POST_TEMP_DIR_PREFIX: &str = "twitter-post-";
const MAX_TWITTER_POST_CARD_PNG_BYTES: usize = 5 * 1024 * 1024;
const MAX_TWITTER_AVATAR_BYTES: usize = 1024 * 1024;
const MAX_TWITTER_AVATAR_REDIRECTS: usize = 3;
const MAX_TWITTER_PROFILE_HTML_BYTES: usize = 2 * 1024 * 1024;
const MIN_TWITTER_POST_OUTPUT_DIMENSION: u32 = 320;
const MAX_TWITTER_POST_OUTPUT_WIDTH: u32 = 2160;
const MAX_TWITTER_POST_OUTPUT_HEIGHT: u32 = 4096;
const MIN_TWITTER_POST_VIDEO_SLOT_DIMENSION: u32 = 64;
const PREPARED_INSTAGRAM_COOKIE_TTL_MS: u128 = 6 * 60 * 60 * 1000;

static EXTENSION_SETUP_PENDING: AtomicBool = AtomicBool::new(false);

fn extension_setup_requested(args: &[String]) -> bool {
    args.iter().any(|arg| arg == EXTENSION_SETUP_LAUNCH_ARG)
}

fn queue_extension_setup_request() {
    EXTENSION_SETUP_PENDING.store(true, Ordering::Release);
}

#[tauri::command]
fn take_extension_setup_request() -> bool {
    EXTENSION_SETUP_PENDING.swap(false, Ordering::AcqRel)
}

#[cfg(target_os = "windows")]
const DOWNLOAD_NOTIFICATION_ICON_BYTES: &[u8] = include_bytes!("../icons/Square150x150Logo.png");

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwitterPostCardLayout {
    output_width: u32,
    output_height: u32,
    video_x: u32,
    video_y: u32,
    video_width: u32,
    video_height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DownloadStopRequest {
    Pause,
    Cancel,
}

#[derive(Clone, Debug)]
struct ActiveDownloadProcess {
    pid: u32,
    output_dir: PathBuf,
    started_at_ms: u128,
    request: Option<DownloadStopRequest>,
}

#[derive(Default)]
struct DownloadControlState {
    active: Option<ActiveDownloadProcess>,
    paused: Option<ActiveDownloadProcess>,
    stop_generation: u64,
    last_stop_request: Option<DownloadStopRequest>,
}

static DOWNLOAD_CONTROL: OnceLock<Arc<Mutex<DownloadControlState>>> = OnceLock::new();

fn download_control() -> &'static Arc<Mutex<DownloadControlState>> {
    DOWNLOAD_CONTROL.get_or_init(|| Arc::new(Mutex::new(DownloadControlState::default())))
}

fn current_download_stop_generation() -> u64 {
    download_control()
        .lock()
        .map(|state| state.stop_generation)
        .unwrap_or(0)
}

fn check_download_stop_since(generation: u64) -> Result<(), String> {
    match current_download_job_stop() {
        Some(DownloadJobStop::Pause) => return Err(PAUSED_SIGNAL.to_string()),
        Some(DownloadJobStop::Cancel) => return Err(CANCELLED_SIGNAL.to_string()),
        None => {}
    }

    let state = download_control()
        .lock()
        .map_err(|_| "İndirme kontrol kilidi alınamadı.".to_string())?;

    if state.stop_generation == generation {
        return Ok(());
    }

    match state.last_stop_request {
        Some(DownloadStopRequest::Pause) => Err(PAUSED_SIGNAL.to_string()),
        Some(DownloadStopRequest::Cancel) => Err(CANCELLED_SIGNAL.to_string()),
        None => Ok(()),
    }
}

#[derive(Clone, serde::Serialize)]
struct DownloadProgress {
    #[serde(rename = "jobId")]
    job_id: Option<String>,
    percent: Option<f64>,
    downloaded_mb: Option<f64>,
    total_mb: Option<f64>,
    speed_mb: Option<f64>,
    phase: String,
    line: String,
}

#[derive(Clone, serde::Serialize)]
struct DownloadResult {
    message: String,
    file_path: String,
    output_dir: String,
    mode: String,
    file_size: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CookieBrowserInfo {
    id: String,
    label: String,
    installed: bool,
    recommended: bool,
    default_browser: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionSetupInfo {
    extension_path: String,
    browsers: Vec<CookieBrowserInfo>,
    connected: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstagramCookieState {
    has_saved_cookies: bool,
    status: InstagramCookieStatus,
    error: Option<String>,
    browser_id: Option<String>,
    label: Option<String>,
    updated_at_ms: Option<u128>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum InstagramCookieStatus {
    Missing,
    Ready,
    Expired,
    Invalid,
    #[allow(dead_code)]
    BrowserLocked,
}

#[derive(Clone)]
struct CachedInstagramAvatar {
    data_url: String,
    cached_at_ms: u128,
    host_class: String,
    http_status: Option<u16>,
}

static INSTAGRAM_AVATAR_CACHE: OnceLock<Arc<Mutex<HashMap<String, CachedInstagramAvatar>>>> =
    OnceLock::new();

fn instagram_avatar_cache() -> &'static Arc<Mutex<HashMap<String, CachedInstagramAvatar>>> {
    INSTAGRAM_AVATAR_CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn refresh_registered_media_analysis(
    app: &tauri::AppHandle,
    analysis_id: &str,
) -> Result<RegisteredMediaAnalysis, String> {
    let clean_id = analysis_id.trim();
    let (source_url, auth_mode) = begin_registered_media_refresh(clean_id)?;
    let mut refreshed = collect_media(app, &source_url, auth_mode.as_deref())?;
    refreshed.analysis_id = clean_id.to_string();
    refreshed.expires_at_ms = now_ms().saturating_add(MEDIA_ANALYSIS_TTL_MS);

    replace_registered_media_analysis(clean_id, refreshed)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CookieBrowserRuntimeState {
    browser_id: String,
    label: String,
    installed: bool,
    running: bool,
    process_count: usize,
    relaunch_supported: bool,
    executable_path: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepareInstagramCookieAuthResult {
    auth_mode: String,
    browser_id: String,
    label: String,
    saved: bool,
    restarted: bool,
    relaunched: bool,
    relaunch_error: Option<String>,
    cookie_count: usize,
    message: String,
}

#[derive(Clone, Debug)]
struct BrowserCookieJar {
    browser_id: String,
    browser_label: String,
    profile_label: String,
    cookies: Vec<NetscapeCookie>,
    score: u32,
    failed_decrypts: usize,
}

#[derive(Clone, Debug)]
struct NetscapeCookie {
    domain: String,
    include_subdomains: bool,
    path: String,
    secure: bool,
    expires: i64,
    name: String,
    value: String,
}

#[derive(Clone, Debug)]
struct PreparedInstagramCookie {
    text: String,
    browser_id: String,
    created_at_ms: u128,
}

#[derive(Clone, Debug)]
struct PreparedYtdlpCookie {
    text: String,
    browser_id: String,
}

struct CachedYoutubeAnalysis {
    json: Arc<str>,
    created_at: Instant,
    last_accessed_at: Instant,
}

static PREPARED_INSTAGRAM_COOKIES: OnceLock<Arc<Mutex<HashMap<String, PreparedInstagramCookie>>>> =
    OnceLock::new();
static PREPARED_YTDLP_COOKIES: OnceLock<
    Arc<Mutex<HashMap<String, PreparedYtdlpCookie>>>,
> = OnceLock::new();
static CACHED_YOUTUBE_ANALYSES: OnceLock<Mutex<HashMap<String, CachedYoutubeAnalysis>>> =
    OnceLock::new();

fn prepared_instagram_cookies() -> &'static Arc<Mutex<HashMap<String, PreparedInstagramCookie>>> {
    PREPARED_INSTAGRAM_COOKIES.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn prepared_ytdlp_cookies() -> &'static Arc<Mutex<HashMap<String, PreparedYtdlpCookie>>> {
    PREPARED_YTDLP_COOKIES.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn cached_youtube_analyses() -> &'static Mutex<HashMap<String, CachedYoutubeAnalysis>> {
    CACHED_YOUTUBE_ANALYSES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn youtube_analysis_cache_entry_is_fresh(age: Duration) -> bool {
    age < YOUTUBE_ANALYSIS_CACHE_TTL
}

fn youtube_analysis_is_cacheable(value: &serde_json::Value) -> bool {
    let live_flag = ["is_live", "was_live", "is_upcoming"]
        .into_iter()
        .any(|key| value.get(key).and_then(|item| item.as_bool()) == Some(true));
    let live_status = value
        .get("live_status")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|status| !status.is_empty());
    let format_is_live = value
        .get("formats")
        .and_then(|item| item.as_array())
        .is_some_and(|formats| {
            formats.iter().any(|format| {
                format.get("is_live").and_then(|item| item.as_bool()) == Some(true)
            })
        });

    !live_flag
        && !format_is_live
        && live_status.is_none_or(|status| status.eq_ignore_ascii_case("not_live"))
}

fn cache_youtube_analysis(url: &str, info_json: &str) -> Result<(), String> {
    let clean_url = url.trim();
    if !is_youtube_url(clean_url) {
        return Err("YouTube analiz cache anahtarı geçersiz.".to_string());
    }
    if info_json.len() > MAX_YOUTUBE_ANALYSIS_CACHE_BYTES {
        return Err("YouTube analiz verisi cache sınırını aşıyor.".to_string());
    }

    let value = serde_json::from_str::<serde_json::Value>(info_json)
        .map_err(|err| format!("YouTube analiz cache JSON'u geçersiz: {}", err))?;
    let webpage_url = value
        .get("webpage_url")
        .or_else(|| value.get("original_url"))
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if !is_youtube_url(webpage_url) {
        return Err("YouTube analiz cache kaynağı doğrulanamadı.".to_string());
    }
    if !ytdlp_json_has_downloadable_video(&value) {
        return Err("YouTube analiz cache verisinde indirilebilir video yok.".to_string());
    }
    if !youtube_analysis_is_cacheable(&value) {
        invalidate_youtube_analysis(clean_url);
        return Ok(());
    }

    let current = Instant::now();
    let mut cache = cached_youtube_analyses()
        .lock()
        .map_err(|_| "YouTube analiz cache kilidi alınamadı.".to_string())?;
    cache.retain(|_, entry| youtube_analysis_cache_entry_is_fresh(entry.created_at.elapsed()));

    if cache.len() >= YOUTUBE_ANALYSIS_CACHE_CAPACITY && !cache.contains_key(clean_url) {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed_at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
    }

    cache.insert(
        clean_url.to_string(),
        CachedYoutubeAnalysis {
            json: Arc::from(info_json),
            created_at: current,
            last_accessed_at: current,
        },
    );
    Ok(())
}

fn cached_youtube_analysis(url: &str) -> Option<Arc<str>> {
    let clean_url = url.trim();
    let mut cache = cached_youtube_analyses().lock().ok()?;
    let fresh = cache
        .get(clean_url)
        .map(|entry| youtube_analysis_cache_entry_is_fresh(entry.created_at.elapsed()))
        .unwrap_or(false);

    if !fresh {
        cache.remove(clean_url);
        return None;
    }

    cache.get_mut(clean_url).map(|entry| {
        entry.last_accessed_at = Instant::now();
        Arc::clone(&entry.json)
    })
}

fn invalidate_youtube_analysis(url: &str) {
    if let Ok(mut cache) = cached_youtube_analyses().lock() {
        cache.remove(url.trim());
    }
}

fn invalidate_all_youtube_analyses() {
    if let Ok(mut cache) = cached_youtube_analyses().lock() {
        cache.clear();
    }
}

fn add_ytdlp_media_source(
    command: &mut Command,
    url: &str,
) -> Result<Option<TempArtifact>, String> {
    if let Some(info_json) = cached_youtube_analysis(url) {
        let artifact = TempArtifact::write(
            &std::env::temp_dir(),
            "mediadrop-youtube-info-",
            ".json",
            info_json.as_bytes(),
        )?;
        command.arg("--load-info-json").arg(artifact.path());
        return Ok(Some(artifact));
    }

    command.arg(url);
    Ok(None)
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct WindowPosition {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

fn clamp_window_axis(value: i32, area_start: i32, area_size: i32, window_size: i32) -> i32 {
    if area_size <= 0 {
        return value;
    }

    if window_size >= area_size {
        return area_start;
    }

    value.clamp(area_start, area_start + area_size - window_size)
}

fn safe_window_position(window: &tauri::Window, x: i32, y: i32) -> WindowPosition {
    let Ok(monitors) = window.available_monitors() else {
        return WindowPosition { x, y };
    };

    if monitors.is_empty() {
        return WindowPosition { x, y };
    }

    let size = window.outer_size().ok();
    let window_width = size
        .map(|value| value.width as i32)
        .filter(|value| *value > 0)
        .unwrap_or(920);
    let window_height = size
        .map(|value| value.height as i32)
        .filter(|value| *value > 0)
        .unwrap_or(650);
    let center_x = x as f64 + window_width as f64 / 2.0;
    let center_y = y as f64 + window_height as f64 / 2.0;

    let monitor = window
        .monitor_from_point(center_x, center_y)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| monitors.first().cloned());

    let Some(monitor) = monitor else {
        return WindowPosition { x, y };
    };

    let work_area = monitor.work_area();
    let area_x = work_area.position.x;
    let area_y = work_area.position.y;
    let area_width = work_area.size.width as i32;
    let area_height = work_area.size.height as i32;

    WindowPosition {
        x: clamp_window_axis(x, area_x, area_width, window_width),
        y: clamp_window_axis(y, area_y, area_height, window_height),
    }
}

fn keep_window_visible(window: &tauri::Window) -> Result<(), String> {
    let position = window
        .outer_position()
        .map_err(|err| format!("Pencere konumu okunamadı: {}", err))?;
    let safe = safe_window_position(window, position.x, position.y);

    if safe.x == position.x && safe.y == position.y {
        return Ok(());
    }

    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: safe.x,
            y: safe.y,
        }))
        .map_err(|err| format!("Pencere konumu ayarlanamadı: {}", err))
}

fn hls_fallback_error(code: &str, message: String) -> String {
    let payload = json!({
        "code": code,
        "message": message,
        "fallback_offer": {
            "kind": "hls_1080",
            "quality": "1080p",
            "label": "1080p HLS klip indir"
        }
    });

    format!(
        "{}{}",
        STRUCTURED_ERROR_PREFIX,
        serde_json::to_string(&payload).unwrap_or_else(|_| {
            "{\"code\":\"true_quality_failed\",\"message\":\"4K/2K klip indirilemedi.\"}"
                .to_string()
        })
    )
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewStreamResult {
    url: String,
    urls: Vec<String>,
    audio_url: Option<String>,
    mode: String,
    format: String,
}

#[derive(Clone, serde::Serialize)]
struct ToolsUpdateResult {
    checked: bool,
    updated: bool,
    message: String,
}

struct ProgressMetrics {
    percent: Option<f64>,
    downloaded_mb: Option<f64>,
    total_mb: Option<f64>,
    speed_mb: Option<f64>,
}

struct RuntimeTools {
    yt_dlp: PathBuf,
    aria2c: PathBuf,
    ffmpeg_dir: PathBuf,
}

fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut command = Command::new(program);

    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

fn ytdlp_command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut command = hidden_command(program);
    command.env("PYTHONIOENCODING", "utf-8:replace");
    command.env("PYTHONUTF8", "1");
    command.arg("--ignore-config").arg("--no-plugin-dirs");
    command
}

fn append_bounded_text(buffer: &mut String, line: &str, limit: usize) {
    if limit == 0 {
        buffer.clear();
        return;
    }

    let line_bytes = line.len().saturating_add(1);
    if line_bytes >= limit {
        buffer.clear();
        let mut start = line.len().saturating_sub(limit.saturating_sub(1));
        while start < line.len() && !line.is_char_boundary(start) {
            start += 1;
        }
        buffer.push_str(&line[start..]);
        buffer.push('\n');
        return;
    }

    let required = buffer.len().saturating_add(line_bytes);
    if required > limit {
        let mut drain_end = (required - limit).max(limit / 4).min(buffer.len());
        while drain_end < buffer.len() && !buffer.is_char_boundary(drain_end) {
            drain_end += 1;
        }
        buffer.drain(..drain_end);
    }
    buffer.push_str(line);
    buffer.push('\n');
}

fn read_process_lines_lossy<R, F>(pipe: R, mut on_line: F)
where
    R: Read,
    F: FnMut(String),
{
    let mut reader = BufReader::new(pipe);
    let mut buffer = Vec::new();

    loop {
        buffer.clear();

        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) => break,
            Ok(_) => {
                while matches!(buffer.last(), Some(byte) if *byte == b'\n' || *byte == b'\r') {
                    buffer.pop();
                }

                on_line(String::from_utf8_lossy(&buffer).to_string());
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
}

struct TimedCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn capture_command_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Result<TimedCommandOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("Komut başlatılamadı: {}", err))?;

    let pid = child.id();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_handle = stdout.map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let stderr_handle = stderr.map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Komut zaman aşımına uğradı ve durduruldu. Timeout: {} sn",
                        timeout.as_secs()
                    ));
                }

                thread::sleep(Duration::from_millis(120));
            }
            Err(err) => {
                let _ = kill_process_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Komut durumu okunamadı: {}", err));
            }
        }
    };

    let stdout = stdout_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    Ok(TimedCommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn kill_process_tree(pid: u32) -> Result<(), String> {
    let status = hidden_command("taskkill.exe")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F")
        .status()
        .map_err(|err| format!("İndirme işlemi durdurulamadı: {}", err))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill başarısız oldu. Status: {}", status))
    }
}

fn mark_active_process(pid: u32, output_dir: &Path, started_at_ms: u128) {
    if let Ok(mut state) = download_control().lock() {
        state.active = Some(ActiveDownloadProcess {
            pid,
            output_dir: output_dir.to_path_buf(),
            started_at_ms,
            request: None,
        });
        state.paused = None;
    }
}

fn mark_active_range(output_dir: &Path, started_at_ms: u128) {
    mark_active_process(0, output_dir, started_at_ms);
}

fn request_active_process_stop(
    request: DownloadStopRequest,
    expected_job_id: Option<&str>,
) -> Result<(), String> {
    if current_download_job_id().is_some() {
        let expected_job_id = expected_job_id
            .map(str::trim)
            .filter(|job_id| !job_id.is_empty())
            .ok_or_else(|| {
                structured_backend_error(
                    "download_job_mismatch",
                    "Duraklatma veya iptal isteği etkin indirme kimliğini içermiyor.",
                )
            })?;
        let manager_request = match request {
            DownloadStopRequest::Pause => DownloadJobStop::Pause,
            DownloadStopRequest::Cancel => DownloadJobStop::Cancel,
        };
        request_download_job_stop(expected_job_id, manager_request)?;
    }

    let maybe_pid = {
        let mut state = download_control()
            .lock()
            .map_err(|_| "İndirme kontrol kilidi alınamadı.".to_string())?;

        state.stop_generation = state.stop_generation.wrapping_add(1);
        state.last_stop_request = Some(request);

        if let Some(active) = state.active.as_mut() {
            active.request = Some(request);
            Some(active.pid)
        } else {
            if request == DownloadStopRequest::Cancel {
                if let Some(paused) = state.paused.take() {
                    cleanup_incomplete_downloads(&paused.output_dir, paused.started_at_ms);
                }
            }
            None
        }
    };

    if let Some(pid) = maybe_pid {
        if pid == 0 {
            Ok(())
        } else {
            kill_process_tree(pid)
        }
    } else {
        Ok(())
    }
}

fn take_finished_process_request(pid: u32) -> Option<ActiveDownloadProcess> {
    let mut state = download_control().lock().ok()?;

    let should_take = state
        .active
        .as_ref()
        .map(|active| active.pid == pid)
        .unwrap_or(false);

    if !should_take {
        return None;
    }

    let active = state.active.take()?;

    if active.request == Some(DownloadStopRequest::Pause) {
        let mut paused = active.clone();
        paused.request = None;
        state.paused = Some(paused);
    }

    Some(active)
}

fn check_active_range_stop(output_dir: &Path, output_path: &Path) -> Result<(), String> {
    let active = {
        let mut state = download_control()
            .lock()
            .map_err(|_| "İndirme kontrol kilidi alınamadı.".to_string())?;

        let should_take = state
            .active
            .as_ref()
            .map(|active| active.pid == 0 && active.request.is_some())
            .unwrap_or(false);

        if should_take {
            state.active.take()
        } else {
            None
        }
    };

    let Some(active) = active else {
        return Ok(());
    };

    let _ = fs::remove_file(output_path);

    match active.request {
        Some(DownloadStopRequest::Pause) => {
            if let Ok(mut state) = download_control().lock() {
                let mut paused = active.clone();
                paused.request = None;
                state.paused = Some(paused);
            }
            Err(PAUSED_SIGNAL.to_string())
        }
        Some(DownloadStopRequest::Cancel) => {
            cleanup_incomplete_downloads(output_dir, active.started_at_ms);
            Err(CANCELLED_SIGNAL.to_string())
        }
        None => Ok(()),
    }
}

fn finish_active_range() {
    let _ = take_finished_process_request(0);
}

fn is_recent_file(path: &Path, started_at_ms: u128) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    let Ok(modified) = metadata.modified() else {
        return false;
    };

    let modified_ms = modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    modified_ms + 5000 >= started_at_ms
}

fn is_mediadrop_internal_temp_artifact(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    let lower = name.to_lowercase();

    lower.starts_with("mediadrop-temp-")
        || lower.starts_with("md-hls-")
        || lower.starts_with("true-quality-range-")
}

fn is_mediadrop_job_output_artifact(path: &Path, started_at_ms: u128) -> bool {
    path.is_file()
        && is_recent_file(path, started_at_ms)
        && is_mediadrop_internal_temp_artifact(path)
}

fn cleanup_incomplete_downloads(output_dir: &Path, started_at_ms: u128) {
    let Ok(entries) = fs::read_dir(output_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !is_mediadrop_job_output_artifact(&path, started_at_ms) {
            continue;
        }

        let _ = fs::remove_file(&path);
    }
}

struct JobOutputActivityTracker {
    output_dir: PathBuf,
    started_at_ms: u128,
    tracked_paths: Vec<PathBuf>,
    last_scan_ms: u64,
}

impl JobOutputActivityTracker {
    fn new(output_dir: &Path, started_at_ms: u128, now_ms: u64) -> Self {
        let mut tracker = Self {
            output_dir: output_dir.to_path_buf(),
            started_at_ms,
            tracked_paths: Vec::new(),
            last_scan_ms: 0,
        };

        tracker.discover(now_ms);
        tracker
    }

    fn current_total_size(&mut self, now_ms: u64) -> u64 {
        if Duration::from_millis(now_ms.saturating_sub(self.last_scan_ms))
            >= WATCHDOG_FILE_SCAN_INTERVAL
        {
            self.discover(now_ms);
        }

        let mut total = 0u64;

        self.tracked_paths.retain(|path| {
            if !path.is_file() {
                return false;
            }

            total = total.saturating_add(file_size(path).unwrap_or(0));
            true
        });

        total
    }

    fn discover(&mut self, now_ms: u64) {
        self.last_scan_ms = now_ms;

        let Ok(entries) = fs::read_dir(&self.output_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if !is_mediadrop_job_output_artifact(&path, self.started_at_ms) {
                continue;
            }

            if self.tracked_paths.iter().any(|known| known == &path) {
                continue;
            }

            self.tracked_paths.push(path);
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => a == b,
    }
}

fn remove_owned_temp_dir(path: &Path, expected_parent: &Path, expected_prefix: &str) {
    if expected_prefix.is_empty() || !path.is_dir() {
        return;
    }

    let Some(parent) = path.parent() else {
        return;
    };

    if !same_path(parent, expected_parent) {
        return;
    }

    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return;
    };

    if !name.starts_with(expected_prefix) {
        return;
    }

    let _ = fs::remove_dir_all(path);
}

fn file_size(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|meta| meta.len())
}

fn command_first_line_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let mut command = hidden_command(program);
    command.args(args);

    let output = capture_command_with_timeout(command, timeout)?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let first_line = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .ok_or_else(|| "Komut anlamli cikti uretmedi.".to_string())?;

    if output.status.success() {
        Ok(first_line)
    } else {
        Err(first_line)
    }
}

fn dotted_version_parts(value: &str) -> Option<Vec<u32>> {
    let parts = value
        .trim()
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

fn compare_dotted_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left_parts = dotted_version_parts(left)?;
    let right_parts = dotted_version_parts(right)?;
    let max_len = left_parts.len().max(right_parts.len());

    for index in 0..max_len {
        let left = *left_parts.get(index).unwrap_or(&0);
        let right = *right_parts.get(index).unwrap_or(&0);

        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            ordering => return Some(ordering),
        }
    }

    Some(std::cmp::Ordering::Equal)
}

fn runtime_tool_health_args(tool: &str) -> Option<&'static [&'static str]> {
    match tool {
        "ffmpeg" | "ffprobe" => Some(&["-version"]),
        "aria2c" | "deno" | "gallery-dl" | "yt-dlp" => Some(&["--version"]),
        _ => None,
    }
}

fn runtime_tool_is_healthy(tool: &str, path: &Path) -> bool {
    runtime_tool_file_is_valid(path)
        && runtime_tool_health_args(tool).is_none_or(|args| {
            command_first_line_with_timeout(path, args, DIAGNOSTIC_COMMAND_TIMEOUT).is_ok()
        })
}

fn should_copy_runtime_tool(tool: &str, source: &Path, dest: &Path) -> bool {
    if !dest.exists() {
        return true;
    }

    if tool == "instaloader-helper" {
        let source_modified = fs::metadata(source).and_then(|meta| meta.modified()).ok();
        let dest_modified = fs::metadata(dest).and_then(|meta| meta.modified()).ok();
        if source_modified.is_some() && source_modified > dest_modified {
            return true;
        }
    }

    if let Some(args) = runtime_tool_health_args(tool) {
        let source_version =
            command_first_line_with_timeout(source, args, DIAGNOSTIC_COMMAND_TIMEOUT).ok();
        let dest_version =
            command_first_line_with_timeout(dest, args, DIAGNOSTIC_COMMAND_TIMEOUT).ok();

        match (source_version, dest_version) {
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            (Some(source_version), Some(dest_version)) if tool == "yt-dlp" => {
                return compare_dotted_versions(&source_version, &dest_version)
                    .map(|ordering| ordering == std::cmp::Ordering::Greater)
                    .unwrap_or(true);
            }
            _ => {}
        }
    }

    if file_size(source) == file_size(dest) {
        return false;
    }

    true
}

fn rename_with_retries(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    let delays = [
        Duration::from_millis(60),
        Duration::from_millis(180),
        Duration::from_millis(360),
    ];
    let mut last_error = None;

    for delay in delays {
        match fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                thread::sleep(delay);
            }
        }
    }

    fs::rename(source, target).map_err(|err| last_error.unwrap_or(err))
}

fn prepend_path(command: &mut Command, dir: &Path) {
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = if old_path.is_empty() {
        dir.to_string_lossy().to_string()
    } else {
        format!("{};{}", dir.to_string_lossy(), old_path)
    };

    command.env("PATH", new_path);
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
fn get_twitter_post_mp4_template() -> ApiResult<String> {
    let template = include_str!("../../src/templates/twitter-post-mp4-template.html");

    if template.trim().is_empty() {
        return Err(ApiError::new(
            "template_empty",
            "X/Twitter MP4 template dosyası boş.",
        ));
    }

    if !template.contains("data-video-slot") {
        return Err(ApiError::new(
            "template_missing_data_video_slot",
            "X/Twitter MP4 template içinde data-video-slot bulunamadı.",
        ));
    }

    Ok(template.to_string())
}

#[tauri::command]
async fn update_ytdlp(app: tauri::AppHandle) -> ApiResult<ToolsUpdateResult> {
    let result = tauri::async_runtime::spawn_blocking(move || update_ytdlp_blocking(app))
        .await
        .map_err(|err| ApiError::new("thread_error", format!("Eklenti güncelleme thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

fn update_ytdlp_blocking(app: tauri::AppHandle) -> Result<ToolsUpdateResult, String> {
    let yt_dlp = ensure_runtime_tool(&app, "yt-dlp")?;

    let mut command = ytdlp_command(&yt_dlp);
    command.arg("-U");

    let output = capture_command_with_timeout(command, Duration::from_secs(45))
        .map_err(|err| format!("yt-dlp güncelleme kontrolü tamamlanamadı: {}", err))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr).trim().to_string();

    if !output.status.success() {
        let msg = if combined.is_empty() {
            "yt-dlp güncelleme başarısız oldu.".to_string()
        } else {
            combined.clone()
        };

        let ctx = ErrorReportContext::new("yt-dlp-update", "yt-dlp -U", &combined)
            .platform("Tools")
            .kind("update_ytdlp");
        let msg = with_error_report(&app, msg, ctx);
        return Err(msg);
    }

    let lower = combined.to_lowercase();
    let updated = lower.contains("updated yt-dlp")
        || lower.contains("updated to")
        || lower.contains("successfully updated")
        || (lower.contains("updating") && !lower.contains("up to date"));
    if updated {
        if let Ok(mut cache) = runtime_tool_cache().lock() {
            cache.remove("yt-dlp");
        }
        invalidate_all_youtube_analyses();
        ensure_runtime_tool(&app, "yt-dlp")?;
    }

    let message = if combined.is_empty() {
        "yt-dlp kontrol edildi.".to_string()
    } else {
        combined
    };

    Ok(ToolsUpdateResult {
        checked: true,
        updated,
        message,
    })
}

fn youtube_browser_auth_required(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("confirm your age")
        || lower.contains("age-restricted")
        || lower.contains("age restricted")
        || lower.contains("inappropriate for some users")
}

fn youtube_browser_cookie_read_failed(error: &str) -> bool {
    let lower = error.to_lowercase();
    (lower.contains("could not copy") && lower.contains("cookie"))
        || lower.contains("failed to decrypt")
        || lower.contains("cookie database")
        || lower.contains("cookies-from-browser")
}

fn youtube_browser_cookie_database_locked(error: &str) -> bool {
    let lower = error.to_lowercase();
    (lower.contains("could not copy") && lower.contains("cookie"))
        || lower.contains("cookie database is locked")
        || lower.contains("cookie database is busy")
        || lower.contains("sharing violation")
}

fn youtube_cookie_analysis_error(
    error: &str,
    cookie_browser: Option<&str>,
    browser_restart_available: bool,
) -> Option<String> {
    if cookie_browser.is_some()
        && browser_restart_available
        && youtube_browser_cookie_database_locked(error)
    {
        return Some(structured_backend_error(
            "browser_restart_required",
            "Seçili tarayıcı cookie veritabanını kullanıyor. YouTube oturumunu bir kez okuyabilmek için aynı tarayıcıyı kısa süreliğine yeniden başlatmalıyız.",
        ));
    }

    if youtube_browser_auth_required(error)
        || (cookie_browser.is_some() && youtube_browser_cookie_read_failed(error))
    {
        let (code, message) = if cookie_browser.is_some() {
            (
                "youtube_auth_failed",
                "Seçili tarayıcıda yaş doğrulaması yapılmış bir YouTube oturumu bulunamadı. YouTube'a giriş yaptığın başka bir tarayıcı seç.",
            )
        } else {
            (
                "youtube_auth_required",
                "Bu video yaş kısıtlı. İndirebilmek için YouTube'a giriş yaptığın tarayıcı oturumuna izin vermelisin.",
            )
        };
        return Some(structured_backend_error(code, message));
    }

    None
}

fn twitter_cookie_analysis_error(
    error: &str,
    cookie_browser: Option<&str>,
    browser_restart_available: bool,
) -> Option<String> {
    if cookie_browser.is_some()
        && browser_restart_available
        && youtube_browser_cookie_database_locked(error)
    {
        return Some(structured_backend_error(
            "browser_restart_required",
            "Seçili tarayıcı cookie veritabanını kullanıyor. X/Twitter oturumunu bir kez okuyabilmek için aynı tarayıcıyı kısa süreliğine yeniden başlatmalıyız.",
        ));
    }

    if cookie_browser.is_some() && youtube_browser_cookie_read_failed(error) {
        return Some(structured_backend_error(
            "twitter_auth_failed",
            "Seçili tarayıcıdan X/Twitter oturumu okunamadı. X'e giriş yaptığın başka bir tarayıcı seç.",
        ));
    }

    None
}

fn friendly_media_access_error(platform: &str, error: &str) -> Option<String> {
    let lower = error.to_lowercase();

    if lower.contains("unsupported url") || lower.contains("not a valid url") {
        return Some(unsupported_media_link_message().to_string());
    }

    let login_required = lower.contains("login required")
        || lower.contains("cookies")
        || lower.contains("private")
        || lower.contains("sign in to confirm")
        || lower.contains("members-only")
        || lower.contains("age-restricted")
        || lower.contains("not a bot");

    if login_required {
        return Some(format!(
            "{} videosu gizli, yaş kısıtlı veya giriş gerektiriyor olabilir.",
            platform
        ));
    }

    let unavailable = lower.contains("video unavailable")
        || lower.contains("this video is unavailable")
        || lower.contains("has been removed")
        || lower.contains("not found")
        || lower.contains("http error 404")
        || lower.contains("status code 404")
        || lower.contains("content isn't available")
        || lower.contains("content is not available")
        || lower.contains("no video")
        || lower.contains("invalid video id")
        || lower.contains("incomplete youtube id");

    if unavailable {
        return Some(format!(
            "{} linki bozuk, video silinmiş ya da böyle bir video yok gibi görünüyor.",
            platform
        ));
    }

    let no_formats = lower.contains("no video formats found")
        || lower.contains("requested format is not available")
        || lower.contains("no media links found");

    if no_formats {
        return Some(format!(
            "{} videosunda indirilebilir uygun format bulunamadı.",
            platform
        ));
    }

    None
}

fn mediadrop_download_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base_dir = app.path().download_dir().or_else(|_| {
        std::env::var("USERPROFILE")
            .map(|profile| PathBuf::from(profile).join("Downloads"))
            .map_err(|_| "USERPROFILE bulunamadı. İndirme klasörü belirlenemedi.".to_string())
    })?;

    let dir = base_dir.join("MediaDrop");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("MediaDrop indirme klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

fn resolve_download_dir(
    app: &tauri::AppHandle,
    output_dir: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(custom_dir) = output_dir {
        let clean = custom_dir.trim();

        if !clean.is_empty() {
            let dir = PathBuf::from(clean);

            fs::create_dir_all(&dir)
                .map_err(|err| format!("Seçilen indirme klasörü oluşturulamadı: {}", err))?;

            return Ok(dir);
        }
    }

    mediadrop_download_dir(app)
}

fn mediadrop_runtime_bin_dir() -> Result<PathBuf, String> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .or_else(|_| {
            std::env::var("USERPROFILE").map(|profile| format!("{}\\AppData\\Local", profile))
        })
        .map_err(|_| "LOCALAPPDATA bulunamadı. Araç klasörü oluşturulamadı.".to_string())?;

    let dir = PathBuf::from(local_app_data).join("MediaDrop").join("bin");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("MediaDrop araç klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

fn mediadrop_thumbnail_dir() -> Result<PathBuf, String> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .or_else(|_| {
            std::env::var("USERPROFILE").map(|profile| format!("{}\\AppData\\Local", profile))
        })
        .map_err(|_| "LOCALAPPDATA bulunamadı. Thumbnail klasörü oluşturulamadı.".to_string())?;

    let dir = PathBuf::from(local_app_data)
        .join("MediaDrop")
        .join("thumbs");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("MediaDrop thumbnail klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

fn mediadrop_error_reports_dir() -> Result<PathBuf, String> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .or_else(|_| {
            std::env::var("USERPROFILE").map(|profile| format!("{}\\AppData\\Local", profile))
        })
        .map_err(|_| "LOCALAPPDATA bulunamadı. Hata raporu klasörü oluşturulamadı.".to_string())?;

    let dir = PathBuf::from(local_app_data)
        .join("MediaDrop")
        .join("Hata Raporlari");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("Hata raporu klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

fn safe_report_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase()
}

fn limit_report_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let tail = value
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    format!(
        "[Rapor metni kısaltıldı. Son {} karakter gösteriliyor.]\n\n{}",
        max_chars, tail
    )
}

fn is_sensitive_url_key(key: &str) -> bool {
    let key = key.trim().trim_start_matches('?').to_ascii_lowercase();

    matches!(
        key.as_str(),
        "sig"
            | "signature"
            | "lsig"
            | "token"
            | "access_token"
            | "auth"
            | "authorization"
            | "cookie"
            | "key"
            | "policy"
            | "expire"
            | "expires"
            | "n"
            | "x-goog-signature"
            | "x-goog-credential"
            | "x-goog-policy"
            | "x-goog-algorithm"
            | "x-goog-date"
    ) || key.contains("token")
        || key.contains("secret")
        || key.contains("signature")
        || key.contains("credential")
}

fn redact_url_query(url: &str) -> String {
    let Some(query_start) = url.find('?') else {
        return url.to_string();
    };

    let (prefix, query_with_marker) = url.split_at(query_start + 1);
    let query = query_with_marker;
    let (query, fragment) = query
        .split_once('#')
        .map(|(left, right)| (left, Some(right)))
        .unwrap_or((query, None));

    let redacted_query = query
        .split('&')
        .map(|part| {
            let key = part.split_once('=').map(|(key, _)| key).unwrap_or(part);

            if is_sensitive_url_key(key) {
                format!("{}=REDACTED", key)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");

    match fragment {
        Some(fragment) => format!("{}{}#{}", prefix, redacted_query, fragment),
        None => format!("{}{}", prefix, redacted_query),
    }
}

fn redact_url_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    let scheme_start = lower.find("https://").or_else(|| lower.find("http://"));

    let Some(start) = scheme_start else {
        return token.to_string();
    };

    let (prefix, url_and_suffix) = token.split_at(start);
    let trimmed_len = url_and_suffix
        .trim_end_matches([')', ']', '}', '"', '\'', ',', ';'])
        .len();
    let (url, suffix) = url_and_suffix.split_at(trimmed_len);

    format!("{}{}{}", prefix, redact_url_query(url), suffix)
}

fn redact_sensitive_url_parts(value: &str) -> String {
    value
        .split_inclusive(|ch: char| ch.is_whitespace())
        .map(redact_url_token)
        .collect::<String>()
}

fn sanitize_report_text(value: &str) -> String {
    let mut text = value.to_string();

    // Önce daha özel path'leri maskele.
    // Eğer önce USERPROFILE maskelenirse:
    // C:\Users\mabki\AppData\Local -> %USERPROFILE%\AppData\Local olur
    // ve LOCALAPPDATA maskesi artık çalışmaz.
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        if !local_app_data.trim().is_empty() {
            text = text.replace(&local_app_data, "%LOCALAPPDATA%");
            text = text.replace(&local_app_data.replace('\\', "/"), "%LOCALAPPDATA%");
        }
    }

    if let Ok(temp) = std::env::var("TEMP") {
        if !temp.trim().is_empty() {
            text = text.replace(&temp, "%TEMP%");
            text = text.replace(&temp.replace('\\', "/"), "%TEMP%");
        }
    }

    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        if !user_profile.trim().is_empty() {
            text = text.replace(&user_profile, "%USERPROFILE%");
            text = text.replace(&user_profile.replace('\\', "/"), "%USERPROFILE%");
        }
    }

    redact_sensitive_url_parts(&text)
}

fn sanitized_path(path: &Path) -> String {
    sanitize_report_text(&path.to_string_lossy())
}

fn tool_version_report(app: &tauri::AppHandle, tool: &str, args: &[&str]) -> String {
    match ensure_runtime_tool(app, tool) {
        Ok(path) => {
            match command_first_line_with_timeout(&path, args, DIAGNOSTIC_COMMAND_TIMEOUT) {
                Ok(first_line) | Err(first_line) => sanitize_report_text(&first_line),
            }
        }
        Err(err) => format!("Bulunamadı: {}", sanitize_report_text(&err)),
    }
}

fn tool_path_report(app: &tauri::AppHandle, tool: &str) -> String {
    match ensure_runtime_tool(app, tool) {
        Ok(path) => sanitized_path(&path),
        Err(err) => format!("Bulunamadı: {}", sanitize_report_text(&err)),
    }
}

fn os_report_line() -> String {
    let mut command = hidden_command("cmd.exe");
    command.args(["/C", "ver"]);

    let output = capture_command_with_timeout(command, DIAGNOSTIC_COMMAND_TIMEOUT);

    match output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                "Bilinmiyor".to_string()
            } else {
                text
            }
        }
        Err(_) => "Bilinmiyor".to_string(),
    }
}

fn env_flag(name: &str) -> &'static str {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => "true",
        _ => "false",
    }
}

fn env_value_or_unknown(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Bilinmiyor".to_string())
}

fn output_dir_writable_report(path: &Path) -> String {
    if !path.is_dir() {
        return "false (klasör yok)".to_string();
    }

    let test_path = path.join(format!(".mediadrop-write-test-{}.tmp", now_ms()));

    match fs::write(&test_path, b"test") {
        Ok(()) => {
            let _ = fs::remove_file(&test_path);
            "true".to_string()
        }
        Err(err) => format!("false ({})", err),
    }
}

fn drive_from_path(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();

    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Some(text[0..2].to_string());
    }

    None
}

fn free_space_report(path: Option<&Path>) -> String {
    let Some(path) = path else {
        return "Bilinmiyor".to_string();
    };

    let Some(drive) = drive_from_path(path) else {
        return "Bilinmiyor".to_string();
    };

    let query = format!("DeviceID='{}'", drive);
    let mut command = hidden_command("wmic.exe");
    command
        .arg("logicaldisk")
        .arg("where")
        .arg(&query)
        .arg("get")
        .arg("FreeSpace")
        .arg("/value");

    let output = capture_command_with_timeout(command, DIAGNOSTIC_COMMAND_TIMEOUT);

    let Ok(output) = output else {
        return "Bilinmiyor".to_string();
    };

    if !output.status.success() {
        return "Bilinmiyor".to_string();
    }

    let text = String::from_utf8_lossy(&output.stdout);

    for line in text.lines() {
        if let Some(raw) = line.trim().strip_prefix("FreeSpace=") {
            if let Ok(bytes) = raw.trim().parse::<f64>() {
                return format!("{:.2} GB", bytes / 1024.0 / 1024.0 / 1024.0);
            }
        }
    }

    "Bilinmiyor".to_string()
}

fn platform_from_kind_or_url(kind: &str, url: &str) -> &'static str {
    let clean_kind = kind.trim().to_lowercase();

    if clean_kind == "twitter" || is_twitter_url(url) {
        return "X/Twitter";
    }

    if clean_kind == "instagram" || is_instagram_url(url) {
        return "Instagram";
    }

    if clean_kind == "tiktok" || is_tiktok_url(url) {
        return "TikTok";
    }

    if is_youtube_url(url) {
        return "YouTube";
    }

    "Bilinmiyor"
}

#[derive(Clone)]
struct ErrorReportContext {
    category: String,
    url: String,
    details: String,
    platform: String,
    kind: String,
    format_id: String,
    quality: String,
    fast_mode: Option<bool>,
    output_mode: Option<String>,
    clip_start_seconds: Option<f64>,
    clip_end_seconds: Option<f64>,
    output_dir: Option<PathBuf>,
}

impl ErrorReportContext {
    fn new(
        category: impl Into<String>,
        url: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            category: category.into(),
            url: url.into(),
            details: details.into(),
            platform: "Bilinmiyor".to_string(),
            kind: "Bilinmiyor".to_string(),
            format_id: "Bilinmiyor".to_string(),
            quality: "Bilinmiyor".to_string(),
            fast_mode: None,
            output_mode: None,
            clip_start_seconds: None,
            clip_end_seconds: None,
            output_dir: None,
        }
    }

    fn platform(mut self, value: impl Into<String>) -> Self {
        self.platform = value.into();
        self
    }

    fn kind(mut self, value: impl Into<String>) -> Self {
        self.kind = value.into();
        self
    }

    fn format_id(mut self, value: impl Into<String>) -> Self {
        self.format_id = value.into();
        self
    }

    fn quality(mut self, value: impl Into<String>) -> Self {
        self.quality = value.into();
        self
    }

    fn fast_mode(mut self, value: bool) -> Self {
        self.fast_mode = Some(value);
        self
    }

    fn output_mode(mut self, value: impl Into<String>) -> Self {
        self.output_mode = Some(value.into());
        self
    }

    fn clip_range(mut self, start: Option<f64>, end: Option<f64>) -> Self {
        self.clip_start_seconds = start;
        self.clip_end_seconds = end;
        self
    }

    fn output_dir(mut self, value: PathBuf) -> Self {
        self.output_dir = Some(value);
        self
    }
}

fn mediadrop_cookie_dir() -> Result<PathBuf, String> {
    let dir = mediadrop_config_dir()?.join("cookies");
    fs::create_dir_all(&dir)
        .map_err(|err| format!("MediaDrop cookie klasoru olusturulamadi: {}", err))?;
    Ok(dir)
}

fn instagram_cookie_store_path() -> Result<PathBuf, String> {
    Ok(mediadrop_cookie_dir()?.join("instagram-cookies.dpapi"))
}

fn instagram_cookie_meta_path() -> Result<PathBuf, String> {
    Ok(mediadrop_cookie_dir()?.join("instagram-cookies.json"))
}

fn instagram_session_store_path() -> Result<PathBuf, String> {
    Ok(mediadrop_cookie_dir()?.join("instagram-session.dpapi"))
}

fn instagram_session_meta_path() -> Result<PathBuf, String> {
    Ok(mediadrop_cookie_dir()?.join("instagram-session.json"))
}

#[cfg(target_os = "windows")]
fn dpapi_protect_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if bytes.is_empty() {
        return Err("Saklanacak cookie verisi bos.".to_string());
    }

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 || output.pbData.is_null() || output.cbData == 0 {
        return Err("Cookie verisi Windows tarafinda sifrelenemedi.".to_string());
    }

    let protected =
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }

    Ok(protected)
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if bytes.is_empty() {
        return Err("Kayitli cookie verisi bos.".to_string());
    }

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 || output.pbData.is_null() || output.cbData == 0 {
        return Err("Kayitli Instagram cookie verisi acilamadi.".to_string());
    }

    let plain = unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }

    Ok(plain)
}

#[cfg(not(target_os = "windows"))]
fn dpapi_protect_bytes(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("Instagram cookie saklama yalnizca Windows DPAPI ile destekleniyor.".to_string())
}

#[cfg(not(target_os = "windows"))]
fn dpapi_unprotect_bytes(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("Instagram cookie okuma yalnizca Windows DPAPI ile destekleniyor.".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SavedInstagramCookieValidation {
    Ready,
    Expired,
    Invalid,
}

fn unix_time_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn parse_netscape_cookie_line(line: &str) -> Option<NetscapeCookie> {
    let clean = line.trim();
    if clean.is_empty() || (clean.starts_with('#') && !clean.starts_with("#HttpOnly_")) {
        return None;
    }

    let fields = clean.split('\t').collect::<Vec<_>>();
    if fields.len() < 7 {
        return None;
    }

    let domain = fields[0].trim().trim_start_matches("#HttpOnly_");
    if domain.is_empty() {
        return None;
    }

    Some(NetscapeCookie {
        domain: domain.to_string(),
        include_subdomains: fields[1].trim().eq_ignore_ascii_case("TRUE"),
        path: fields[2].trim().to_string(),
        secure: fields[3].trim().eq_ignore_ascii_case("TRUE"),
        expires: fields[4].trim().parse::<i64>().unwrap_or(0),
        name: fields[5].trim().to_string(),
        value: fields[6..].join("\t").trim().to_string(),
    })
}

fn validate_netscape_instagram_session(text: &str) -> SavedInstagramCookieValidation {
    let now = unix_time_seconds();
    let mut found_session = false;
    let mut found_user_id = false;
    let mut live_session = false;
    let mut live_user_id = false;

    for cookie in text.lines().filter_map(parse_netscape_cookie_line) {
        if !is_instagram_cookie_host(&cookie.domain) || cookie.value.trim().is_empty() {
            continue;
        }

        let live = cookie.expires <= 0 || cookie.expires > now;
        match cookie.name.as_str() {
            "sessionid" => {
                found_session = true;
                live_session |= live;
            }
            "ds_user_id" => {
                found_user_id = true;
                live_user_id |= live;
            }
            _ => {}
        }
    }

    if live_session && live_user_id {
        SavedInstagramCookieValidation::Ready
    } else if (found_session && !live_session) || (found_user_id && !live_user_id) {
        SavedInstagramCookieValidation::Expired
    } else {
        SavedInstagramCookieValidation::Invalid
    }
}

fn netscape_cookie_has_instagram_session(text: &str) -> bool {
    validate_netscape_instagram_session(text) == SavedInstagramCookieValidation::Ready
}

fn read_instagram_cookie_state_blocking() -> InstagramCookieState {
    let cookie_store_path = instagram_cookie_store_path().ok();
    let has_saved_cookie_jar = cookie_store_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let (status, error) = if !has_saved_cookie_jar {
        (InstagramCookieStatus::Missing, None)
    } else {
        let validation = cookie_store_path
            .as_ref()
            .ok_or_else(|| "Cookie kayit yolu bulunamadi.".to_string())
            .and_then(|path| {
                fs::read(path).map_err(|err| format!("Kayitli cookie okunamadi: {}", err))
            })
            .and_then(|protected| dpapi_unprotect_bytes(&protected))
            .and_then(|plain| {
                String::from_utf8(plain)
                    .map_err(|_| "Kayitli cookie metin formatinda degil.".to_string())
            })
            .map(|text| validate_netscape_instagram_session(&text));

        match validation {
            Ok(SavedInstagramCookieValidation::Ready) => (InstagramCookieStatus::Ready, None),
            Ok(SavedInstagramCookieValidation::Expired) => (
                InstagramCookieStatus::Expired,
                Some("Kayitli Instagram oturumunun suresi dolmus.".to_string()),
            ),
            Ok(SavedInstagramCookieValidation::Invalid) => (
                InstagramCookieStatus::Invalid,
                Some("Kayitli Instagram cookie verisinde gecerli sessionid yok.".to_string()),
            ),
            Err(err) => (InstagramCookieStatus::Invalid, Some(err)),
        }
    };
    let has_saved_cookies = status == InstagramCookieStatus::Ready;

    let meta = instagram_cookie_meta_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .or_else(|| {
            instagram_session_meta_path()
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        })
        .unwrap_or_else(|| json!({}));

    InstagramCookieState {
        has_saved_cookies,
        status,
        error,
        browser_id: meta
            .get("browserId")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        label: meta
            .get("label")
            .or_else(|| meta.get("browserLabel"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        updated_at_ms: meta
            .get("updatedAtMs")
            .and_then(|value| value.as_u64())
            .map(|value| value as u128),
    }
}

fn read_instagram_session_meta() -> serde_json::Value {
    instagram_session_meta_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_else(|| json!({}))
}

fn materialize_saved_instagram_cookie_file() -> Result<TempArtifact, String> {
    let protected = fs::read(instagram_cookie_store_path()?)
        .map_err(|err| format!("Kayitli Instagram cookie verisi okunamadi: {}", err))?;
    let plain = dpapi_unprotect_bytes(&protected)?;
    let text = String::from_utf8(plain)
        .map_err(|_| "Kayitli Instagram cookie verisi metin formatinda degil.".to_string())?;

    match validate_netscape_instagram_session(&text) {
        SavedInstagramCookieValidation::Ready => {}
        SavedInstagramCookieValidation::Expired => {
            return Err("Kayitli Instagram oturumunun suresi dolmus.".to_string());
        }
        SavedInstagramCookieValidation::Invalid => {
            return Err("Kayitli Instagram cookie verisi gecersiz veya bos.".to_string());
        }
    }

    TempArtifact::write(
        &std::env::temp_dir(),
        "mediadrop-instagram-cookies-",
        ".txt",
        text.as_bytes(),
    )
}

fn prune_prepared_instagram_cookies_locked(cookies: &mut HashMap<String, PreparedInstagramCookie>) {
    let now = now_ms();
    cookies.retain(|_, cookie| {
        now.saturating_sub(cookie.created_at_ms) <= PREPARED_INSTAGRAM_COOKIE_TTL_MS
    });
}

fn store_prepared_instagram_cookie_jar(jar: &BrowserCookieJar) -> Result<String, String> {
    let text = cookie_jar_to_netscape(&jar.cookies);

    if !netscape_cookie_has_instagram_session(&text) {
        return Err("Hazirlanan Instagram cookie verisi gecersiz veya bos.".to_string());
    }

    let token = Uuid::new_v4().to_string();
    let prepared = PreparedInstagramCookie {
        text,
        browser_id: jar.browser_id.clone(),
        created_at_ms: now_ms(),
    };
    let mut cookies = prepared_instagram_cookies()
        .lock()
        .map_err(|_| "Gecici Instagram cookie kilidi alinamadi.".to_string())?;
    prune_prepared_instagram_cookies_locked(&mut cookies);
    cookies.insert(token.clone(), prepared);
    Ok(token)
}

fn materialize_prepared_instagram_cookie_file(token: &str) -> Result<TempArtifact, String> {
    let clean = token.trim();
    if clean.is_empty() {
        return Err("Gecici Instagram cookie token'i bos.".to_string());
    }

    let prepared = {
        let mut cookies = prepared_instagram_cookies()
            .lock()
            .map_err(|_| "Gecici Instagram cookie kilidi alinamadi.".to_string())?;
        prune_prepared_instagram_cookies_locked(&mut cookies);
        cookies.get(clean).cloned().ok_or_else(|| {
            "Gecici Instagram cookie izni suresi doldu. Tekrar izin ver.".to_string()
        })?
    };

    if now_ms().saturating_sub(prepared.created_at_ms) > PREPARED_INSTAGRAM_COOKIE_TTL_MS {
        return Err("Gecici Instagram cookie izni suresi doldu. Tekrar izin ver.".to_string());
    }

    if !netscape_cookie_has_instagram_session(&prepared.text) {
        return Err("Gecici Instagram cookie verisi gecersiz veya bos.".to_string());
    }

    TempArtifact::write(
        &std::env::temp_dir(),
        &format!("mediadrop-instagram-prepared-{}-", prepared.browser_id),
        ".txt",
        prepared.text.as_bytes(),
    )
}

fn structured_backend_error(code: &str, message: &str) -> String {
    let (retryable, action) = match code {
        "instagram_auth_required" | "twitter_auth_required" | "twitter_auth_failed" => {
            (true, Some("request_cookie_permission"))
        }
        "instagram_browser_locked" | "browser_restart_required" | "browser_still_running" => {
            (true, Some("request_browser_restart"))
        }
        "instagram_rate_limited" => (true, Some("retry_later")),
        "instagram_highlight_unsupported" => (false, Some("instagram_highlight_unsupported")),
        "download_busy" => (true, Some("wait_for_active_download")),
        _ => (false, None),
    };
    let payload = json!({
        "code": code,
        "message": sanitize_report_text(message),
        "retryable": retryable,
        "action": action,
        "reportId": serde_json::Value::Null,
    });
    format!(
        "{}{}",
        STRUCTURED_ERROR_PREFIX,
        serde_json::to_string(&payload)
            .unwrap_or_else(|_| format!("{{\"code\":\"{}\",\"message\":\"{}\"}}", code, message))
    )
}

fn instagram_highlight_unsupported_error(clean_url: &str) -> Option<String> {
    (media_platform_from_url(clean_url) == "instagram" && instagram_highlight_url(clean_url)).then(
        || {
            structured_backend_error(
                "instagram_highlight_unsupported",
                "Instagram highlight baglantilari aktif Story olarak desteklenmiyor.",
            )
        },
    )
}

fn save_instagram_session_file(
    session_file: &Path,
    browser_id: &str,
    browser_label: &str,
    username: &str,
    helper_version: Option<&str>,
) -> Result<(), String> {
    let clean_username = username.trim();
    if clean_username.is_empty() || !session_file.is_file() {
        return Ok(());
    }

    let bytes = fs::read(session_file)
        .map_err(|err| format!("Instagram session dosyasi okunamadi: {}", err))?;
    let protected = dpapi_protect_bytes(&bytes)?;
    fs::write(instagram_session_store_path()?, protected)
        .map_err(|err| format!("Instagram session kaydi yazilamadi: {}", err))?;

    let meta = json!({
        "browserId": browser_id,
        "browserLabel": browser_label,
        "label": browser_label,
        "username": clean_username,
        "updatedAtMs": now_ms(),
        "instaloaderVersion": helper_version.unwrap_or(""),
    });
    let meta_text = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Instagram session meta JSON olusturulamadi: {}", err))?;
    fs::write(instagram_session_meta_path()?, meta_text)
        .map_err(|err| format!("Instagram session meta yazilamadi: {}", err))?;

    Ok(())
}

fn materialize_saved_instagram_session_file() -> Result<(TempArtifact, String), String> {
    let meta = read_instagram_session_meta();
    let username = meta
        .get("username")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Kayitli Instagram session kullanici adi bulunamadi.".to_string())?
        .to_string();

    let protected = fs::read(instagram_session_store_path()?)
        .map_err(|err| format!("Kayitli Instagram session verisi okunamadi: {}", err))?;
    let plain = dpapi_unprotect_bytes(&protected)?;
    if plain.is_empty() {
        return Err("Kayitli Instagram session verisi bos.".to_string());
    }

    let artifact = TempArtifact::write(
        &std::env::temp_dir(),
        "mediadrop-instagram-session-",
        ".bin",
        &plain,
    )?;
    Ok((artifact, username))
}

#[tauri::command]
fn get_instagram_cookie_state() -> InstagramCookieState {
    read_instagram_cookie_state_blocking()
}

#[tauri::command]
fn clear_instagram_cookie_state() -> ApiResult<()> {
    let store_path = instagram_cookie_store_path()?;
    if store_path.exists() {
        fs::remove_file(&store_path)
            .map_err(|err| format!("Instagram cookie kaydi silinemedi: {}", err))?;
    }

    let meta_path = instagram_cookie_meta_path()?;
    if meta_path.exists() {
        let _ = fs::remove_file(meta_path);
    }

    let session_store_path = instagram_session_store_path()?;
    if session_store_path.exists() {
        fs::remove_file(&session_store_path)
            .map_err(|err| format!("Instagram session kaydi silinemedi: {}", err))?;
    }

    let session_meta_path = instagram_session_meta_path()?;
    if session_meta_path.exists() {
        let _ = fs::remove_file(session_meta_path);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
#[path = "infra/sqlite_snapshot.rs"]
mod sqlite_win;
#[cfg(target_os = "windows")]
fn nt_success(status: i32) -> bool {
    status >= 0
}

#[cfg(target_os = "windows")]
fn bcrypt_u32_property(
    handle: windows_sys::Win32::Security::Cryptography::BCRYPT_HANDLE,
    property: windows_sys::core::PCWSTR,
) -> Result<u32, String> {
    use windows_sys::Win32::Security::Cryptography::BCryptGetProperty;

    let mut output = 0u32;
    let mut result = 0u32;
    let status = unsafe {
        BCryptGetProperty(
            handle,
            property,
            (&mut output as *mut u32).cast::<u8>(),
            std::mem::size_of::<u32>() as u32,
            &mut result,
            0,
        )
    };

    if nt_success(status) {
        Ok(output)
    } else {
        Err(format!("BCrypt ozelligi okunamadi: {}", status))
    }
}

#[cfg(target_os = "windows")]
fn aes_gcm_decrypt(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptCloseAlgorithmProvider, BCryptDecrypt, BCryptDestroyKey, BCryptGenerateSymmetricKey,
        BCryptOpenAlgorithmProvider, BCryptSetProperty, BCRYPT_AES_ALGORITHM,
        BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO, BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
        BCRYPT_CHAINING_MODE, BCRYPT_OBJECT_LENGTH,
    };

    if key.is_empty() || nonce.is_empty() || tag.is_empty() {
        return Err("AES-GCM girdisi eksik.".to_string());
    }

    let mut alg = ptr::null_mut();
    let status =
        unsafe { BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_AES_ALGORITHM, ptr::null(), 0) };
    if !nt_success(status) || alg.is_null() {
        return Err(format!("AES saglayicisi acilamadi: {}", status));
    }

    let close_alg = |alg| unsafe {
        BCryptCloseAlgorithmProvider(alg, 0);
    };

    let chain_mode: Vec<u16> = "ChainingModeGCM".encode_utf16().chain(Some(0)).collect();
    let status = unsafe {
        BCryptSetProperty(
            alg,
            BCRYPT_CHAINING_MODE,
            chain_mode.as_ptr().cast::<u8>(),
            (chain_mode.len() * 2) as u32,
            0,
        )
    };
    if !nt_success(status) {
        close_alg(alg);
        return Err(format!("AES-GCM modu ayarlanamadi: {}", status));
    }

    let object_len = match bcrypt_u32_property(alg, BCRYPT_OBJECT_LENGTH) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            close_alg(alg);
            return Err("AES anahtar nesne boyutu bos.".to_string());
        }
        Err(err) => {
            close_alg(alg);
            return Err(err);
        }
    };

    let mut key_object = vec![0u8; object_len as usize];
    let mut key_handle = ptr::null_mut();
    let status = unsafe {
        BCryptGenerateSymmetricKey(
            alg,
            &mut key_handle,
            key_object.as_mut_ptr(),
            key_object.len() as u32,
            key.as_ptr(),
            key.len() as u32,
            0,
        )
    };
    if !nt_success(status) || key_handle.is_null() {
        close_alg(alg);
        return Err(format!("AES anahtari olusturulamadi: {}", status));
    }

    let mut nonce_buffer = nonce.to_vec();
    let mut tag_buffer = tag.to_vec();
    let mut info = BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
        cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
        dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
        pbNonce: nonce_buffer.as_mut_ptr(),
        cbNonce: nonce_buffer.len() as u32,
        pbAuthData: ptr::null_mut(),
        cbAuthData: 0,
        pbTag: tag_buffer.as_mut_ptr(),
        cbTag: tag_buffer.len() as u32,
        pbMacContext: ptr::null_mut(),
        cbMacContext: 0,
        cbAAD: 0,
        cbData: ciphertext.len() as u64,
        dwFlags: 0,
    };

    let mut plain = vec![0u8; ciphertext.len()];
    let mut result = 0u32;
    let status = unsafe {
        BCryptDecrypt(
            key_handle,
            ciphertext.as_ptr(),
            ciphertext.len() as u32,
            (&mut info as *mut BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO).cast(),
            ptr::null_mut(),
            0,
            plain.as_mut_ptr(),
            plain.len() as u32,
            &mut result,
            0,
        )
    };

    unsafe {
        BCryptDestroyKey(key_handle);
    }
    close_alg(alg);

    if !nt_success(status) {
        return Err(format!("AES-GCM cookie cozulemedi: {}", status));
    }

    plain.truncate(result as usize);
    Ok(plain)
}

fn ensure_install_id_in_config(value: &mut serde_json::Value) -> String {
    if !value.is_object() {
        *value = json!({});
    }

    if let Some(id) = value.get("install_id").and_then(|item| item.as_str()) {
        let clean = id.trim();
        if !clean.is_empty() {
            return clean.to_string();
        }
    }

    let id = format!("md_{}", Uuid::new_v4());

    if let Some(obj) = value.as_object_mut() {
        obj.insert("install_id".to_string(), json!(id.clone()));
        obj.insert("created_at_ms".to_string(), json!(now_ms()));
    }

    id
}

fn get_or_create_install_id() -> String {
    update_mediadrop_config(ensure_install_id_in_config)
        .unwrap_or_else(|_| format!("md_{}", Uuid::new_v4()))
}

fn cloud_reports_enabled_from_config(config: &serde_json::Value) -> bool {
    config
        .get("cloud_reports_enabled")
        .and_then(|item| item.as_bool())
        .unwrap_or(false)
}

fn cloud_reports_enabled_value() -> bool {
    let Ok(_guard) = mediadrop_config_io().lock() else {
        return false;
    };
    let Ok(value) = try_read_mediadrop_config_unlocked() else {
        return false;
    };
    value
        .as_ref()
        .map(cloud_reports_enabled_from_config)
        .unwrap_or(false)
}

fn set_cloud_reports_enabled_value(enabled: bool) -> Result<(), String> {
    update_mediadrop_config(|config| {
        let _ = ensure_install_id_in_config(config);
        if let Some(obj) = config.as_object_mut() {
            obj.insert("cloud_reports_enabled".to_string(), json!(enabled));
            obj.insert("cloud_reports_updated_at_ms".to_string(), json!(now_ms()));
        }
    })
}

#[tauri::command]
fn get_cloud_reports_enabled() -> bool {
    cloud_reports_enabled_value()
}

#[tauri::command]
fn set_cloud_reports_enabled(enabled: bool) -> ApiResult<()> {
    set_cloud_reports_enabled_value(enabled).map_err(ApiError::from)
}

fn mediadrop_pending_reports_dir() -> Result<PathBuf, String> {
    let dir = mediadrop_config_dir()?.join("pending_reports");

    fs::create_dir_all(&dir)
        .map_err(|err| format!("Bekleyen hata raporu klasörü oluşturulamadı: {}", err))?;

    Ok(dir)
}

fn pending_report_file_name() -> String {
    format!("pending-report-{}.json", Uuid::new_v4())
}

static PENDING_REPORT_IO: OnceLock<Mutex<()>> = OnceLock::new();

fn pending_report_io() -> &'static Mutex<()> {
    PENDING_REPORT_IO.get_or_init(|| Mutex::new(()))
}

fn trim_pending_cloud_reports(max_files: usize) {
    let Ok(dir) = mediadrop_pending_reports_dir() else {
        return;
    };

    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
        })
        .collect::<Vec<_>>();

    if files.len() <= max_files {
        return;
    }

    files.sort_by(|a, b| {
        let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
        a_time.cmp(&b_time)
    });

    let remove_count = files.len().saturating_sub(max_files);

    for path in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

fn save_pending_cloud_report(payload: serde_json::Value, reason: &str) -> Result<PathBuf, String> {
    let _guard = pending_report_io()
        .lock()
        .map_err(|_| "Bekleyen rapor kuyruğu kilidi alınamadı.".to_string())?;
    let dir = mediadrop_pending_reports_dir()?;
    let path = dir.join(pending_report_file_name());
    let temp_path = dir.join(format!("pending-report-{}.tmp", Uuid::new_v4()));
    let content = json!({
        "queued_at_ms": now_ms(),
        "reason": sanitize_report_text(reason),
        "payload": payload
    });

    let text = serde_json::to_string_pretty(&content)
        .map_err(|err| format!("Bekleyen cloud raporu JSON oluşturulamadı: {}", err))?;

    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|err| format!("Bekleyen cloud raporu açılamadı: {}", err))?;
        file.write_all(text.as_bytes())
            .map_err(|err| format!("Bekleyen cloud raporu yazılamadı: {}", err))?;
        file.sync_all()
            .map_err(|err| format!("Bekleyen cloud raporu diske yazılamadı: {}", err))?;
        atomic_replace_config_file(&temp_path, &path)
            .map_err(|err| format!("Bekleyen cloud raporu kuyruğa alınamadı: {}", err))
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;
    trim_pending_cloud_reports(50);

    Ok(path)
}

fn read_pending_payload(path: &Path) -> Result<serde_json::Value, String> {
    let text =
        fs::read_to_string(path).map_err(|err| format!("Bekleyen rapor okunamadı: {}", err))?;

    let value = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|err| format!("Bekleyen rapor JSON okunamadı: {}", err))?;

    if let Some(payload) = value.get("payload") {
        return Ok(payload.clone());
    }

    Ok(value)
}

fn pending_report_files() -> Vec<PathBuf> {
    let Ok(dir) = mediadrop_pending_reports_dir() else {
        return Vec::new();
    };

    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
        })
        .collect::<Vec<_>>();

    files.sort_by(|a, b| {
        let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
        a_time.cmp(&b_time)
    });

    files
}

fn recover_stale_claimed_reports() {
    let Ok(dir) = mediadrop_pending_reports_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let stale_before = SystemTime::now()
        .checked_sub(Duration::from_secs(10 * 60))
        .unwrap_or(UNIX_EPOCH);
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        let is_claim = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".sending-"));
        let is_stale = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .map(|modified| modified <= stale_before)
            .unwrap_or(false);
        if is_claim && is_stale {
            let target = dir.join(pending_report_file_name());
            let _ = fs::rename(path, target);
        }
    }
}

fn flush_pending_cloud_reports_blocking() -> Result<usize, String> {
    if !cloud_reports_enabled_value() {
        return Ok(0);
    }

    let _guard = pending_report_io()
        .lock()
        .map_err(|_| "Bekleyen rapor kuyruğu kilidi alınamadı.".to_string())?;
    recover_stale_claimed_reports();

    let mut sent = 0usize;
    let files = pending_report_files();

    for path in files.into_iter().take(20) {
        let claim = path.with_extension(format!("sending-{}", Uuid::new_v4()));
        if fs::rename(&path, &claim).is_err() {
            continue;
        }
        let payload = match read_pending_payload(&claim) {
            Ok(payload) => payload,
            Err(err) => {
                write_cloud_report_debug_log(&format!(
                    "Bekleyen rapor okunamadı, siliniyor: {} | {}",
                    sanitized_path(&claim),
                    err
                ));
                let _ = fs::remove_file(claim);
                continue;
            }
        };

        match send_cloud_report_payload(&payload) {
            Ok(response) => {
                sent += 1;
                let _ = fs::remove_file(&claim);
                write_cloud_report_debug_log(&format!(
                    "Bekleyen cloud raporu gönderildi: {} | Response: {}",
                    sanitized_path(&path),
                    response
                ));
            }
            Err(err) => {
                if atomic_replace_config_file(&claim, &path).is_err() {
                    write_cloud_report_debug_log(&format!(
                        "Bekleyen rapor claim dosyası geri alınamadı: {}",
                        sanitized_path(&claim)
                    ));
                }
                write_cloud_report_debug_log(&format!(
                    "Bekleyen cloud raporu gönderilemedi, kuyrukta kalacak: {} | Error: {}",
                    sanitized_path(&path),
                    err
                ));
                break;
            }
        }
    }

    Ok(sent)
}

#[tauri::command]
async fn flush_pending_cloud_reports() -> ApiResult<usize> {
    let result = tauri::async_runtime::spawn_blocking(flush_pending_cloud_reports_blocking)
        .await
        .map_err(|err| ApiError::new("thread_error", format!("Bekleyen cloud raporu thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

fn json_escape_report_value(value: &str) -> String {
    sanitize_report_text(value)
}

fn cloud_fast_mode_value(value: Option<bool>) -> serde_json::Value {
    match value {
        Some(flag) => json!(flag),
        None => serde_json::Value::Null,
    }
}

fn build_cloud_report_payload(ctx: &ErrorReportContext, report_text: &str) -> serde_json::Value {
    let build_mode = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };

    let output_dir_text = ctx
        .output_dir
        .as_ref()
        .map(|path| sanitized_path(path))
        .unwrap_or_else(|| "Bilinmiyor".to_string());

    let output_exists = ctx
        .output_dir
        .as_ref()
        .map(|path| path.is_dir())
        .unwrap_or(false);

    let output_writable = ctx
        .output_dir
        .as_ref()
        .map(|path| output_dir_writable_report(path))
        .unwrap_or_else(|| "Bilinmiyor".to_string());

    let free_space = free_space_report(ctx.output_dir.as_deref());

    json!({
        "install_id": get_or_create_install_id(),
        "app_version": MEDIADROP_APP_VERSION,
        "build_mode": build_mode,

        "category": ctx.category,
        "platform": ctx.platform,
        "url": json_escape_report_value(&ctx.url),
        "kind": ctx.kind,
        "format_id": ctx.format_id,
        "quality": ctx.quality,
        "fast_mode": cloud_fast_mode_value(ctx.fast_mode),
        "output_mode": ctx.output_mode.as_deref().unwrap_or("Bilinmiyor"),
        "clip_start_seconds": ctx.clip_start_seconds,
        "clip_end_seconds": ctx.clip_end_seconds,

        "os": os_report_line(),
        "arch": env_value_or_unknown("PROCESSOR_ARCHITECTURE"),

        "tools": {
            "yt_dlp": tool_version_report_dummy_safe("yt-dlp"),
            "ffmpeg": "Raporda mevcut",
            "ffprobe": "Raporda mevcut",
            "aria2c": "Raporda mevcut",
            "deno": "Raporda mevcut",
            "gallery_dl": "Raporda mevcut"
        },

        "storage": {
            "output_dir": output_dir_text,
            "output_dir_exists": output_exists,
            "output_dir_writable": output_writable,
            "free_space": free_space
        },

        "environment": {
            "http_proxy": env_flag("HTTP_PROXY") == "true",
            "https_proxy": env_flag("HTTPS_PROXY") == "true",
            "no_proxy": env_flag("NO_PROXY") == "true",
            "rust_log": env_flag("RUST_LOG") == "true"
        },

        "report_text": report_text
    })
}

// Cloud payload içinde tool sürümünü ayrıca uzun uzun hesaplamaya gerek yok.
// Asıl detay zaten report_text içinde var.
// Bu alan admin tabloda dolu dursun diye minimal bırakıyoruz.
fn tool_version_report_dummy_safe(tool: &str) -> String {
    format!("{}: report_text içinde", tool)
}

fn send_cloud_report_payload(payload: &serde_json::Value) -> Result<String, String> {
    let client = cloud_report_client()?;

    let response = client
        .post(CLOUD_REPORT_ENDPOINT)
        .header("x-mediadrop-client", "desktop")
        .header("x-mediadrop-version", MEDIADROP_APP_VERSION)
        .json(payload)
        .send()
        .map_err(|err| {
            format!(
                "Cloud rapor gönderilemedi: {}\nDebug: {:?}\nURL: {}",
                err, err, CLOUD_REPORT_ENDPOINT
            )
        })?;

    let status = response.status();
    let text = response.text().unwrap_or_else(|_| "".to_string());

    if !status.is_success() {
        return Err(format!("Cloud rapor reddedildi: {} | {}", status, text));
    }

    Ok(text)
}

fn write_cloud_report_debug_log(message: &str) {
    let Ok(dir) = mediadrop_error_reports_dir() else {
        return;
    };

    let path = dir.join("cloud-report-status.txt");

    let content = format!("[{}]\n{}\n\n", now_ms(), sanitize_report_text(message));

    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(content.as_bytes())
        });
}

fn process_cloud_error_report(ctx: ErrorReportContext, report_text: String) {
    if !cloud_reports_enabled_value() {
        write_cloud_report_debug_log(
            "Cloud rapor gönderimi kapalı. Rapor yalnızca yerelde tutuldu.",
        );
        return;
    }

    let payload = build_cloud_report_payload(&ctx, &report_text);
    let _ = flush_pending_cloud_reports_blocking();

    let mut last_error = String::new();

    for attempt in 1..=3 {
        match send_cloud_report_payload(&payload) {
            Ok(response) => {
                write_cloud_report_debug_log(&format!(
                    "Cloud rapor gönderildi. Attempt: {} | Response: {}",
                    attempt, response
                ));
                let _ = flush_pending_cloud_reports_blocking();
                return;
            }
            Err(err) => {
                last_error = err;
                write_cloud_report_debug_log(&format!(
                    "Cloud rapor gönderilemedi. Attempt: {} | Error: {}",
                    attempt, last_error
                ));

                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }

    match save_pending_cloud_report(payload, &last_error) {
        Ok(path) => write_cloud_report_debug_log(&format!(
            "Cloud rapor 3 denemeden sonra kuyruğa alındı: {} | Son hata: {}",
            sanitized_path(&path),
            last_error
        )),
        Err(queue_err) => write_cloud_report_debug_log(&format!(
            "Cloud rapor kuyruğa alınamadı. Son hata: {} | Queue error: {}",
            last_error, queue_err
        )),
    }
}

struct CloudReportTask {
    ctx: ErrorReportContext,
    report_text: String,
}

static CLOUD_REPORT_WORKER: OnceLock<std::sync::mpsc::SyncSender<CloudReportTask>> =
    OnceLock::new();

fn cloud_report_worker() -> &'static std::sync::mpsc::SyncSender<CloudReportTask> {
    CLOUD_REPORT_WORKER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<CloudReportTask>(24);
        let _ = thread::Builder::new()
            .name("mediadrop-report-worker".to_string())
            .spawn(move || {
                while let Ok(task) = receiver.recv() {
                    process_cloud_error_report(task.ctx, task.report_text);
                }
            });
        sender
    })
}

fn send_error_report_to_cloud_background(ctx: ErrorReportContext, report_text: String) {
    let task = CloudReportTask { ctx, report_text };
    match cloud_report_worker().try_send(task) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(task))
        | Err(std::sync::mpsc::TrySendError::Disconnected(task)) => {
            let payload = build_cloud_report_payload(&task.ctx, &task.report_text);
            let _ = save_pending_cloud_report(payload, "bounded report worker unavailable");
            write_cloud_report_debug_log(
                "Cloud rapor worker kuyruğu dolu veya kapalı; rapor bekleyen kuyruğa alındı.",
            );
        }
    }
}

struct DiagnosticToolSnapshot {
    yt_dlp_version: String,
    ffmpeg_version: String,
    ffprobe_version: String,
    aria2c_version: String,
    deno_version: String,
    gallery_dl_version: String,
    yt_dlp_path: String,
    ffmpeg_path: String,
    ffprobe_path: String,
    aria2c_path: String,
    deno_path: String,
    gallery_dl_path: String,
}

static DIAGNOSTIC_TOOL_SNAPSHOT: OnceLock<DiagnosticToolSnapshot> = OnceLock::new();

fn diagnostic_tool_snapshot(app: &tauri::AppHandle) -> &'static DiagnosticToolSnapshot {
    DIAGNOSTIC_TOOL_SNAPSHOT.get_or_init(|| DiagnosticToolSnapshot {
        yt_dlp_version: tool_version_report(app, "yt-dlp", &["--version"]),
        ffmpeg_version: tool_version_report(app, "ffmpeg", &["-version"]),
        ffprobe_version: tool_version_report(app, "ffprobe", &["-version"]),
        aria2c_version: tool_version_report(app, "aria2c", &["--version"]),
        deno_version: tool_version_report(app, "deno", &["--version"]),
        gallery_dl_version: tool_version_report(app, "gallery-dl", &["--version"]),
        yt_dlp_path: tool_path_report(app, "yt-dlp"),
        ffmpeg_path: tool_path_report(app, "ffmpeg"),
        ffprobe_path: tool_path_report(app, "ffprobe"),
        aria2c_path: tool_path_report(app, "aria2c"),
        deno_path: tool_path_report(app, "deno"),
        gallery_dl_path: tool_path_report(app, "gallery-dl"),
    })
}

fn format_error_report(app: &tauri::AppHandle, ctx: &ErrorReportContext) -> String {
    let tools = diagnostic_tool_snapshot(app);
    let build_mode = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let output_dir_text = ctx
        .output_dir
        .as_ref()
        .map(|path| sanitized_path(path))
        .unwrap_or_else(|| "Bilinmiyor".to_string());
    let output_exists = ctx
        .output_dir
        .as_ref()
        .map(|path| path.is_dir().to_string())
        .unwrap_or_else(|| "Bilinmiyor".to_string());
    let output_writable = ctx
        .output_dir
        .as_ref()
        .map(|path| output_dir_writable_report(path))
        .unwrap_or_else(|| "Bilinmiyor".to_string());
    let free_space = free_space_report(ctx.output_dir.as_deref());
    let fast_mode = ctx
        .fast_mode
        .map(|value| value.to_string())
        .unwrap_or_else(|| "Bilinmiyor".to_string());
    let output_mode = ctx.output_mode.as_deref().unwrap_or("Bilinmiyor");

    let raw_details = sanitize_report_text(&limit_report_text(&ctx.details, 14000));
    let clean_url = sanitize_report_text(&ctx.url);

    format!(
        "MediaDrop Hata Raporu\n=====================\n\n\
[App]\n\
Product: MediaDrop\n\
Version: {}\n\
Build Mode: {}\n\
Identifier: {}\n\n\
[System]\n\
OS: {}\n\
Architecture: {}\n\
Locale Env: {}\n\
Timezone Env: {}\n\n\
[Operation]\n\
Category: {}\n\
Platform: {}\n\
URL: {}\n\
Kind: {}\n\
Format ID: {}\n\
Quality: {}\n\
Fast Mode: {}\n\
Output Mode: {}\n\
Clip Start: {}\n\
Clip End: {}\n\
Output Dir: {}\n\n\
[Tools]\n\
yt-dlp: {}\n\
ffmpeg: {}\n\
ffprobe: {}\n\
aria2c: {}\n\
deno: {}\n\
gallery-dl: {}\n\n\
[Tool Paths]\n\
yt-dlp: {}\n\
ffmpeg: {}\n\
ffprobe: {}\n\
aria2c: {}\n\
deno: {}\n\
gallery-dl: {}\n\n\
[Storage]\n\
Output dir exists: {}\n\
Output dir writable: {}\n\
Free space: {}\n\n\
[Environment]\n\
HTTP_PROXY set: {}\n\
HTTPS_PROXY set: {}\n\
NO_PROXY set: {}\n\
RUST_LOG set: {}\n\n\
[Raw Error / Details]\n{}\n",
        MEDIADROP_APP_VERSION,
        build_mode,
        MEDIADROP_IDENTIFIER,
        os_report_line(),
        env_value_or_unknown("PROCESSOR_ARCHITECTURE"),
        env_value_or_unknown("LANG"),
        env_value_or_unknown("TZ"),
        ctx.category,
        ctx.platform,
        clean_url,
        ctx.kind,
        ctx.format_id,
        ctx.quality,
        fast_mode,
        output_mode,
        clip_report_value(ctx.clip_start_seconds),
        clip_report_value(ctx.clip_end_seconds),
        output_dir_text,
        tools.yt_dlp_version,
        tools.ffmpeg_version,
        tools.ffprobe_version,
        tools.aria2c_version,
        tools.deno_version,
        tools.gallery_dl_version,
        tools.yt_dlp_path,
        tools.ffmpeg_path,
        tools.ffprobe_path,
        tools.aria2c_path,
        tools.deno_path,
        tools.gallery_dl_path,
        output_exists,
        output_writable,
        free_space,
        env_flag("HTTP_PROXY"),
        env_flag("HTTPS_PROXY"),
        env_flag("NO_PROXY"),
        env_flag("RUST_LOG"),
        raw_details
    )
}

fn write_error_report(app: &tauri::AppHandle, ctx: &ErrorReportContext) -> Result<PathBuf, String> {
    let dir = mediadrop_error_reports_dir()?;
    let stamp = now_ms();
    let clean_category = safe_report_filename(&ctx.category);
    let path = dir.join(format!("mediadrop-{}-{}.txt", clean_category, stamp));
    let content = format_error_report(app, ctx);

    fs::write(&path, &content).map_err(|err| format!("Hata raporu yazılamadı: {}", err))?;

    send_error_report_to_cloud_background(ctx.clone(), content);

    Ok(path)
}

fn with_error_report(app: &tauri::AppHandle, message: String, ctx: ErrorReportContext) -> String {
    match write_error_report(app, &ctx) {
        Ok(path) => format!(
            "{}\n\nHata raporu oluşturuldu: {}",
            message,
            sanitized_path(&path)
        ),
        Err(_) => message,
    }
}

fn with_optional_error_report(
    app: &tauri::AppHandle,
    enabled: bool,
    message: String,
    ctx: ErrorReportContext,
) -> String {
    if enabled {
        with_error_report(app, message, ctx)
    } else {
        message
    }
}

/// Hata raporları klasöründeki en son oluşturulan .txt dosyasını
/// Windows Explorer'da seçili şekilde açar.
#[tauri::command]
fn reveal_last_error_report() -> ApiResult<()> {
    let dir = mediadrop_error_reports_dir()?;

    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&dir)
        .map_err(|err| format!("Hata raporu klasörü okunamadı: {}", err))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));

    let latest = entries
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .ok_or_else(|| "Henüz hiç hata raporu oluşturulmamış.".to_string())?;

    reveal_file_in_explorer(&latest).map_err(ApiError::from)
}

fn unique_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn existing_file(path: PathBuf) -> Option<PathBuf> {
    if path.exists() && path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn find_in_path(tool: &str) -> Option<PathBuf> {
    let mut command = hidden_command("where.exe");
    command.arg(tool);

    let output = capture_command_with_timeout(command, DIAGNOSTIC_COMMAND_TIMEOUT).ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let first_line = text.lines().next()?.trim();

    if first_line.is_empty() {
        return None;
    }

    existing_file(PathBuf::from(first_line))
}

fn candidate_tool_paths(app: &tauri::AppHandle, tool: &str) -> Vec<PathBuf> {
    let plain = format!("{}.exe", tool);
    let suffixed = format!("{}-{}.exe", tool, TARGET_TRIPLE);

    let mut paths = Vec::new();

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            paths.push(exe_dir.join(&plain));
            paths.push(exe_dir.join(&suffixed));
            paths.push(exe_dir.join("binaries").join(&plain));
            paths.push(exe_dir.join("binaries").join(&suffixed));
            paths.push(exe_dir.join("resources").join(&plain));
            paths.push(exe_dir.join("resources").join(&suffixed));
            paths.push(exe_dir.join("resources").join("binaries").join(&plain));
            paths.push(exe_dir.join("resources").join("binaries").join(&suffixed));
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        paths.push(resource_dir.join(&plain));
        paths.push(resource_dir.join(&suffixed));
        paths.push(resource_dir.join("binaries").join(&plain));
        paths.push(resource_dir.join("binaries").join(&suffixed));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        paths.push(current_dir.join("src-tauri").join("binaries").join(&plain));
        paths.push(
            current_dir
                .join("src-tauri")
                .join("binaries")
                .join(&suffixed),
        );
        paths.push(current_dir.join("binaries").join(&plain));
        paths.push(current_dir.join("binaries").join(&suffixed));
    }

    if tool == "ffmpeg" || tool == "ffprobe" {
        paths.push(PathBuf::from(FFMPEG_DIR).join(&plain));
    }

    paths
}

fn find_source_tool_path(app: &tauri::AppHandle, tool: &str) -> Result<PathBuf, String> {
    for path in candidate_tool_paths(app, tool) {
        if let Some(found) = existing_file(path) {
            return Ok(found);
        }
    }

    if let Some(found) = find_in_path(tool) {
        return Ok(found);
    }

    Err(format!(
        "{} bulunamadı. Installer içine gömülü binary veya PATH kurulumu yok.",
        tool
    ))
}

static RUNTIME_TOOL_CACHE: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

fn runtime_tool_cache() -> &'static Mutex<HashMap<String, PathBuf>> {
    RUNTIME_TOOL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn runtime_tool_file_is_valid(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn copy_runtime_tool_atomically(tool: &str, source: &Path, dest: &Path) -> Result<(), String> {
    let temp = dest.with_extension(format!("exe.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        fs::copy(source, &temp).map_err(|err| {
            format!(
                "{} runtime klasörüne kopyalanamadı: {} -> {} | {}",
                tool,
                source.to_string_lossy(),
                dest.to_string_lossy(),
                err
            )
        })?;
        if !runtime_tool_file_is_valid(&temp) {
            return Err(format!("{} runtime kopyası boş veya geçersiz.", tool));
        }
        atomic_replace_config_file(&temp, dest)
            .map_err(|err| format!("{} runtime kopyası etkinleştirilemedi: {}", tool, err))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn ensure_runtime_tool(app: &tauri::AppHandle, tool: &str) -> Result<PathBuf, String> {
    if let Ok(cache) = runtime_tool_cache().lock() {
        if let Some(path) = cache
            .get(tool)
            .filter(|path| runtime_tool_file_is_valid(path))
        {
            return Ok(path.clone());
        }
    }

    let bin_dir = mediadrop_runtime_bin_dir()?;
    let dest = bin_dir.join(format!("{}.exe", tool));
    let source = find_source_tool_path(app, tool);
    let resolved = match source {
        Ok(source_path) => {
            if !same_path(&source_path, &dest) {
                let should_copy = should_copy_runtime_tool(tool, &source_path, &dest);
                if should_copy {
                    copy_runtime_tool_atomically(tool, &source_path, &dest)?;
                }
            }
            if runtime_tool_is_healthy(tool, &dest) {
                Ok(dest.clone())
            } else {
                Err(format!("{} runtime sağlık kontrolü başarısız.", tool))
            }
        }
        Err(err) => {
            if runtime_tool_is_healthy(tool, &dest) {
                Ok(dest.clone())
            } else {
                Err(err)
            }
        }
    }?;
    runtime_tool_cache()
        .lock()
        .map_err(|_| "Runtime araç kayıt kilidi alınamadı.".to_string())?
        .insert(tool.to_string(), resolved.clone());
    Ok(resolved)
}

fn ensure_runtime_tools(app: &tauri::AppHandle) -> Result<RuntimeTools, String> {
    let yt_dlp = ensure_runtime_tool(app, "yt-dlp")?;
    let aria2c = ensure_runtime_tool(app, "aria2c")?;
    let ffmpeg = ensure_runtime_tool(app, "ffmpeg")?;
    let _ffprobe = ensure_runtime_tool(app, "ffprobe")?;

    // Deno, YouTube tarafındaki JS challenge çözümü için yt-dlp tarafından PATH üzerinden görülür.
    // Eksikse uygulamayı komple düşürmüyoruz; ama bundle'a eklenince runtime klasörüne kopyalanır.
    let _deno = ensure_runtime_tool(app, "deno").ok();

    let ffmpeg_dir = ffmpeg
        .parent()
        .ok_or_else(|| "ffmpeg klasörü belirlenemedi.".to_string())?
        .to_path_buf();

    Ok(RuntimeTools {
        yt_dlp,
        aria2c,
        ffmpeg_dir,
    })
}

fn filename_quality_label(kind: &str, quality: &str) -> String {
    let q = quality.trim().to_lowercase().replace(' ', "");

    if kind == "twitter" {
        return "X-Twitter Best".to_string();
    }

    if kind == "instagram" {
        return "Instagram Best".to_string();
    }

    if kind == "tiktok" {
        return "TikTok Best".to_string();
    }

    if kind == "audio" {
        if q.contains("kbps") {
            return format!("MP3 {}", q);
        }

        return "MP3".to_string();
    }

    match q.as_str() {
        "2160p" => "4K".to_string(),
        "1440p" => "2K".to_string(),
        "1080p" => "1080p".to_string(),
        "720p" => "720p".to_string(),
        "480p" => "480p".to_string(),
        "360p" => "360p".to_string(),
        _ => quality.trim().to_string(),
    }
}

#[derive(Clone, Copy, Debug)]
struct ClipRange {
    start: f64,
    end: f64,
}

fn normalize_clip_range(
    start: Option<f64>,
    end: Option<f64>,
    is_youtube: bool,
    clean_kind: &str,
) -> Result<Option<ClipRange>, String> {
    match (start, end) {
        (Some(start), Some(end)) => {
            if !is_youtube || clean_kind == "audio" {
                return Err(
                    "Klip indirme şu an yalnızca YouTube video formatları için destekleniyor."
                        .to_string(),
                );
            }

            if !start.is_finite() || !end.is_finite() {
                return Err("Klip başlangıç/bitiş zamanı geçersiz.".to_string());
            }

            let start = start.max(0.0);
            let end = end.max(0.0);

            if end <= start {
                return Err("Klip bitiş zamanı başlangıçtan büyük olmalı.".to_string());
            }

            if end - start < 1.0 {
                return Err("Klip süresi en az 1 saniye olmalı.".to_string());
            }

            Ok(Some(ClipRange { start, end }))
        }
        (None, None) => Ok(None),
        _ => Err("Klip için başlangıç ve bitiş zamanı birlikte gönderilmeli.".to_string()),
    }
}

fn format_clip_time(seconds: f64) -> String {
    let safe = if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    };
    let rounded = safe.floor() as u64;
    let hours = rounded / 3600;
    let minutes = (rounded % 3600) / 60;
    let secs = rounded % 60;

    if hours > 0 {
        format!("{}-{:02}-{:02}", hours, minutes, secs)
    } else {
        format!("{:02}-{:02}", minutes, secs)
    }
}

fn clip_file_label(clip: Option<ClipRange>) -> String {
    match clip {
        Some(range) => format!(
            "Klip {}-{}",
            format_clip_time(range.start),
            format_clip_time(range.end)
        ),
        None => "".to_string(),
    }
}

fn clip_download_sections_arg(clip: ClipRange) -> String {
    format!("*{:.3}-{:.3}", clip.start, clip.end)
}

fn clip_report_value(value: Option<f64>) -> String {
    match value {
        Some(number) if number.is_finite() => format!("{:.3}", number),
        _ => "Bilinmiyor".to_string(),
    }
}

const HLS_CLIP_PREROLL_SECONDS: f64 = 5.0;

fn hls_padded_clip_range(range: ClipRange) -> ClipRange {
    ClipRange {
        start: (range.start - HLS_CLIP_PREROLL_SECONDS).max(0.0),
        end: range.end,
    }
}

fn offset_clip_range(original: ClipRange, downloaded: ClipRange) -> ClipRange {
    let start = (original.start - downloaded.start).max(0.0);
    let duration = (original.end - original.start).max(1.0);

    ClipRange {
        start,
        end: start + duration,
    }
}

fn unit_to_mb(value: &str, unit: &str) -> Option<f64> {
    let number = value.replace(',', ".").parse::<f64>().ok()?;
    let normalized = unit.to_lowercase();

    if normalized.contains("gib") || normalized.contains("gb") {
        return Some(number * 1024.0);
    }

    if normalized.contains("kib") || normalized.contains("kb") {
        return Some(number / 1024.0);
    }

    Some(number)
}

fn compact_value_to_mb(value: &str) -> Option<f64> {
    let cleaned = value
        .trim()
        .trim_matches('[')
        .trim_matches(']')
        .replace("/s", "");

    let mut number = String::new();
    let mut unit = String::new();

    for ch in cleaned.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            number.push(ch);
        } else if !number.is_empty() {
            unit.push(ch);
        }
    }

    if number.is_empty() || unit.is_empty() {
        return None;
    }

    unit_to_mb(&number, &unit)
}

fn structured_progress_number(value: Option<&str>) -> Option<f64> {
    value?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn parse_yt_dlp_progress(line: &str) -> Option<ProgressMetrics> {
    if line.starts_with(YTDLP_PROGRESS_MARKER) {
        let mut parts = line.split('\t');
        parts.next()?;
        parts.next()?; // status

        let downloaded_bytes = structured_progress_number(parts.next());
        let total_bytes = structured_progress_number(parts.next()).filter(|total| *total > 0.0);
        let estimated_bytes =
            structured_progress_number(parts.next()).filter(|total| *total > 0.0);
        let speed_bytes = structured_progress_number(parts.next());
        let total_bytes = total_bytes.or(estimated_bytes);
        let percent = match (downloaded_bytes, total_bytes) {
            (Some(downloaded), Some(total)) => Some((downloaded / total * 100.0).clamp(0.0, 100.0)),
            _ => None,
        };

        return Some(ProgressMetrics {
            percent,
            downloaded_mb: downloaded_bytes.map(|bytes| bytes / 1024.0 / 1024.0),
            total_mb: total_bytes.map(|bytes| bytes / 1024.0 / 1024.0),
            speed_mb: speed_bytes.map(|bytes| bytes / 1024.0 / 1024.0),
        });
    }

    if !line.contains("[download]") || !line.contains('%') {
        return None;
    }

    let percent_index = line.find('%')?;
    let before_percent = &line[..percent_index];
    let percent_raw = before_percent.split_whitespace().last()?;
    let percent = percent_raw.replace(',', ".").parse::<f64>().ok();

    let total_mb = if let Some(of_index) = line.find(" of ") {
        let after_of = &line[of_index + 4..];
        let total_token = after_of.split_whitespace().next().unwrap_or("");
        compact_value_to_mb(total_token)
    } else {
        None
    };

    let speed_mb = if let Some(at_index) = line.find(" at ") {
        let after_at = &line[at_index + 4..];
        let speed_token = after_at.split_whitespace().next().unwrap_or("");
        compact_value_to_mb(speed_token)
    } else {
        None
    };

    let downloaded_mb = match (percent, total_mb) {
        (Some(p), Some(total)) => Some((total * p) / 100.0),
        _ => None,
    };

    Some(ProgressMetrics {
        percent,
        downloaded_mb,
        total_mb,
        speed_mb,
    })
}

fn parse_aria2_progress(line: &str) -> Option<ProgressMetrics> {
    if !line.contains("DL:") || !line.contains('/') || !line.contains('%') {
        return None;
    }

    let mut percent = None;
    let mut downloaded_mb = None;
    let mut total_mb = None;
    let mut speed_mb = None;

    for token in line.split_whitespace() {
        if token.contains('/') && token.contains('(') && token.contains('%') {
            let token = token.trim_matches('[').trim_matches(']');

            if let Some((downloaded_raw, rest)) = token.split_once('/') {
                downloaded_mb = compact_value_to_mb(downloaded_raw);

                if let Some((total_raw, percent_part)) = rest.split_once('(') {
                    total_mb = compact_value_to_mb(total_raw);

                    let percent_raw = percent_part
                        .split('%')
                        .next()
                        .unwrap_or("")
                        .replace(',', ".");

                    percent = percent_raw.parse::<f64>().ok();
                }
            }
        }

        if let Some(speed_raw) = token.strip_prefix("DL:") {
            speed_mb = compact_value_to_mb(speed_raw);
        }
    }

    if percent.is_none() && downloaded_mb.is_none() && total_mb.is_none() && speed_mb.is_none() {
        return None;
    }

    Some(ProgressMetrics {
        percent,
        downloaded_mb,
        total_mb,
        speed_mb,
    })
}

fn parse_progress_line(line: &str) -> Option<ProgressMetrics> {
    parse_aria2_progress(line).or_else(|| parse_yt_dlp_progress(line))
}

fn is_ytdlp_postprocess_line(line: &str) -> bool {
    let lower = line.to_lowercase();

    lower.contains("[merger]")
        || lower.contains("[extractaudio]")
        || lower.contains("[videoconvertor]")
        || lower.contains("[fixup")
        || lower.contains("[movefiles]")
        || lower.contains("merging formats")
        || lower.contains("postprocess")
        || lower.contains("post-process")
        || lower.contains("post processor")
        || (lower.contains("ffmpeg")
            && (lower.contains("merg")
                || lower.contains("convert")
                || lower.contains("extract")
                || lower.contains("post")))
}

fn phase_from_line(line: &str) -> Option<String> {
    if line.contains("Destination") {
        return Some("Dosya hazırlanıyor...".to_string());
    }

    if line.contains("[Merger]") {
        return Some("Video ve ses birleştiriliyor...".to_string());
    }

    if line.contains("[ExtractAudio]") {
        return Some("Ses MP3'e dönüştürülüyor...".to_string());
    }

    if line.contains("Deleting original") {
        return Some("Geçici dosyalar temizleniyor...".to_string());
    }

    if is_ytdlp_postprocess_line(line) {
        return Some("Birleştiriliyor / son işlem yapılıyor...".to_string());
    }

    if line.contains("[download]") {
        return Some("İndiriliyor...".to_string());
    }

    None
}

fn emit_progress(
    app: &tauri::AppHandle,
    percent: Option<f64>,
    downloaded_mb: Option<f64>,
    total_mb: Option<f64>,
    speed_mb: Option<f64>,
    phase: impl Into<String>,
    line: impl Into<String>,
) {
    let phase = phase.into();
    let line = line.into();
    update_download_job_progress(percent, downloaded_mb, total_mb, speed_mb, &phase);
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            job_id: current_download_job_id(),
            percent,
            downloaded_mb,
            total_mb,
            speed_mb,
            phase,
            line,
        },
    );
}

fn emit_simple_progress(app: &tauri::AppHandle, percent: Option<f64>, phase: impl Into<String>) {
    emit_progress(app, percent, None, None, None, phase, "");
}

#[derive(Clone, Copy)]
enum DownloadProgressMode {
    Default,
    TwitterPostMp4Download,
}

fn handle_output_line_with_mode(app: &tauri::AppHandle, line: String, mode: DownloadProgressMode) {
    if let Some(metrics) = parse_progress_line(&line) {
        let (percent, phase) = match mode {
            DownloadProgressMode::Default => (metrics.percent, "İndiriliyor..."),
            DownloadProgressMode::TwitterPostMp4Download => (
                metrics
                    .percent
                    .map(|percent| (percent * 0.8).clamp(0.0, 80.0)),
                "Gönderi videosu indiriliyor...",
            ),
        };

        emit_progress(
            app,
            percent,
            metrics.downloaded_mb,
            metrics.total_mb,
            metrics.speed_mb,
            phase,
            line,
        );

        return;
    }

    if let Some(phase) = phase_from_line(&line) {
        let phase = match mode {
            DownloadProgressMode::Default => phase,
            DownloadProgressMode::TwitterPostMp4Download => {
                "Gönderi videosu indiriliyor...".to_string()
            }
        };

        emit_progress(app, None, None, None, None, phase, line);
    }
}

#[derive(Clone, Copy)]
struct YtdlpWatchdogConfig {
    notice_after: Duration,
    download_stall_timeout: Duration,
    postprocess_stall_timeout: Duration,
}

impl YtdlpWatchdogConfig {
    fn full_download() -> Self {
        Self {
            notice_after: DOWNLOAD_STALL_NOTICE_AFTER,
            download_stall_timeout: FULL_DOWNLOAD_STALL_TIMEOUT,
            postprocess_stall_timeout: FULL_POSTPROCESS_STALL_TIMEOUT,
        }
    }

    fn clip_download() -> Self {
        Self {
            notice_after: DOWNLOAD_STALL_NOTICE_AFTER,
            download_stall_timeout: CLIP_DOWNLOAD_STALL_TIMEOUT,
            postprocess_stall_timeout: CLIP_POSTPROCESS_STALL_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug)]
struct YtdlpFinalOutput {
    job_id: Option<String>,
    path: PathBuf,
}

static LAST_YTDLP_FINAL_OUTPUT: OnceLock<Mutex<Option<YtdlpFinalOutput>>> = OnceLock::new();

fn ytdlp_final_output_slot() -> &'static Mutex<Option<YtdlpFinalOutput>> {
    LAST_YTDLP_FINAL_OUTPUT.get_or_init(|| Mutex::new(None))
}

fn clear_ytdlp_final_output() {
    if let Ok(mut slot) = ytdlp_final_output_slot().lock() {
        *slot = None;
    }
}

fn record_ytdlp_final_output(job_id: Option<String>, line: &str) -> bool {
    let Some(path) = line.trim().strip_prefix(YTDLP_FINAL_PATH_MARKER) else {
        return false;
    };
    let path = path.trim();
    if path.is_empty() {
        return true;
    }
    if let Ok(mut slot) = ytdlp_final_output_slot().lock() {
        *slot = Some(YtdlpFinalOutput {
            job_id,
            path: PathBuf::from(path),
        });
    }
    true
}

fn take_ytdlp_final_output(output_dir: &Path) -> Option<PathBuf> {
    let output = ytdlp_final_output_slot().lock().ok()?.take()?;
    if output.job_id != current_download_job_id() {
        return None;
    }
    let path = if output.path.is_absolute() {
        output.path
    } else {
        output_dir.join(output.path)
    };
    let canonical_dir = fs::canonicalize(output_dir).ok()?;
    let canonical_path = fs::canonicalize(path).ok()?;
    if !canonical_path.starts_with(&canonical_dir)
        || !canonical_path.is_file()
        || file_size(&canonical_path).unwrap_or(0) == 0
    {
        return None;
    }
    Some(canonical_path)
}

fn run_ytdlp_process_with_watchdog(
    app: &tauri::AppHandle,
    mut command: Command,
    output_dir: &Path,
    started_at_ms: u128,
    watchdog: Option<YtdlpWatchdogConfig>,
    progress_mode: DownloadProgressMode,
    stop_generation: Option<u64>,
) -> Result<(), String> {
    clear_ytdlp_final_output();
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("yt-dlp indirme komutu başlatılamadı: {}", err))?;

    let pid = child.id();
    mark_active_process(pid, output_dir, started_at_ms);

    if let Some(generation) = stop_generation {
        if let Err(err) = check_download_stop_since(generation) {
            let _ = kill_process_tree(pid);
            let _ = child.kill();
            let _ = child.wait();
            let _ = take_finished_process_request(pid);
            return Err(err);
        }
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stderr_text = Arc::new(Mutex::new(String::new()));
    let last_output_ms = Arc::new(AtomicU64::new(now_ms() as u64));
    let process_phase = Arc::new(AtomicU64::new(YTDLP_PHASE_DOWNLOAD));

    let stdout_handle = if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let job_id = current_download_job_id();
        let last_output_ms_clone = Arc::clone(&last_output_ms);
        let process_phase_clone = Arc::clone(&process_phase);

        Some(thread::spawn(move || {
            read_process_lines_lossy(stdout, |line| {
                last_output_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                if record_ytdlp_final_output(job_id.clone(), &line) {
                    return;
                }
                if is_ytdlp_postprocess_line(&line) {
                    process_phase_clone.store(YTDLP_PHASE_POSTPROCESS, Ordering::Relaxed);
                } else if parse_progress_line(&line).is_some() {
                    process_phase_clone.store(YTDLP_PHASE_DOWNLOAD, Ordering::Relaxed);
                }
                handle_output_line_with_mode(&app_clone, line, progress_mode);
            });
        }))
    } else {
        None
    };

    let stderr_handle = if let Some(stderr) = stderr {
        let app_clone = app.clone();
        let stderr_text_clone = Arc::clone(&stderr_text);
        let last_output_ms_clone = Arc::clone(&last_output_ms);
        let process_phase_clone = Arc::clone(&process_phase);

        Some(thread::spawn(move || {
            read_process_lines_lossy(stderr, |line| {
                last_output_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                if is_ytdlp_postprocess_line(&line) {
                    process_phase_clone.store(YTDLP_PHASE_POSTPROCESS, Ordering::Relaxed);
                } else if parse_progress_line(&line).is_some() {
                    process_phase_clone.store(YTDLP_PHASE_DOWNLOAD, Ordering::Relaxed);
                }

                if let Ok(mut text) = stderr_text_clone.lock() {
                    append_bounded_text(&mut text, &line, MAX_YTDLP_STDERR_BYTES);
                }

                handle_output_line_with_mode(&app_clone, line, progress_mode);
            });
        }))
    } else {
        None
    };

    let mut watchdog_error: Option<String> = None;
    let mut stop_error: Option<String> = None;
    let initial_watchdog_ms = now_ms() as u64;
    let mut output_activity =
        JobOutputActivityTracker::new(output_dir, started_at_ms, initial_watchdog_ms);
    let mut last_seen_output_size = output_activity.current_total_size(initial_watchdog_ms);
    let mut last_seen_output_ms = last_output_ms.load(Ordering::Relaxed);
    let mut last_file_activity_ms = initial_watchdog_ms;
    let mut inactivity_notice_sent = false;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Some(generation) = stop_generation {
                    if let Err(err) = check_download_stop_since(generation) {
                        stop_error = Some(err);
                        let _ = kill_process_tree(pid);
                        let _ = child.kill();
                        break child.wait().map_err(|err| {
                            format!("İndirme işlemi durdurulurken hata oluştu: {}", err)
                        })?;
                    }
                }

                if let Some(config) = watchdog {
                    let now = now_ms() as u64;
                    let current_output_ms = last_output_ms.load(Ordering::Relaxed);

                    if current_output_ms != last_seen_output_ms {
                        last_seen_output_ms = current_output_ms;
                        inactivity_notice_sent = false;
                    }

                    let current_size = output_activity.current_total_size(now);

                    if last_seen_output_size != current_size {
                        last_seen_output_size = current_size;
                        last_file_activity_ms = now;
                        inactivity_notice_sent = false;
                    }

                    let silent_for = Duration::from_millis(now.saturating_sub(current_output_ms));
                    let file_idle_for =
                        Duration::from_millis(now.saturating_sub(last_file_activity_ms));
                    let postprocess_active =
                        process_phase.load(Ordering::Relaxed) == YTDLP_PHASE_POSTPROCESS;
                    let timeout = if postprocess_active {
                        config.postprocess_stall_timeout
                    } else {
                        config.download_stall_timeout
                    };

                    if !inactivity_notice_sent
                        && silent_for >= config.notice_after
                        && file_idle_for >= config.notice_after
                    {
                        let phase = if postprocess_active {
                            "Birleştiriliyor / son işlem yapılıyor..."
                        } else {
                            "Bağlantı/indirme yanıt vermiyor gibi görünüyor..."
                        };

                        emit_simple_progress(app, None, phase);
                        inactivity_notice_sent = true;
                    }

                    if silent_for >= timeout && file_idle_for >= timeout {
                        watchdog_error = Some(format!(
                            "yt-dlp işlemi {} saniye çıktı/ilerleme üretmedi ve indirilen/geçici dosya boyutu değişmedi. İşlem durduruldu.",
                            timeout.as_secs()
                        ));
                        let _ = kill_process_tree(pid);
                        let _ = child.kill();
                        break child.wait().map_err(|err| {
                            format!("İndirme işlemi durdurulurken hata oluştu: {}", err)
                        })?;
                    }
                }

                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => {
                let _ = kill_process_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("İndirme işlemi durumu okunamadı: {}", err));
            }
        }
    };

    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }

    if let Some(handle) = stderr_handle {
        let _ = handle.join();
    }

    let finished_process = take_finished_process_request(pid);

    if let Some(active) = finished_process {
        match active.request {
            Some(DownloadStopRequest::Pause) => {
                return Err(PAUSED_SIGNAL.to_string());
            }
            Some(DownloadStopRequest::Cancel) => {
                cleanup_incomplete_downloads(&active.output_dir, active.started_at_ms);
                return Err(CANCELLED_SIGNAL.to_string());
            }
            None => {}
        }
    }

    if let Some(err) = watchdog_error {
        cleanup_incomplete_downloads(output_dir, started_at_ms);
        return Err(err);
    }

    if let Some(err) = stop_error {
        return Err(err);
    }

    if !status.success() {
        let error_message = stderr_text
            .lock()
            .map(|text| text.clone())
            .unwrap_or_else(|_| "".to_string());

        if error_message.trim().is_empty() {
            return Err("İndirme sırasında hata oluştu.".to_string());
        }

        return Err(error_message);
    }

    Ok(())
}

fn is_ssl_or_network_error(error: &str) -> bool {
    let lower = error.to_lowercase();

    lower.contains("ssl")
        || lower.contains("tls")
        || lower.contains("certificate")
        || lower.contains("cert")
        || lower.contains("handshake")
        || lower.contains("connection reset")
        || lower.contains("connection aborted")
        || lower.contains("connection refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("remote end closed connection")
        || lower.contains("unexpected eof")
        || lower.contains("eof occurred")
        || lower.contains("invalid_session_id")
        || lower.contains("invalid session id")
        || lower.contains("connection was reset")
        || lower.contains("recv failure")
        || lower.contains("network")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExternalDownloader {
    Native,
    Aria2c,
    Curl,
}

#[derive(Clone)]
struct DownloadAttempt {
    label: String,
    format_selector: String,
    external_downloader: ExternalDownloader,
    force_ipv4: bool,
    force_ipv6: bool,
    impersonate_chrome: bool,
    extractor_args: Option<String>,
    http_chunk_size: Option<String>,
    force_keyframes_at_cuts: bool,
    recode_video: bool,
    only_when_ssl_error: bool,
}

fn download_attempt(label: &str, format_selector: &str) -> DownloadAttempt {
    DownloadAttempt {
        label: label.to_string(),
        format_selector: format_selector.to_string(),
        external_downloader: ExternalDownloader::Native,
        force_ipv4: false,
        force_ipv6: false,
        impersonate_chrome: false,
        extractor_args: None,
        http_chunk_size: None,
        force_keyframes_at_cuts: false,
        recode_video: false,
        only_when_ssl_error: false,
    }
}

fn social_compatible_format_selector() -> &'static str {
    "best[ext=mp4][vcodec^=avc1][acodec!=none]/best[ext=mp4][vcodec^=h264][acodec!=none]/best[vcodec^=avc1][acodec!=none]/best[vcodec^=h264][acodec!=none]/bestvideo[ext=mp4][vcodec^=avc1]+bestaudio[ext=m4a]/bestvideo[ext=mp4][vcodec^=h264]+bestaudio[ext=m4a]/bestvideo[vcodec^=avc1]+bestaudio/bestvideo[vcodec^=h264]+bestaudio/best[ext=mp4]/best"
}

fn social_format_selector(clean_format_id: &str) -> String {
    match clean_format_id {
        "" | "best[ext=mp4]/bestvideo+bestaudio/best" => {
            social_compatible_format_selector().to_string()
        }
        value => value.to_string(),
    }
}

fn youtube_selected_selector(format_id: &str, quality: &str) -> String {
    match quality_height_limit(quality) {
        Some(height) => format!(
            "{0}[acodec!=none]/{0}[acodec=none]+bestaudio[ext=m4a]/{0}[acodec=none]+bestaudio/bestvideo[height<={1}]+bestaudio[ext=m4a]/bestvideo[height<={1}]+bestaudio/best[height<={1}][vcodec!=none][acodec!=none]",
            format_id, height
        ),
        None => format!(
            "{0}[acodec!=none]/{0}[acodec=none]+bestaudio[ext=m4a]/{0}[acodec=none]+bestaudio/bestvideo+bestaudio[ext=m4a]/bestvideo+bestaudio/best[vcodec!=none][acodec!=none]",
            format_id
        ),
    }
}

fn quality_height_limit(quality: &str) -> Option<u32> {
    let clean = quality.trim().to_lowercase();

    if clean.contains("4k") {
        return Some(2160);
    }

    if clean.contains("2k") {
        return Some(1440);
    }

    let digits = clean
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();

    if digits.is_empty() {
        return None;
    }

    digits.parse::<u32>().ok()
}

fn safe_output_filename_part(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;

    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            Some(ch)
        } else if ch == ' ' || ch == '-' || ch == '_' || ch == '.' {
            Some(' ')
        } else {
            None
        };

        if let Some(ch) = mapped {
            if ch == ' ' {
                if !last_was_space && !out.is_empty() {
                    out.push(' ');
                    last_was_space = true;
                }
            } else {
                out.push(ch);
                last_was_space = false;
            }
        }
    }

    let clean = out.trim().to_string();

    if clean.is_empty() {
        "MediaDrop Video".to_string()
    } else {
        clean.chars().take(150).collect()
    }
}

fn is_pretty_filename_alnum(ch: char) -> bool {
    if ch.is_ascii_alphanumeric() {
        return true;
    }

    let code = ch as u32;
    ch.is_alphanumeric() && matches!(code, 0x00C0..=0x024F | 0x1E00..=0x1EFF)
}

fn is_forbidden_windows_filename_char(ch: char) -> bool {
    ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

fn is_invisible_or_directional_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF
    )
}

fn is_windows_reserved_file_stem(value: &str) -> bool {
    let clean = value
        .trim()
        .trim_matches(|ch| ch == '.' || ch == ' ')
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();

    matches!(clean.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (clean.len() == 4
            && (clean.starts_with("COM") || clean.starts_with("LPT"))
            && clean
                .chars()
                .nth(3)
                .map(|ch| ('1'..='9').contains(&ch))
                .unwrap_or(false))
}

fn truncate_filename_chars(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    out = out.trim_matches(|ch| ch == '.' || ch == ' ').to_string();
    out
}

fn pretty_output_filename_part(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }

            continue;
        }

        if is_forbidden_windows_filename_char(ch) || is_invisible_or_directional_char(ch) {
            continue;
        }

        let mapped = if is_pretty_filename_alnum(ch)
            || matches!(ch, '-' | '_' | '.' | '(' | ')' | '[' | ']')
        {
            Some(ch)
        } else if matches!(ch, '&' | '+' | '=' | ',' | ';') {
            Some(' ')
        } else {
            None
        };

        if let Some(mapped) = mapped {
            if mapped == ' ' {
                if !last_was_space && !out.is_empty() {
                    out.push(' ');
                    last_was_space = true;
                }
            } else {
                out.push(mapped);
                last_was_space = false;
            }
        }
    }

    let mut clean = truncate_filename_chars(&out, 150);

    if let Some((first, rest)) = clean.split_once(' ') {
        if is_windows_reserved_file_stem(first) {
            clean = rest.trim().to_string();
        }
    }

    let fallback = truncate_filename_chars(fallback, 80);
    let fallback = if fallback.is_empty() {
        "MediaDrop Video".to_string()
    } else {
        fallback
    };

    if clean.is_empty() || is_windows_reserved_file_stem(&clean) {
        return fallback;
    }

    clean
}

fn unique_output_path(dir: &Path, base_name: &str, ext: &str) -> PathBuf {
    let base = safe_output_filename_part(base_name);
    let ext = ext.trim_start_matches('.');
    let mut candidate = dir.join(format!("{}.{}", base, ext));

    if !candidate.exists() {
        return candidate;
    }

    for index in 2..=999 {
        candidate = dir.join(format!("{} ({}).{}", base, index, ext));
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(format!("{} {}.{}", base, unique_stamp(), ext))
}

fn unique_pretty_output_path(dir: &Path, base_name: &str, ext: &str) -> PathBuf {
    let base = pretty_output_filename_part(base_name, "MediaDrop Video");
    let ext = ext.trim_start_matches('.');
    let ext = if ext.is_empty() { "mp4" } else { ext };
    let mut candidate = dir.join(format!("{}.{}", base, ext));

    if !candidate.exists() {
        return candidate;
    }

    for index in 2..=999 {
        candidate = dir.join(format!("{} ({}).{}", base, index, ext));
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(format!("{} {}.{}", base, unique_stamp(), ext))
}

fn stable_hash_hex(values: &[&str]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;

    for value in values {
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }

        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{:016x}", hash)
}

fn temp_output_stem(
    clean_kind: &str,
    clean_url: &str,
    clean_format_id: &str,
    quality: &str,
    clip_range: Option<ClipRange>,
) -> String {
    let clip_key = clip_range
        .map(|range| format!("{:.3}-{:.3}", range.start, range.end))
        .unwrap_or_default();
    let hash = stable_hash_hex(&[clean_kind, clean_url, clean_format_id, quality, &clip_key]);

    format!("mediadrop-temp-{}", hash)
}

fn fallback_media_title(file_kind: &str) -> &'static str {
    match file_kind {
        "twitter" => "X videosu",
        "instagram" => "Instagram videosu",
        "tiktok" => "TikTok videosu",
        "audio" => "MediaDrop ses",
        _ => "YouTube videosu",
    }
}

fn strip_url_like_words(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|word| {
            let lower = word.to_ascii_lowercase();
            !lower.starts_with("http://")
                && !lower.starts_with("https://")
                && !lower.starts_with("www.")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn concise_social_title(value: &str, fallback: &str) -> String {
    let without_urls = strip_url_like_words(value);
    let clean = pretty_output_filename_part(&without_urls, fallback);

    if clean == fallback {
        return clean;
    }

    let words = clean.split_whitespace().take(6).collect::<Vec<_>>();

    if words.is_empty() {
        fallback.to_string()
    } else {
        truncate_filename_chars(&words.join(" "), 70)
    }
}

fn pretty_media_title(file_kind: &str, title_hint: Option<&str>) -> String {
    let fallback = fallback_media_title(file_kind);
    let raw = title_hint.unwrap_or("").trim();

    match file_kind {
        "twitter" | "instagram" | "tiktok" => concise_social_title(raw, fallback),
        _ => pretty_output_filename_part(raw, fallback),
    }
}

fn pretty_output_base(
    file_kind: &str,
    title_hint: Option<&str>,
    quality: &str,
    clip_range: Option<ClipRange>,
) -> String {
    let title = pretty_media_title(file_kind, title_hint);
    let file_label = filename_quality_label(file_kind, quality);
    let clip_label = clip_file_label(clip_range);

    [title, file_label, clip_label]
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn finalize_pretty_output_file(path: &Path, download_dir: &Path, final_base: &str) -> PathBuf {
    if !path.is_file() {
        return path.to_path_buf();
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    let target = unique_pretty_output_path(download_dir, final_base, ext);

    if same_path(path, &target) {
        return path.to_path_buf();
    }

    match rename_with_retries(path, &target) {
        Ok(()) => target,
        Err(_) => path.to_path_buf(),
    }
}

fn twitter_post_output_base(title_hint: Option<&str>, post_text: &str) -> String {
    let title_source = title_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(post_text);
    let title = pretty_media_title("twitter", Some(title_source));
    truncate_filename_chars(&format!("X Gönderisi - {}", title).replace("  ", " "), 96)
}

fn twitter_post_temp_root() -> Result<PathBuf, String> {
    let root = mediadrop_config_dir()?.join("twitter-post-temp");
    fs::create_dir_all(&root)
        .map_err(|err| format!("Gönderi geçici klasörü oluşturulamadı: {}", err))?;
    Ok(root)
}

fn unique_twitter_post_temp_dir() -> Result<(PathBuf, PathBuf), String> {
    let root = twitter_post_temp_root()?;

    for _ in 0..20 {
        let dir = root.join(format!(
            "{}{}-{}",
            TWITTER_POST_TEMP_DIR_PREFIX,
            unique_stamp(),
            Uuid::new_v4()
        ));

        if dir.exists() {
            continue;
        }

        fs::create_dir(&dir)
            .map_err(|err| format!("Gönderi geçici iş klasörü oluşturulamadı: {}", err))?;

        return Ok((root, dir));
    }

    Err("Gönderi geçici iş klasörü için benzersiz ad üretilemedi.".to_string())
}

fn remove_twitter_post_temp_dir(path: &Path, expected_parent: &Path) {
    remove_owned_temp_dir(path, expected_parent, TWITTER_POST_TEMP_DIR_PREFIX);
}

fn check_twitter_post_stop(
    stop_generation: u64,
    temp_dir: &Path,
    temp_root: &Path,
    output_path: Option<&Path>,
) -> Result<(), String> {
    match check_download_stop_since(stop_generation) {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Some(path) = output_path {
                let _ = fs::remove_file(path);
            }
            remove_twitter_post_temp_dir(temp_dir, temp_root);
            Err(err)
        }
    }
}

fn finalize_twitter_post_source_video(path: &Path, temp_dir: &Path) -> PathBuf {
    if !path.is_file() {
        return path.to_path_buf();
    }

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim_start_matches('.'))
        .filter(|value| !value.is_empty())
        .unwrap_or("mp4");
    let target = temp_dir.join(format!("source-video.{}", ext));

    if same_path(path, &target) {
        return path.to_path_buf();
    }

    match rename_with_retries(path, &target) {
        Ok(()) => target,
        Err(_) => path.to_path_buf(),
    }
}

fn clean_optional_text(value: Option<String>) -> String {
    value.unwrap_or_default().trim().to_string()
}

fn decode_twitter_post_card_png(value: &str) -> Result<Vec<u8>, String> {
    let clean = value
        .trim()
        .strip_prefix("data:image/png;base64,")
        .unwrap_or_else(|| value.trim())
        .split_whitespace()
        .collect::<String>();

    if clean.is_empty() {
        return Err("card_png_render_failed: Gönderi kartı PNG verisi boş.".to_string());
    }

    if clean.len() > MAX_TWITTER_POST_CARD_PNG_BYTES * 2 {
        return Err("card_png_render_failed: Gönderi kartı PNG verisi çok büyük.".to_string());
    }

    let bytes = general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map_err(|_| "card_png_render_failed: Gönderi kartı PNG verisi okunamadı.".to_string())?;

    if bytes.len() > MAX_TWITTER_POST_CARD_PNG_BYTES {
        return Err("card_png_render_failed: Gönderi kartı PNG verisi çok büyük.".to_string());
    }

    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err("card_png_render_failed: Gönderi kartı PNG verisi geçersiz.".to_string());
    }

    Ok(bytes)
}

fn write_twitter_post_card_png(temp_dir: &Path, card_png_base64: &str) -> Result<PathBuf, String> {
    let bytes = decode_twitter_post_card_png(card_png_base64)?;
    let path = temp_dir.join("card.png");

    fs::write(&path, bytes)
        .map_err(|err| format!("card_png_render_failed: Gönderi kartı yazılamadı: {}", err))?;

    Ok(path)
}

fn write_twitter_post_overlay_png(
    temp_dir: &Path,
    overlay_png_base64: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(overlay_png_base64) = overlay_png_base64 else {
        return Ok(None);
    };

    if overlay_png_base64.trim().is_empty() {
        return Ok(None);
    }

    let bytes = decode_twitter_post_card_png(overlay_png_base64)?;
    let path = temp_dir.join("overlay.png");

    fs::write(&path, bytes).map_err(|err| {
        format!(
            "card_png_render_failed: GÃ¶nderi overlay yazÄ±lamadÄ±: {}",
            err
        )
    })?;

    Ok(Some(path))
}

fn even_floor_u32(value: u32) -> u32 {
    value & !1
}

fn validate_twitter_post_card_layout(
    layout: TwitterPostCardLayout,
) -> Result<TwitterPostCardLayout, String> {
    let normalized = TwitterPostCardLayout {
        output_width: even_floor_u32(layout.output_width),
        output_height: even_floor_u32(layout.output_height),
        video_x: even_floor_u32(layout.video_x),
        video_y: even_floor_u32(layout.video_y),
        video_width: even_floor_u32(layout.video_width),
        video_height: even_floor_u32(layout.video_height),
    };

    let invalid_message = "card_layout_invalid: Gönderi kartı layout bilgisi geçersiz.".to_string();

    if normalized.output_width < MIN_TWITTER_POST_OUTPUT_DIMENSION
        || normalized.output_height < MIN_TWITTER_POST_OUTPUT_DIMENSION
        || normalized.output_width > MAX_TWITTER_POST_OUTPUT_WIDTH
        || normalized.output_height > MAX_TWITTER_POST_OUTPUT_HEIGHT
        || normalized.video_width < MIN_TWITTER_POST_VIDEO_SLOT_DIMENSION
        || normalized.video_height < MIN_TWITTER_POST_VIDEO_SLOT_DIMENSION
    {
        return Err(invalid_message);
    }

    let video_right = normalized
        .video_x
        .checked_add(normalized.video_width)
        .ok_or_else(|| invalid_message.clone())?;
    let video_bottom = normalized
        .video_y
        .checked_add(normalized.video_height)
        .ok_or_else(|| invalid_message.clone())?;

    if video_right > normalized.output_width || video_bottom > normalized.output_height {
        return Err(invalid_message);
    }

    Ok(normalized)
}

fn media_duration_seconds(tools: &RuntimeTools, path: &Path) -> Result<f64, String> {
    let ffprobe = tools.ffmpeg_dir.join("ffprobe.exe");
    let mut command = hidden_command(ffprobe);

    command
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path);

    let output = capture_command_with_timeout(command, Duration::from_secs(12))
        .map_err(|err| format!("ffprobe süre okuma zaman aşımı/başlatma hatası: {}", err))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
        return Err(if combined.is_empty() {
            "Video süresi okunamadı.".to_string()
        } else {
            combined
        });
    }

    let duration = stdout
        .trim()
        .parse::<f64>()
        .map_err(|_| "Video süresi okunamadı.".to_string())?;

    if duration.is_finite() && duration > 0.0 {
        Ok(duration)
    } else {
        Err("Video süresi geçersiz.".to_string())
    }
}

fn build_ffmpeg_twitter_post_compose_command(
    tools: &RuntimeTools,
    card_path: &Path,
    video_path: &Path,
    overlay_path: Option<&Path>,
    output_path: &Path,
    layout: TwitterPostCardLayout,
    duration_seconds: f64,
) -> Command {
    let ffmpeg = tools.ffmpeg_dir.join("ffmpeg.exe");
    let mut command = hidden_command(ffmpeg);
    prepend_path(&mut command, &tools.ffmpeg_dir);
    let filter = if overlay_path.is_some() {
        format!(
            "[0:v]scale={output_width}:{output_height},setsar=1[card];\
             [1:v]split=2[slot_bg_src][slot_fg_src];\
             [slot_bg_src]scale={video_width}:{video_height}:force_original_aspect_ratio=increase,\
             crop={video_width}:{video_height}:(iw-{video_width})/2:(ih-{video_height})/2,\
             boxblur=24:2,eq=brightness=-0.06:saturation=0.78,setsar=1[slot_bg];\
             [slot_fg_src]scale={video_width}:{video_height}:force_original_aspect_ratio=decrease,setsar=1[slot_fg];\
             [slot_bg][slot_fg]overlay=(W-w)/2:(H-h)/2:shortest=1[slot];\
             [2:v]scale={output_width}:{output_height},setsar=1[overlay];\
             [card][slot]overlay={video_x}:{video_y}:shortest=1[base];\
             [base][overlay]overlay=0:0:shortest=1,format=yuv420p[outv]",
            output_width = layout.output_width,
            output_height = layout.output_height,
            video_width = layout.video_width,
            video_height = layout.video_height,
            video_x = layout.video_x,
            video_y = layout.video_y
        )
    } else {
        format!(
            "[0:v]scale={output_width}:{output_height},setsar=1[card];\
             [1:v]split=2[slot_bg_src][slot_fg_src];\
             [slot_bg_src]scale={video_width}:{video_height}:force_original_aspect_ratio=increase,\
             crop={video_width}:{video_height}:(iw-{video_width})/2:(ih-{video_height})/2,\
             boxblur=24:2,eq=brightness=-0.06:saturation=0.78,setsar=1[slot_bg];\
             [slot_fg_src]scale={video_width}:{video_height}:force_original_aspect_ratio=decrease,setsar=1[slot_fg];\
             [slot_bg][slot_fg]overlay=(W-w)/2:(H-h)/2:shortest=1[slot];\
             [card][slot]overlay={video_x}:{video_y}:shortest=1,format=yuv420p[outv]",
            output_width = layout.output_width,
            output_height = layout.output_height,
            video_width = layout.video_width,
            video_height = layout.video_height,
            video_x = layout.video_x,
            video_y = layout.video_y
        )
    };

    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-loop")
        .arg("1")
        .arg("-i")
        .arg(card_path)
        .arg("-i")
        .arg(video_path);

    if let Some(overlay_path) = overlay_path {
        command.arg("-loop").arg("1").arg("-i").arg(overlay_path);
    }

    command
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[outv]")
        .arg("-map")
        .arg("1:a?")
        .arg("-t")
        .arg(format!("{:.3}", duration_seconds.max(1.0)))
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("20")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    command
}

fn run_ffmpeg_twitter_post_compose_process(
    app: &tauri::AppHandle,
    mut command: Command,
    output_dir: &Path,
    output_path: &Path,
    started_at_ms: u128,
    duration_seconds: f64,
    stop_generation: u64,
) -> Result<(), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("ffmpeg gönderi MP4 komutu başlatılamadı: {}", err))?;

    let pid = child.id();
    mark_active_process(pid, output_dir, started_at_ms);

    if let Err(err) = check_download_stop_since(stop_generation) {
        let _ = kill_process_tree(pid);
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(output_path);
        let _ = take_finished_process_request(pid);
        return Err(err);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stderr_text = Arc::new(Mutex::new(String::new()));
    let duration = duration_seconds.max(1.0);
    let last_progress_ms = Arc::new(AtomicU64::new(now_ms() as u64));

    let stdout_handle = if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let last_progress_ms_clone = Arc::clone(&last_progress_ms);

        Some(thread::spawn(move || {
            read_process_lines_lossy(stdout, |line| {
                if let Some(out_time_us) = parse_ffmpeg_out_time_microseconds(&line) {
                    last_progress_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                    let seconds = out_time_us as f64 / 1_000_000.0;
                    let ratio = (seconds / duration).clamp(0.0, 1.0);
                    let percent = (85.0 + 14.0 * ratio).clamp(85.0, 99.0);
                    emit_progress(
                        &app_clone,
                        Some(percent),
                        None,
                        None,
                        None,
                        "MP4 oluşturuluyor...",
                        line,
                    );
                } else if line.starts_with("progress=end") {
                    last_progress_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                    emit_progress(
                        &app_clone,
                        Some(99.0),
                        None,
                        None,
                        None,
                        "MP4 oluşturuluyor...",
                        line,
                    );
                }
            });
        }))
    } else {
        None
    };

    let stderr_handle = if let Some(stderr) = stderr {
        let stderr_text_clone = Arc::clone(&stderr_text);

        Some(thread::spawn(move || {
            read_process_lines_lossy(stderr, |line| {
                if let Ok(mut text) = stderr_text_clone.lock() {
                    text.push_str(&line);
                    text.push('\n');
                }
            });
        }))
    } else {
        None
    };

    let started = Instant::now();
    let total_timeout = Duration::from_secs((180.0 + duration * 8.0).ceil() as u64);
    let no_progress_timeout = Duration::from_secs(90);
    let mut watchdog_error: Option<String> = None;
    let mut stop_error: Option<String> = None;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let elapsed = started.elapsed();
                let last_progress_elapsed = Duration::from_millis(
                    (now_ms() as u64).saturating_sub(last_progress_ms.load(Ordering::Relaxed)),
                );

                if elapsed >= total_timeout {
                    watchdog_error = Some(format!(
                        "ffmpeg gönderi MP4 işlemi toplam zaman sınırını aştı ve durduruldu. Sınır: {} sn",
                        total_timeout.as_secs()
                    ));
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    break child.wait().map_err(|err| {
                        format!("Gönderi MP4 işlemi durdurulurken hata oluştu: {}", err)
                    })?;
                }

                if last_progress_elapsed >= no_progress_timeout {
                    watchdog_error = Some(format!(
                        "ffmpeg gönderi MP4 işlemi ilerleme üretmedi ve durduruldu. İlerleme yok: {} sn",
                        no_progress_timeout.as_secs()
                    ));
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    break child.wait().map_err(|err| {
                        format!("Gönderi MP4 işlemi durdurulurken hata oluştu: {}", err)
                    })?;
                }

                if let Err(err) = check_download_stop_since(stop_generation) {
                    stop_error = Some(err);
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    break child.wait().map_err(|err| {
                        format!("Gönderi MP4 işlemi durdurulurken hata oluştu: {}", err)
                    })?;
                }

                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => {
                let _ = kill_process_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Gönderi MP4 işlemi durumu okunamadı: {}", err));
            }
        }
    };

    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }

    if let Some(handle) = stderr_handle {
        let _ = handle.join();
    }

    let finished_process = take_finished_process_request(pid);

    if let Some(active) = finished_process {
        match active.request {
            Some(DownloadStopRequest::Pause) => {
                let _ = fs::remove_file(output_path);
                return Err(PAUSED_SIGNAL.to_string());
            }
            Some(DownloadStopRequest::Cancel) => {
                let _ = fs::remove_file(output_path);
                cleanup_incomplete_downloads(&active.output_dir, active.started_at_ms);
                return Err(CANCELLED_SIGNAL.to_string());
            }
            None => {}
        }
    }

    if let Some(err) = watchdog_error {
        let _ = fs::remove_file(output_path);
        return Err(err);
    }

    if let Some(err) = stop_error {
        let _ = fs::remove_file(output_path);
        return Err(err);
    }

    if !status.success() {
        let _ = fs::remove_file(output_path);
        let error_message = stderr_text
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default();

        return Err(if error_message.trim().is_empty() {
            format!("Gönderi MP4 çıktısı oluşturulamadı. Status: {}", status)
        } else {
            error_message
        });
    }

    if !output_path.is_file() || file_size(output_path).unwrap_or(0) < 1024 {
        let _ = fs::remove_file(output_path);
        return Err("Gönderi MP4 çıktısı oluşturulamadı veya boş çıktı üretildi.".to_string());
    }

    Ok(())
}

fn ytdlp_print_first_line(
    tools: &RuntimeTools,
    clean_url: &str,
    args: &[&str],
) -> Result<String, String> {
    let mut command = ytdlp_command(&tools.yt_dlp);
    prepend_path(&mut command, &tools.ffmpeg_dir);

    command
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--socket-timeout")
        .arg("10")
        .args(args);
    let _cookie_file = add_registered_ytdlp_cookies(&mut command, clean_url)?;
    command.arg(clean_url);

    let output = capture_command_with_timeout(command, Duration::from_secs(12))
        .map_err(|err| format!("yt-dlp komutu zaman aşımı/başlatma hatası: {}", err))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
        return Err(if combined.is_empty() {
            "yt-dlp çıktı üretmeden başarısız oldu.".to_string()
        } else {
            combined
        });
    }

    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.to_string())
        .ok_or_else(|| "yt-dlp beklenen çıktıyı üretmedi.".to_string())
}

fn ytdlp_print_title(tools: &RuntimeTools, clean_url: &str) -> String {
    ytdlp_print_first_line(tools, clean_url, &["--print", "%(title)s"])
        .unwrap_or_else(|_| "MediaDrop Clip".to_string())
}

fn ytdlp_stream_urls(
    yt_dlp: &Path,
    clean_url: &str,
    selector: &str,
    extractor_args: Option<&str>,
    force_ipv4: bool,
    force_ipv6: bool,
    timeout: Duration,
) -> Result<Vec<String>, String> {
    let mut command = ytdlp_command(yt_dlp);
    if let Some(tool_dir) = yt_dlp.parent() {
        prepend_path(&mut command, tool_dir);
    }

    command
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--socket-timeout")
        .arg("10");

    if force_ipv4 {
        command.arg("--force-ipv4");
    }

    if force_ipv6 {
        command.arg("--force-ipv6");
    }

    if let Some(args) = extractor_args {
        if !args.trim().is_empty() {
            command.arg("--extractor-args").arg(args);
        }
    }

    let _cookie_file = add_registered_ytdlp_cookies(&mut command, clean_url)?;
    command.arg("-g").arg("-f").arg(selector);
    let youtube_info_file = add_ytdlp_media_source(&mut command, clean_url)?;
    let used_cached_analysis = youtube_info_file.is_some();

    let output = match capture_command_with_timeout(command, timeout) {
        Ok(output) => output,
        Err(err) => {
            if used_cached_analysis {
                invalidate_youtube_analysis(clean_url);
            }
            return Err(format!(
                "yt-dlp stream çözme zaman aşımı/başlatma hatası: {}",
                err
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        if used_cached_analysis {
            invalidate_youtube_analysis(clean_url);
        }
        let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
        return Err(if combined.is_empty() {
            "yt-dlp stream URL çözemedi.".to_string()
        } else {
            combined
        });
    }

    let urls = stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    if urls.is_empty() {
        if used_cached_analysis {
            invalidate_youtube_analysis(clean_url);
        }
        Err("yt-dlp stream URL üretmedi.".to_string())
    } else {
        Ok(urls)
    }
}

fn youtube_preview_attempt_timeout(elapsed: Duration) -> Option<Duration> {
    YOUTUBE_PREVIEW_RESOLVE_BUDGET
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(YOUTUBE_PREVIEW_ATTEMPT_TIMEOUT))
}

fn youtube_preview_selector(quality: &str) -> String {
    // Önce tek dosyalı progressive MP4, yoksa native oynatıcı için ayrı video + ses URL'leri.
    let height = quality_height_limit(quality)
        .unwrap_or(1080)
        .clamp(144, 1080);

    format!(
        "best[height<={0}][ext=mp4][vcodec!=none][acodec!=none]/22/18/best[height<={0}][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]/bestvideo[height<={0}][ext=mp4]+bestaudio[ext=m4a]/bestvideo[height<={0}]+bestaudio",
        height
    )
}

fn youtube_hls_clip_height_limit(quality: &str) -> Option<u32> {
    // YouTube'un sesli HLS/m3u8 akışı çoğu videoda en fazla 1080p verir.
    // 1440p/4K seçilirse hızlı klip modu önce 1080p HLS arar; tam kalite DASH
    // klip ayrı bir segment downloader gerektirir.
    quality_height_limit(quality).map(|height| height.min(1080))
}

fn youtube_hls_clip_quality_label(quality: &str) -> String {
    match youtube_hls_clip_height_limit(quality) {
        Some(2160) => "4K".to_string(),
        Some(1440) => "2K".to_string(),
        Some(1080) => "1080p".to_string(),
        Some(720) => "720p".to_string(),
        Some(480) => "480p".to_string(),
        Some(360) => "360p".to_string(),
        Some(height) => format!("{}p", height),
        None => "HLS".to_string(),
    }
}

fn youtube_hls_clip_selector(quality: &str) -> String {
    // YouTube HLS'yi çoğu zaman ayrı video ve ses akışları olarak sunar. İkisini de
    // HLS'den seçmek, --download-sections'ın yalnızca ilgili segmentleri çekmesini sağlar.
    match youtube_hls_clip_height_limit(quality) {
        Some(height) => format!(
            "bestvideo[protocol*=m3u8][height<={0}]+bestaudio[protocol*=m3u8]/best[protocol*=m3u8][height<={0}][vcodec!=none][acodec!=none]/bestvideo[protocol*=m3u8][height<={0}]+bestaudio/best[protocol*=m3u8][height<={0}]",
            height
        ),
        None => "bestvideo[protocol*=m3u8]+bestaudio[protocol*=m3u8]/best[protocol*=m3u8][vcodec!=none][acodec!=none]/bestvideo[protocol*=m3u8]+bestaudio/best[protocol*=m3u8]".to_string(),
    }
}

fn make_youtube_hls_clip_attempts(quality: &str) -> Vec<DownloadAttempt> {
    let selector = youtube_hls_clip_selector(quality);
    let mut attempts = Vec::new();

    let mut default_ipv4 = download_attempt("Klip modu / HLS segment kesimi", &selector);
    default_ipv4.force_ipv4 = true;
    attempts.push(default_ipv4);

    let mut web = download_attempt("Klip modu / HLS web client", &selector);
    web.force_ipv4 = true;
    web.extractor_args = Some("youtube:player_client=web".to_string());
    attempts.push(web);

    let mut mweb = download_attempt("Klip modu / HLS mweb client", &selector);
    mweb.force_ipv4 = true;
    mweb.extractor_args = Some("youtube:player_client=mweb".to_string());
    attempts.push(mweb);

    let mut ios = download_attempt("Klip modu / HLS iOS client", &selector);
    ios.force_ipv4 = true;
    ios.extractor_args = Some("youtube:player_client=ios".to_string());
    attempts.push(ios);

    let mut android = download_attempt("Klip modu / HLS Android client", &selector);
    android.force_ipv4 = true;
    android.extractor_args = Some("youtube:player_client=android".to_string());
    attempts.push(android);

    let mut normal_dns = download_attempt("Klip modu / HLS normal DNS", &selector);
    normal_dns.force_ipv4 = false;
    attempts.push(normal_dns);

    attempts
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipEncodeMode {
    Copy,
    RemoteCopy,
    Nvenc,
    X264,
}

impl ClipEncodeMode {
    fn label(self) -> &'static str {
        match self {
            ClipEncodeMode::Copy => "hızlı kopyalama",
            ClipEncodeMode::RemoteCopy => "yüksek kalite parça indirme",
            ClipEncodeMode::Nvenc => "GPU encode",
            ClipEncodeMode::X264 => "CPU encode",
        }
    }

    fn progress_phase(self) -> &'static str {
        match self {
            ClipEncodeMode::Copy => "Klip hızlı kesiliyor...",
            ClipEncodeMode::RemoteCopy => "Yüksek kalite parça indiriliyor...",
            ClipEncodeMode::Nvenc => "Klip GPU ile işleniyor...",
            ClipEncodeMode::X264 => "Klip CPU ile işleniyor...",
        }
    }

    fn no_progress_timeout(self) -> Duration {
        match self {
            ClipEncodeMode::Copy => Duration::from_secs(35),
            ClipEncodeMode::RemoteCopy => Duration::from_secs(90),
            ClipEncodeMode::Nvenc => Duration::from_secs(45),
            ClipEncodeMode::X264 => Duration::from_secs(60),
        }
    }

    fn total_timeout(self, duration_seconds: f64) -> Duration {
        let clip_seconds = duration_seconds.max(1.0);
        let seconds = match self {
            ClipEncodeMode::Copy => 90.0 + clip_seconds * 1.5,
            ClipEncodeMode::RemoteCopy => 240.0 + clip_seconds * 8.0,
            ClipEncodeMode::Nvenc => 120.0 + clip_seconds * 3.0,
            ClipEncodeMode::X264 => 180.0 + clip_seconds * 6.0,
        };

        Duration::from_secs(seconds.clamp(60.0, 900.0) as u64)
    }
}

fn hls_clean_clip_encode_modes() -> [ClipEncodeMode; 3] {
    // HLS section clips can start on a non-clean timestamp/keyframe boundary.
    // Re-encoding only the short downloaded segment fixes the common 1-2 second
    // stutter at the beginning without downloading the full source video.
    [
        ClipEncodeMode::Nvenc,
        ClipEncodeMode::X264,
        ClipEncodeMode::Copy,
    ]
}

fn parse_ffmpeg_out_time_microseconds(line: &str) -> Option<u64> {
    let raw = line
        .strip_prefix("out_time_ms=")
        .or_else(|| line.strip_prefix("out_time_us="))?
        .trim();

    raw.parse::<u64>().ok()
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg_clip_process_with_progress_range(
    app: &tauri::AppHandle,
    mut command: Command,
    output_dir: &Path,
    output_path: &Path,
    started_at_ms: u128,
    duration_seconds: f64,
    encode_mode: ClipEncodeMode,
    progress_start: f64,
    progress_end: f64,
) -> Result<(), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("ffmpeg klip komutu başlatılamadı: {}", err))?;

    let pid = child.id();
    mark_active_process(pid, output_dir, started_at_ms);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stderr_text = Arc::new(Mutex::new(String::new()));
    let duration = duration_seconds.max(1.0);
    let progress_start = progress_start.clamp(0.0, 99.0);
    let progress_end = progress_end.clamp(progress_start, 100.0);
    let last_progress_ms = Arc::new(AtomicU64::new(now_ms() as u64));

    let stdout_handle = if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let last_progress_ms_clone = Arc::clone(&last_progress_ms);
        let phase = encode_mode.progress_phase().to_string();

        Some(thread::spawn(move || {
            read_process_lines_lossy(stdout, |line| {
                if let Some(out_time_us) = parse_ffmpeg_out_time_microseconds(&line) {
                    last_progress_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                    let seconds = out_time_us as f64 / 1_000_000.0;
                    let ratio = (seconds / duration).clamp(0.0, 1.0);
                    let percent = (progress_start + (progress_end - progress_start) * ratio)
                        .clamp(progress_start, progress_end.min(99.0));
                    emit_progress(
                        &app_clone,
                        Some(percent),
                        None,
                        None,
                        None,
                        phase.clone(),
                        line,
                    );
                } else if line.starts_with("progress=end") {
                    last_progress_ms_clone.store(now_ms() as u64, Ordering::Relaxed);
                    emit_progress(
                        &app_clone,
                        Some(progress_end.min(99.0)),
                        None,
                        None,
                        None,
                        "Klip tamamlanıyor...",
                        line,
                    );
                }
            });
        }))
    } else {
        None
    };

    let stderr_handle = if let Some(stderr) = stderr {
        let stderr_text_clone = Arc::clone(&stderr_text);

        Some(thread::spawn(move || {
            read_process_lines_lossy(stderr, |line| {
                if let Ok(mut text) = stderr_text_clone.lock() {
                    text.push_str(&line);
                    text.push('\n');
                }
            });
        }))
    } else {
        None
    };

    let started = Instant::now();
    let total_timeout = encode_mode.total_timeout(duration_seconds);
    let no_progress_timeout = encode_mode.no_progress_timeout();
    let mut watchdog_error: Option<String> = None;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let elapsed = started.elapsed();
                let last_progress_elapsed = Duration::from_millis(
                    (now_ms() as u64).saturating_sub(last_progress_ms.load(Ordering::Relaxed)),
                );

                if elapsed >= total_timeout {
                    watchdog_error = Some(format!(
                        "ffmpeg klip işlemi toplam zaman sınırını aştı ve durduruldu. Mod: {}, sınır: {} sn",
                        encode_mode.label(),
                        total_timeout.as_secs()
                    ));
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    break child.wait().map_err(|err| {
                        format!("Klip işlemi durdurulurken hata oluştu: {}", err)
                    })?;
                }

                if last_progress_elapsed >= no_progress_timeout {
                    watchdog_error = Some(format!(
                        "ffmpeg klip işlemi ilerleme üretmedi ve durduruldu. Mod: {}, ilerleme yok: {} sn",
                        encode_mode.label(),
                        no_progress_timeout.as_secs()
                    ));
                    let _ = kill_process_tree(pid);
                    let _ = child.kill();
                    break child.wait().map_err(|err| {
                        format!("Klip işlemi durdurulurken hata oluştu: {}", err)
                    })?;
                }

                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => {
                let _ = kill_process_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Klip işlemi durumu okunamadı: {}", err));
            }
        }
    };

    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }

    if let Some(handle) = stderr_handle {
        let _ = handle.join();
    }

    let finished_process = take_finished_process_request(pid);

    if let Some(active) = finished_process {
        match active.request {
            Some(DownloadStopRequest::Pause) => {
                let _ = fs::remove_file(output_path);
                return Err(PAUSED_SIGNAL.to_string());
            }
            Some(DownloadStopRequest::Cancel) => {
                let _ = fs::remove_file(output_path);
                cleanup_incomplete_downloads(&active.output_dir, active.started_at_ms);
                return Err(CANCELLED_SIGNAL.to_string());
            }
            None => {}
        }
    }

    if let Some(err) = watchdog_error {
        let _ = fs::remove_file(output_path);
        return Err(err);
    }

    if !status.success() {
        let _ = fs::remove_file(output_path);
        let error_message = stderr_text
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default();

        return Err(if error_message.trim().is_empty() {
            format!(
                "ffmpeg klip işlemi başarısız oldu. Mod: {}. Status: {}",
                encode_mode.label(),
                status
            )
        } else {
            error_message
        });
    }

    if !output_path.is_file() || file_size(output_path).unwrap_or(0) < 1024 {
        let _ = fs::remove_file(output_path);
        return Err("Klip dosyası oluşturulamadı veya boş çıktı üretildi.".to_string());
    }

    Ok(())
}

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn media_probe_value(tools: &RuntimeTools, output_path: &Path) -> Result<serde_json::Value, String> {
    let ffprobe = tools.ffmpeg_dir.join("ffprobe.exe");
    let mut command = hidden_command(ffprobe);

    command
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(output_path);

    let output = capture_command_with_timeout(command, Duration::from_secs(12))
        .map_err(|err| format!("ffprobe doğrulama zaman aşımı/başlatma hatası: {}", err))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
        return Err(if combined.is_empty() {
            "ffprobe klip doğrulaması başarısız oldu.".to_string()
        } else {
            combined
        });
    }

    serde_json::from_str::<serde_json::Value>(&stdout)
        .map_err(|err| format!("ffprobe JSON okunamadı: {}", err))
}

fn validate_media_probe_value(
    value: &serde_json::Value,
    expected_height: Option<u32>,
    expected_duration: Option<f64>,
) -> Result<(), String> {
    let streams = value
        .get("streams")
        .and_then(|item| item.as_array())
        .ok_or_else(|| "ffprobe stream bilgisi üretmedi.".to_string())?;

    let video_stream = streams
        .iter()
        .find(|stream| {
            stream
                .get("codec_type")
                .and_then(|item| item.as_str())
                .map(|item| item == "video")
                .unwrap_or(false)
        })
        .ok_or_else(|| "Çıktıda video stream bulunamadı.".to_string())?;

    let codec = video_stream
        .get("codec_name")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();

    if codec.is_empty() || codec == "none" {
        return Err("Çıktıda geçerli video codec bulunamadı.".to_string());
    }

    let actual_height = video_stream
        .get("height")
        .and_then(|item| item.as_u64())
        .unwrap_or(0) as u32;

    if actual_height == 0 {
        return Err("Çıktı video yüksekliği okunamadı.".to_string());
    }

    if let Some(expected) = expected_height {
        if actual_height + 2 < expected {
            return Err(format!(
                "Çıktı beklenen kalitenin altında. Beklenen: {}p, gerçek: {}p",
                expected, actual_height
            ));
        }
    }

    let audio_stream = streams.iter().find(|stream| {
        stream
            .get("codec_type")
            .and_then(|item| item.as_str())
            .map(|item| item == "audio")
            .unwrap_or(false)
    });

    let audio_stream = audio_stream.ok_or_else(|| "Çıktıda ses stream bulunamadı.".to_string())?;
    let audio_codec = audio_stream
        .get("codec_name")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();

    if audio_codec.is_empty() || audio_codec == "none" {
        return Err("Çıktıda geçerli ses codec bulunamadı.".to_string());
    }

    let format_duration = value
        .get("format")
        .and_then(|format| format.get("duration"))
        .and_then(json_f64);

    let stream_duration = video_stream.get("duration").and_then(json_f64);
    let actual_duration = format_duration.or(stream_duration).unwrap_or(0.0);
    if actual_duration <= 0.0 {
        return Err("Çıktı süresi okunamadı veya sıfır görünüyor.".to_string());
    }

    if let Some(expected) = expected_duration.map(|duration| duration.max(1.0)) {
        if (actual_duration - expected).abs() > 2.5 {
            return Err(format!(
                "Çıktı süresi beklenenden farklı. Beklenen: {:.2} sn, gerçek: {:.2} sn",
                expected, actual_duration
            ));
        }
    }

    if let Some(audio_duration) = audio_stream.get("duration").and_then(json_f64) {
        let video_duration = stream_duration.unwrap_or(actual_duration);
        if audio_duration > 0.0 && (audio_duration - video_duration).abs() > 2.5 {
            return Err(format!(
                "Çıktı ses/video süreleri uyuşmuyor. Video: {:.2} sn, ses: {:.2} sn",
                video_duration, audio_duration
            ));
        }
    }

    let video_start = video_stream.get("start_time").and_then(json_f64);
    let audio_start = audio_stream.get("start_time").and_then(json_f64);

    if let (Some(video_start), Some(audio_start)) = (video_start, audio_start) {
        if (video_start - audio_start).abs() > 1.0 {
            return Err(format!(
                "Çıktı ses/video başlangıçları uyuşmuyor. Video: {:.2} sn, ses: {:.2} sn",
                video_start, audio_start
            ));
        }
    }

    Ok(())
}

fn validate_video_output(
    tools: &RuntimeTools,
    output_path: &Path,
    expected_height: Option<u32>,
    expected_duration: Option<f64>,
) -> Result<(), String> {
    if !output_path.is_file() {
        return Err("Video dosyası bulunamadı.".to_string());
    }

    let size = file_size(output_path).unwrap_or(0);
    if size < 100 * 1024 {
        return Err(format!("Video dosyası çok küçük görünüyor: {} bayt", size));
    }

    let value = media_probe_value(tools, output_path)?;
    validate_media_probe_value(&value, expected_height, expected_duration)
}

fn validate_clip_output(
    tools: &RuntimeTools,
    output_path: &Path,
    expected_height: Option<u32>,
    expected_duration: f64,
) -> Result<(), String> {
    validate_video_output(tools, output_path, expected_height, Some(expected_duration))
}

fn add_ffmpeg_remote_input_args(
    command: &mut Command,
    youtube_headers: &str,
    stream_url: &str,
    seek_seconds: f64,
) {
    command
        .arg("-ss")
        .arg(format!("{:.3}", seek_seconds))
        .arg("-rw_timeout")
        .arg("15000000")
        .arg("-reconnect")
        .arg("1")
        .arg("-reconnect_streamed")
        .arg("1")
        .arg("-reconnect_delay_max")
        .arg("5")
        .arg("-headers")
        .arg(youtube_headers)
        .arg("-user_agent")
        .arg("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .arg("-i")
        .arg(stream_url);
}

fn true_quality_clip_requested(quality: &str) -> bool {
    quality_height_limit(quality)
        .map(|height| height > 1080)
        .unwrap_or(false)
}

fn youtube_true_quality_video_selector(selected_format_id: &str, quality: &str) -> String {
    let clean = selected_format_id.trim();

    if !clean.is_empty() {
        return clean.to_string();
    }

    match quality_height_limit(quality) {
        Some(height) => format!(
            "bestvideo[height={0}]/bestvideo[height<={0}][height>1080]/bestvideo[height<={0}]",
            height
        ),
        None => "bestvideo[height>1080]/bestvideo".to_string(),
    }
}

fn youtube_true_quality_audio_selector() -> String {
    "bestaudio[ext=m4a]/140/bestaudio".to_string()
}

#[derive(Clone)]
struct TrueQualityClipAttempt {
    label: String,
    video_selector: String,
    audio_selector: String,
    extractor_args: Option<&'static str>,
    force_ipv4: bool,
    force_ipv6: bool,
}

fn make_true_quality_clip_attempts(
    selected_format_id: &str,
    quality: &str,
) -> Vec<TrueQualityClipAttempt> {
    let selected_selector = youtube_true_quality_video_selector(selected_format_id, quality);
    let audio_selector = youtube_true_quality_audio_selector();
    let height_selector = quality_height_limit(quality)
        .map(|height| {
            format!(
                "bestvideo[height={0}]/bestvideo[height<={0}][height>1080]/bestvideo[height<={0}]",
                height
            )
        })
        .unwrap_or_else(|| "bestvideo[height>1080]/bestvideo".to_string());

    let mut attempts = Vec::new();

    for (label, selector, extractor_args, force_ipv4, force_ipv6) in [
        (
            "True Quality / selected format / visionos",
            selected_selector.clone(),
            Some("youtube:player_client=visionos"),
            true,
            false,
        ),
        (
            "True Quality / selected format / default IPv4",
            selected_selector.clone(),
            None,
            true,
            false,
        ),
        (
            "True Quality / selected format / web",
            selected_selector.clone(),
            Some("youtube:player_client=web"),
            true,
            false,
        ),
        (
            "True Quality / selected format / mweb",
            selected_selector.clone(),
            Some("youtube:player_client=mweb"),
            true,
            false,
        ),
        (
            "True Quality / same height / visionos",
            height_selector.clone(),
            Some("youtube:player_client=visionos"),
            true,
            false,
        ),
        (
            "True Quality / same height / default IPv4",
            height_selector.clone(),
            None,
            true,
            false,
        ),
        (
            "True Quality / same height / web",
            height_selector.clone(),
            Some("youtube:player_client=web"),
            true,
            false,
        ),
        (
            "True Quality / same height / normal DNS",
            height_selector,
            None,
            false,
            false,
        ),
    ] {
        attempts.push(TrueQualityClipAttempt {
            label: label.to_string(),
            video_selector: selector,
            audio_selector: audio_selector.clone(),
            extractor_args,
            force_ipv4,
            force_ipv6,
        });
    }

    attempts
}

fn true_quality_merge_encode_modes() -> [ClipEncodeMode; 3] {
    // Copy is the fastest path and preserves AV1/WebM-style high quality when the
    // container accepts it. Validation catches bad cuts; NVENC/x264 remain fallbacks.
    [
        ClipEncodeMode::Copy,
        ClipEncodeMode::Nvenc,
        ClipEncodeMode::X264,
    ]
}

fn build_ffmpeg_remote_single_stream_clip_command(
    tools: &RuntimeTools,
    stream_url: &str,
    output_path: &Path,
    clip_range: ClipRange,
    is_audio: bool,
) -> Command {
    let ffmpeg = tools.ffmpeg_dir.join("ffmpeg.exe");
    let duration = (clip_range.end - clip_range.start).max(1.0);
    let mut command = hidden_command(ffmpeg);

    prepend_path(&mut command, &tools.ffmpeg_dir);

    let youtube_headers = "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36\r\nReferer: https://www.youtube.com/\r\nOrigin: https://www.youtube.com\r\n";

    command.arg("-hide_banner").arg("-y");

    add_ffmpeg_remote_input_args(&mut command, youtube_headers, stream_url, clip_range.start);

    command.arg("-t").arg(format!("{:.3}", duration));

    if is_audio {
        command
            .arg("-vn")
            .arg("-map")
            .arg("0:a:0?")
            .arg("-c")
            .arg("copy");
    } else {
        command
            .arg("-an")
            .arg("-map")
            .arg("0:v:0")
            .arg("-c")
            .arg("copy");
    }

    command
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    command
}

#[allow(clippy::too_many_arguments)]
fn download_true_quality_remote_part(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    stream_url: &str,
    output_path: &Path,
    temp_dir: &Path,
    clip_range: ClipRange,
    is_audio: bool,
    started_at_ms: u128,
    progress_start: f64,
    progress_end: f64,
) -> Result<(), String> {
    let duration = (clip_range.end - clip_range.start).max(1.0);
    let command = build_ffmpeg_remote_single_stream_clip_command(
        tools,
        stream_url,
        output_path,
        clip_range,
        is_audio,
    );

    run_ffmpeg_clip_process_with_progress_range(
        app,
        command,
        temp_dir,
        output_path,
        started_at_ms,
        duration,
        ClipEncodeMode::RemoteCopy,
        progress_start,
        progress_end,
    )?;

    let min_size = if is_audio { 16 * 1024 } else { 128 * 1024 };
    let size = file_size(output_path).unwrap_or(0);

    if !output_path.is_file() || size < min_size {
        let _ = fs::remove_file(output_path);
        return Err(format!(
            "Yüksek kalite {} parçası boş/çok küçük çıktı: {} bayt",
            if is_audio { "ses" } else { "video" },
            size
        ));
    }

    Ok(())
}

fn build_ffmpeg_merge_true_quality_clip_command(
    tools: &RuntimeTools,
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    trim_range: ClipRange,
    encode_mode: ClipEncodeMode,
) -> Command {
    let ffmpeg = tools.ffmpeg_dir.join("ffmpeg.exe");
    let duration = (trim_range.end - trim_range.start).max(1.0);
    let seek_margin = if encode_mode == ClipEncodeMode::Copy {
        0.0
    } else {
        2.0
    };
    let preseek = (trim_range.start - seek_margin).max(0.0);
    let fine_seek = (trim_range.start - preseek).max(0.0);

    let mut command = hidden_command(ffmpeg);
    prepend_path(&mut command, &tools.ffmpeg_dir);

    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-fflags")
        .arg("+genpts")
        .arg("-ss")
        .arg(format!("{:.3}", preseek))
        .arg("-i")
        .arg(video_path)
        .arg("-ss")
        .arg(format!("{:.3}", preseek))
        .arg("-i")
        .arg(audio_path)
        .arg("-ss")
        .arg(format!("{:.3}", fine_seek))
        .arg("-t")
        .arg(format!("{:.3}", duration))
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:a:0?");

    match encode_mode {
        ClipEncodeMode::Copy | ClipEncodeMode::RemoteCopy => {
            command
                .arg("-c")
                .arg("copy")
                .arg("-avoid_negative_ts")
                .arg("make_zero");
        }
        ClipEncodeMode::Nvenc => {
            command
                .arg("-c:v")
                .arg("h264_nvenc")
                .arg("-preset")
                .arg("p4")
                .arg("-cq")
                .arg("20")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("192k");
        }
        ClipEncodeMode::X264 => {
            command
                .arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg("veryfast")
                .arg("-crf")
                .arg("20")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("192k");
        }
    }

    command
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    command
}

#[allow(clippy::too_many_arguments)]
fn merge_true_quality_clip_parts(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    video_path: &Path,
    audio_path: &Path,
    final_path: &Path,
    download_dir: &Path,
    trim_range: ClipRange,
    expected_height: Option<u32>,
    expected_duration: f64,
    started_at_ms: u128,
) -> Result<(), String> {
    let mut errors = Vec::new();

    for encode_mode in true_quality_merge_encode_modes() {
        emit_simple_progress(
            app,
            Some(86.0),
            format!(
                "Yüksek kalite klip birleştiriliyor: {}",
                encode_mode.label()
            ),
        );

        let command = build_ffmpeg_merge_true_quality_clip_command(
            tools,
            video_path,
            audio_path,
            final_path,
            trim_range,
            encode_mode,
        );

        match run_ffmpeg_clip_process_with_progress_range(
            app,
            command,
            download_dir,
            final_path,
            started_at_ms,
            expected_duration,
            encode_mode,
            86.0,
            99.0,
        ) {
            Ok(()) => {
                match validate_clip_output(tools, final_path, expected_height, expected_duration) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        let _ = fs::remove_file(final_path);
                        errors.push(format!(
                            "[true-quality merge / {}] Doğrulama başarısız:\n{}",
                            encode_mode.label(),
                            sanitize_report_text(&err)
                        ));
                    }
                }
            }
            Err(err) => {
                if err == PAUSED_SIGNAL || err == CANCELLED_SIGNAL {
                    return Err(err);
                }

                let _ = fs::remove_file(final_path);
                errors.push(format!(
                    "[true-quality merge / {}] ffmpeg başarısız:\n{}",
                    encode_mode.label(),
                    sanitize_report_text(&err)
                ));
            }
        }
    }

    Err(format!(
        "Yüksek kalite video/ses parçalarından doğrulanmış klip üretilemedi.\n\n{}",
        errors.join("\n\n---\n\n")
    ))
}

#[derive(Clone, Debug)]
struct DashByteRange {
    start: u64,
    end: u64,
}

impl DashByteRange {
    fn len(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

#[derive(Clone, Debug)]
struct DashSidxReference {
    start_time: f64,
    end_time: f64,
    byte_start: u64,
    byte_end: u64,
}

#[derive(Clone, Debug)]
struct DashSidxIndex {
    header_end: u64,
    references: Vec<DashSidxReference>,
}

#[derive(Clone, Debug)]
struct DashRangePart {
    media_start: f64,
    bytes_written: u64,
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_be_u64(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn find_mp4_box(data: &[u8], box_type: &[u8; 4]) -> Option<usize> {
    if data.len() < 8 {
        return None;
    }

    for offset in 0..=data.len().saturating_sub(8) {
        if data.get(offset + 4..offset + 8) == Some(&box_type[..]) {
            let size = read_be_u32(data, offset).unwrap_or(0);
            if size >= 8 || size == 1 {
                return Some(offset);
            }
        }
    }

    None
}

fn mp4_box_size(data: &[u8], offset: usize) -> Option<(u64, usize)> {
    let size32 = read_be_u32(data, offset)? as u64;

    if size32 == 1 {
        let size64 = read_be_u64(data, offset + 8)?;
        if size64 < 16 {
            return None;
        }
        Some((size64, 16))
    } else if size32 >= 8 {
        Some((size32, 8))
    } else {
        None
    }
}

fn parse_sidx_index(data: &[u8]) -> Result<DashSidxIndex, String> {
    let Some(sidx_offset) = find_mp4_box(data, b"sidx") else {
        return Err("sidx box bulunamadı.".to_string());
    };

    let (sidx_size, header_len) =
        mp4_box_size(data, sidx_offset).ok_or_else(|| "sidx box boyutu okunamadı.".to_string())?;

    let sidx_end_usize = sidx_offset
        .checked_add(sidx_size as usize)
        .ok_or_else(|| "sidx box boyutu taşma üretti.".to_string())?;

    if sidx_end_usize > data.len() {
        return Err(format!(
            "sidx box probe aralığına sığmadı. Gerekli: {} bayt, mevcut: {} bayt",
            sidx_end_usize,
            data.len()
        ));
    }

    let content_start = sidx_offset + header_len;
    let version = *data
        .get(content_start)
        .ok_or_else(|| "sidx version okunamadı.".to_string())?;

    let mut pos = content_start + 4; // version + flags
    pos += 4; // reference_ID

    let timescale =
        read_be_u32(data, pos).ok_or_else(|| "sidx timescale okunamadı.".to_string())?;
    pos += 4;

    if timescale == 0 {
        return Err("sidx timescale sıfır geldi.".to_string());
    }

    if version == 0 {
        pos += 4; // earliest_presentation_time
        pos += 4; // first_offset
    } else if version == 1 {
        pos += 8; // earliest_presentation_time
        pos += 8; // first_offset
    } else {
        return Err(format!("Desteklenmeyen sidx version: {}", version));
    }

    let first_offset = if version == 0 {
        // We already advanced over first_offset above; re-read it from the known offset.
        let first_offset_pos = content_start + 4 + 4 + 4 + 4;
        read_be_u32(data, first_offset_pos).unwrap_or(0) as u64
    } else {
        let first_offset_pos = content_start + 4 + 4 + 4 + 8;
        read_be_u64(data, first_offset_pos).unwrap_or(0)
    };

    pos += 2; // reserved

    let reference_count = read_be_u16(data, pos)
        .ok_or_else(|| "sidx reference_count okunamadı.".to_string())?
        as usize;
    pos += 2;

    if reference_count == 0 {
        return Err("sidx reference listesi boş.".to_string());
    }

    let sidx_end = sidx_offset as u64 + sidx_size;
    let first_segment_offset = sidx_end
        .checked_add(first_offset)
        .ok_or_else(|| "sidx first segment offset taşma üretti.".to_string())?;

    let mut references = Vec::with_capacity(reference_count);
    let mut current_byte = first_segment_offset;
    let mut current_time = 0.0f64;

    for _ in 0..reference_count {
        let raw_reference =
            read_be_u32(data, pos).ok_or_else(|| "sidx reference size okunamadı.".to_string())?;
        pos += 4;

        let reference_type = (raw_reference >> 31) & 1;
        let reference_size = (raw_reference & 0x7FFF_FFFF) as u64;

        let subsegment_duration = read_be_u32(data, pos)
            .ok_or_else(|| "sidx subsegment duration okunamadı.".to_string())?
            as u64;
        pos += 4;

        pos += 4; // SAP flags

        if reference_type != 0 {
            return Err("sidx nested reference içeriyor; bu sürümde desteklenmiyor.".to_string());
        }

        if reference_size == 0 || subsegment_duration == 0 {
            continue;
        }

        let duration_seconds = subsegment_duration as f64 / timescale as f64;
        let byte_end = current_byte
            .checked_add(reference_size)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| "sidx byte aralığı taşma üretti.".to_string())?;

        references.push(DashSidxReference {
            start_time: current_time,
            end_time: current_time + duration_seconds,
            byte_start: current_byte,
            byte_end,
        });

        current_byte = current_byte
            .checked_add(reference_size)
            .ok_or_else(|| "sidx byte offset taşma üretti.".to_string())?;
        current_time += duration_seconds;
    }

    if references.is_empty() {
        return Err("sidx parse edildi ama kullanılabilir reference bulunamadı.".to_string());
    }

    Ok(DashSidxIndex {
        header_end: first_segment_offset,
        references,
    })
}

fn validate_http_content_range(
    value: &str,
    requested: &DashByteRange,
    allow_eof_shortening: bool,
) -> Result<DashByteRange, String> {
    if requested.end < requested.start {
        return Err("İstenen HTTP byte aralığı geçersiz.".to_string());
    }

    let mut parts = value.split_whitespace();
    let unit = parts.next().unwrap_or_default();
    let spec = parts.next().unwrap_or_default();
    if !unit.eq_ignore_ascii_case("bytes") || spec.is_empty() || parts.next().is_some() {
        return Err(format!("Content-Range başlığı geçersiz: {value}"));
    }

    let (bounds, total_text) = spec
        .split_once('/')
        .ok_or_else(|| format!("Content-Range toplam boyutu eksik: {value}"))?;
    let (start_text, end_text) = bounds
        .split_once('-')
        .ok_or_else(|| format!("Content-Range sınırları eksik: {value}"))?;
    let start = start_text
        .parse::<u64>()
        .map_err(|_| format!("Content-Range başlangıcı geçersiz: {value}"))?;
    let end = end_text
        .parse::<u64>()
        .map_err(|_| format!("Content-Range sonu geçersiz: {value}"))?;
    if end < start {
        return Err(format!("Content-Range ters byte aralığı içeriyor: {value}"));
    }

    let total = if total_text == "*" {
        None
    } else {
        Some(
            total_text
                .parse::<u64>()
                .map_err(|_| format!("Content-Range toplam boyutu geçersiz: {value}"))?,
        )
    };
    if total.is_some_and(|total| total == 0 || end >= total) {
        return Err(format!("Content-Range toplam boyutla uyuşmuyor: {value}"));
    }
    if start != requested.start || end > requested.end {
        return Err(format!(
            "Content-Range istenen aralıkla uyuşmuyor. İstenen: {}-{}, dönen: {}-{}",
            requested.start, requested.end, start, end
        ));
    }
    if end < requested.end {
        let reached_declared_eof = end.checked_add(1).is_some_and(|next| total == Some(next));
        if !allow_eof_shortening || !reached_declared_eof {
            return Err(format!(
                "Content-Range istenen aralıktan erken bitti. İstenen: {}-{}, dönen: {}-{}",
                requested.start, requested.end, start, end
            ));
        }
    }

    Ok(DashByteRange { start, end })
}

fn stream_http_range_body<R, W, F>(
    mut reader: R,
    writer: &mut W,
    expected_len: u64,
    mut on_progress: F,
) -> Result<u64, String>
where
    R: Read,
    W: Write,
    F: FnMut(u64) -> Result<(), String>,
{
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;

    while written < expected_len {
        let remaining = expected_len - written;
        let read_limit = remaining.min(buffer.len() as u64) as usize;
        let count = match reader.read(&mut buffer[..read_limit]) {
            Ok(0) => {
                return Err(format!(
                    "HTTP range gövdesi eksik: {written}/{expected_len} bayt"
                ))
            }
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("HTTP range gövdesi okunamadı: {error}")),
        };
        writer
            .write_all(&buffer[..count])
            .map_err(|error| format!("HTTP range dosyaya yazılamadı: {error}"))?;
        written = written.saturating_add(count as u64);
        on_progress(written)?;
    }

    let mut extra = [0_u8; 1];
    loop {
        match reader.read(&mut extra) {
            Ok(0) => break,
            Ok(_) => {
                return Err(format!(
                    "HTTP range gövdesi beklenenden fazla veri içeriyor: {expected_len} bayt"
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("HTTP range gövdesi doğrulanamadı: {error}")),
        }
    }

    Ok(written)
}

fn open_http_range_response(
    client: &reqwest::blocking::Client,
    url: &str,
    range: &DashByteRange,
    allow_eof_shortening: bool,
) -> Result<(reqwest::blocking::Response, DashByteRange), String> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| "HTTP range linki gecersiz.".to_string())?;
    if !media_url_host_allowed(&parsed) {
        return Err("HTTP range linki guvenli degil.".to_string());
    }
    let range_header = format!("bytes={}-{}", range.start, range.end);

    let response = client
        .get(parsed)
        .header(reqwest::header::RANGE, &range_header)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .header(reqwest::header::REFERER, "https://www.youtube.com/")
        .header("Origin", "https://www.youtube.com")
        .send()
        .map_err(|err| format!("HTTP range isteği başarısız oldu ({}): {}", range_header, err))?;

    let status = response.status();
    if status.as_u16() != 206 {
        return Err(format!(
            "Sunucu HTTP Range isteğine 206 yerine {} döndürdü. Range: {}",
            status, range_header
        ));
    }
    if !media_url_host_allowed(response.url()) {
        return Err("HTTP range yönlendirmesi güvenli değil.".to_string());
    }

    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| format!("HTTP 206 yanıtında Content-Range eksik: {range_header}"))?;
    let returned = validate_http_content_range(content_range, range, allow_eof_shortening)?;
    if response
        .content_length()
        .is_some_and(|length| length != returned.len())
    {
        return Err(format!(
            "HTTP range Content-Length değeri Content-Range ile uyuşmuyor: {range_header}"
        ));
    }

    Ok((response, returned))
}

fn fetch_http_range(
    client: &reqwest::blocking::Client,
    url: &str,
    range: &DashByteRange,
) -> Result<Vec<u8>, String> {
    let (response, returned) = open_http_range_response(client, url, range, true)?;
    let mut bytes = Vec::with_capacity(returned.len().min(16 * 1024 * 1024) as usize);
    stream_http_range_body(response, &mut bytes, returned.len(), |_| Ok(()))?;
    Ok(bytes)
}

fn dash_probe_sizes(is_audio: bool) -> Vec<u64> {
    if is_audio {
        vec![1024 * 1024, 2 * 1024 * 1024, 4 * 1024 * 1024]
    } else {
        vec![
            2 * 1024 * 1024,
            4 * 1024 * 1024,
            8 * 1024 * 1024,
            16 * 1024 * 1024,
        ]
    }
}

fn dash_probe_ranges(is_audio: bool) -> Vec<DashByteRange> {
    let mut start = 0;

    dash_probe_sizes(is_audio)
        .into_iter()
        .map(|target_size| {
            let range = DashByteRange {
                start,
                end: target_size.saturating_sub(1),
            };
            start = target_size;
            range
        })
        .collect()
}

fn load_dash_sidx_index(url: &str, is_audio: bool) -> Result<DashSidxIndex, String> {
    let mut last_error = String::new();
    let client = http_range_client(Duration::from_secs(45))?;
    let mut probe = Vec::new();

    for range in dash_probe_ranges(is_audio) {
        let probe_size = range.end.saturating_add(1);
        match fetch_http_range(&client, url, &range) {
            Ok(bytes) => {
                let complete_range = bytes.len() as u64 == range.len();
                probe.extend_from_slice(&bytes);

                match parse_sidx_index(&probe) {
                    Ok(index) => return Ok(index),
                    Err(err) => {
                        last_error = format!(
                            "Probe {} MB içinde sidx okunamadı: {}",
                            probe_size / 1024 / 1024,
                            err
                        );

                        if !complete_range {
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                last_error = format!(
                    "Probe {} MB indirilemedi: {}",
                    probe_size / 1024 / 1024,
                    err
                );
            }
        }
    }

    Err(last_error)
}

fn merge_dash_byte_ranges(mut ranges: Vec<DashByteRange>) -> Vec<DashByteRange> {
    if ranges.is_empty() {
        return ranges;
    }

    ranges.sort_by_key(|range| range.start);

    let mut merged: Vec<DashByteRange> = Vec::new();

    for range in ranges {
        if range.end < range.start {
            continue;
        }

        if let Some(last) = merged.last_mut() {
            if range.start <= last.end.saturating_add(1) {
                if range.end > last.end {
                    last.end = range.end;
                }
                continue;
            }
        }

        merged.push(range);
    }

    merged
}

fn select_sidx_ranges(
    index: &DashSidxIndex,
    clip_range: ClipRange,
) -> Result<(Vec<DashByteRange>, ClipRange), String> {
    let selected = index
        .references
        .iter()
        .filter(|reference| {
            reference.end_time > clip_range.start && reference.start_time < clip_range.end
        })
        .cloned()
        .collect::<Vec<_>>();

    if selected.is_empty() {
        let total_duration = index
            .references
            .last()
            .map(|reference| reference.end_time)
            .unwrap_or(0.0);

        return Err(format!(
            "sidx içinde seçilen zaman aralığına denk gelen parça bulunamadı. İstenen: {:.3}-{:.3}, indeks süresi: {:.3} sn",
            clip_range.start,
            clip_range.end,
            total_duration
        ));
    }

    let actual_start = selected
        .first()
        .map(|reference| reference.start_time)
        .unwrap_or(clip_range.start);
    let actual_end = selected
        .last()
        .map(|reference| reference.end_time)
        .unwrap_or(clip_range.end);

    let mut ranges = Vec::new();

    let header_end = index.header_end.saturating_sub(1);
    ranges.push(DashByteRange {
        start: 0,
        end: header_end,
    });

    for reference in selected {
        ranges.push(DashByteRange {
            start: reference.byte_start,
            end: reference.byte_end,
        });
    }

    Ok((
        merge_dash_byte_ranges(ranges),
        ClipRange {
            start: actual_start,
            end: actual_end,
        },
    ))
}

fn download_dash_ranges_to_file(
    app: &tauri::AppHandle,
    url: &str,
    output_path: &Path,
    ranges: &[DashByteRange],
    label: &str,
    progress_start: f64,
    progress_end: f64,
) -> Result<u64, String> {
    let total_bytes = ranges.iter().map(DashByteRange::len).sum::<u64>().max(1);
    let total_mb = total_bytes as f64 / 1024.0 / 1024.0;
    let mut written: u64 = 0;
    let client = http_range_client(Duration::from_secs(90))?;

    let mut file = fs::File::create(output_path)
        .map_err(|err| format!("DASH geçici dosyası oluşturulamadı: {}", err))?;
    let output_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let mut last_progress_at = Instant::now();

    let result = (|| {
        for (index, range) in ranges.iter().enumerate() {
            check_active_range_stop(output_dir, output_path)?;
            let (response, returned) = open_http_range_response(&client, url, range, false)?;
            let before_range = written;
            let range_written = stream_http_range_body(
                response,
                &mut file,
                returned.len(),
                |current_range_bytes| {
                    check_active_range_stop(output_dir, output_path)?;
                    if media_progress_update_due(last_progress_at.elapsed(), false) {
                        let current = before_range.saturating_add(current_range_bytes);
                        let ratio = (current as f64 / total_bytes as f64).clamp(0.0, 1.0);
                        let downloaded_mb = current as f64 / 1024.0 / 1024.0;
                        emit_progress(
                            app,
                            Some(progress_start + (progress_end - progress_start) * ratio),
                            Some(downloaded_mb),
                            Some(total_mb),
                            None,
                            format!(
                                "{} indiriliyor... ({}/{})",
                                label,
                                index + 1,
                                ranges.len()
                            ),
                            format!(
                                "HTTP Range {}-{} | {:.2}/{:.2} MB",
                                range.start, range.end, downloaded_mb, total_mb
                            ),
                        );
                        last_progress_at = Instant::now();
                    }
                    Ok(())
                },
            )?;
            written = written.saturating_add(range_written);

            let ratio = (written as f64 / total_bytes as f64).clamp(0.0, 1.0);
            let downloaded_mb = written as f64 / 1024.0 / 1024.0;
            emit_progress(
                app,
                Some(progress_start + (progress_end - progress_start) * ratio),
                Some(downloaded_mb),
                Some(total_mb),
                None,
                format!("{} indiriliyor... ({}/{})", label, index + 1, ranges.len()),
                format!(
                    "HTTP Range {}-{} | {:.2}/{:.2} MB",
                    range.start, range.end, downloaded_mb, total_mb
                ),
            );
            last_progress_at = Instant::now();
        }

        file.flush()
            .map_err(|err| format!("DASH geçici dosyası flush edilemedi: {}", err))?;
        Ok(written)
    })();

    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(output_path);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn download_true_quality_dash_range_part(
    app: &tauri::AppHandle,
    stream_url: &str,
    output_path: &Path,
    clip_range: ClipRange,
    is_audio: bool,
    progress_start: f64,
    progress_end: f64,
    started_at_ms: u128,
) -> Result<DashRangePart, String> {
    let output_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    mark_active_range(output_dir, started_at_ms);

    let result = (|| {
        let label = if is_audio {
            "True Quality ses byte-range"
        } else {
            "True Quality 2K/4K video byte-range"
        };

        emit_simple_progress(
            app,
            Some(progress_start),
            format!("{} indeksi okunuyor...", label),
        );

        let index = load_dash_sidx_index(stream_url, is_audio)?;
        check_active_range_stop(output_dir, output_path)?;

        let (ranges, media_range) = select_sidx_ranges(&index, clip_range)?;
        check_active_range_stop(output_dir, output_path)?;

        let bytes_written = download_dash_ranges_to_file(
            app,
            stream_url,
            output_path,
            &ranges,
            label,
            progress_start,
            progress_end,
        )?;
        check_active_range_stop(output_dir, output_path)?;

        let min_size = if is_audio { 16 * 1024 } else { 128 * 1024 };
        if !output_path.is_file() || file_size(output_path).unwrap_or(0) < min_size {
            let _ = fs::remove_file(output_path);
            return Err(format!(
                "{} çıktısı boş/çok küçük görünüyor: {} bayt",
                label,
                file_size(output_path).unwrap_or(0)
            ));
        }

        Ok(DashRangePart {
            media_start: media_range.start,
            bytes_written,
        })
    })();

    finish_active_range();
    result
}

#[allow(clippy::too_many_arguments)]
fn build_ffmpeg_merge_dash_range_clip_command(
    tools: &RuntimeTools,
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    video_offset: f64,
    audio_offset: f64,
    duration: f64,
    encode_mode: ClipEncodeMode,
) -> Command {
    let ffmpeg = tools.ffmpeg_dir.join("ffmpeg.exe");
    let mut command = hidden_command(ffmpeg);

    prepend_path(&mut command, &tools.ffmpeg_dir);

    let video_offset = video_offset.max(0.0);
    let audio_offset = audio_offset.max(0.0);
    let duration = duration.max(1.0);

    match encode_mode {
        ClipEncodeMode::Copy | ClipEncodeMode::RemoteCopy => {
            command
                .arg("-hide_banner")
                .arg("-y")
                .arg("-fflags")
                .arg("+genpts")
                .arg("-ss")
                .arg(format!("{:.3}", video_offset))
                .arg("-i")
                .arg(video_path)
                .arg("-ss")
                .arg(format!("{:.3}", audio_offset))
                .arg("-i")
                .arg(audio_path)
                .arg("-t")
                .arg(format!("{:.3}", duration))
                .arg("-map")
                .arg("0:v:0")
                .arg("-map")
                .arg("1:a:0?")
                .arg("-c")
                .arg("copy")
                .arg("-avoid_negative_ts")
                .arg("make_zero");
        }
        ClipEncodeMode::Nvenc => {
            command
                .arg("-hide_banner")
                .arg("-y")
                .arg("-fflags")
                .arg("+genpts")
                .arg("-i")
                .arg(video_path)
                .arg("-i")
                .arg(audio_path)
                .arg("-filter_complex")
                .arg(format!(
                    "[0:v:0]trim=start={:.3}:duration={:.3},setpts=PTS-STARTPTS[v];[1:a:0]atrim=start={:.3}:duration={:.3},asetpts=PTS-STARTPTS[a]",
                    video_offset,
                    duration,
                    audio_offset,
                    duration
                ))
                .arg("-map")
                .arg("[v]")
                .arg("-map")
                .arg("[a]")
                .arg("-c:v")
                .arg("h264_nvenc")
                .arg("-preset")
                .arg("p4")
                .arg("-cq")
                .arg("20")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("192k");
        }
        ClipEncodeMode::X264 => {
            command
                .arg("-hide_banner")
                .arg("-y")
                .arg("-fflags")
                .arg("+genpts")
                .arg("-i")
                .arg(video_path)
                .arg("-i")
                .arg(audio_path)
                .arg("-filter_complex")
                .arg(format!(
                    "[0:v:0]trim=start={:.3}:duration={:.3},setpts=PTS-STARTPTS[v];[1:a:0]atrim=start={:.3}:duration={:.3},asetpts=PTS-STARTPTS[a]",
                    video_offset,
                    duration,
                    audio_offset,
                    duration
                ))
                .arg("-map")
                .arg("[v]")
                .arg("-map")
                .arg("[a]")
                .arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg("veryfast")
                .arg("-crf")
                .arg("20")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("192k");
        }
    }

    command
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    command
}

#[allow(clippy::too_many_arguments)]
fn merge_true_quality_dash_range_parts(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    video_path: &Path,
    audio_path: &Path,
    final_path: &Path,
    download_dir: &Path,
    video_part: &DashRangePart,
    audio_part: &DashRangePart,
    clip_range: ClipRange,
    expected_height: Option<u32>,
    expected_duration: f64,
    started_at_ms: u128,
) -> Result<(), String> {
    let mut errors = Vec::new();
    let video_offset = (clip_range.start - video_part.media_start).max(0.0);
    let audio_offset = (clip_range.start - audio_part.media_start).max(0.0);

    for encode_mode in [
        ClipEncodeMode::Copy,
        ClipEncodeMode::Nvenc,
        ClipEncodeMode::X264,
    ] {
        emit_simple_progress(
            app,
            Some(86.0),
            format!(
                "True Quality 2K/4K klip local olarak işleniyor: {}",
                encode_mode.label()
            ),
        );

        let command = build_ffmpeg_merge_dash_range_clip_command(
            tools,
            video_path,
            audio_path,
            final_path,
            video_offset,
            audio_offset,
            expected_duration,
            encode_mode,
        );

        match run_ffmpeg_clip_process_with_progress_range(
            app,
            command,
            download_dir,
            final_path,
            started_at_ms,
            expected_duration,
            encode_mode,
            86.0,
            99.0,
        ) {
            Ok(()) => {
                match validate_clip_output(tools, final_path, expected_height, expected_duration) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        let _ = fs::remove_file(final_path);
                        errors.push(format!(
                            "[dash-range merge / {}] Doğrulama başarısız:\n{}",
                            encode_mode.label(),
                            sanitize_report_text(&err)
                        ));
                    }
                }
            }
            Err(err) => {
                if err == PAUSED_SIGNAL || err == CANCELLED_SIGNAL {
                    return Err(err);
                }

                let _ = fs::remove_file(final_path);
                errors.push(format!(
                    "[dash-range merge / {}] ffmpeg başarısız:\n{}",
                    encode_mode.label(),
                    sanitize_report_text(&err)
                ));
            }
        }
    }

    Err(format!(
        "DASH byte-range parçalarından doğrulanmış 2K/4K klip üretilemedi.\n\n{}",
        errors.join("\n\n---\n\n")
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_true_quality_remote_split_attempt(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    temp_dir: &Path,
    video_url: &str,
    audio_url: &str,
    final_path: &Path,
    padded_range: ClipRange,
    clip_range: ClipRange,
    expected_height: Option<u32>,
    expected_duration: f64,
    started_at_ms: u128,
    attempt_label: &str,
) -> Result<(), String> {
    let attempt_stamp = unique_stamp();
    let temp_video = temp_dir.join(format!("true-quality-remote-video-{}.mkv", attempt_stamp));
    let temp_audio = temp_dir.join(format!("true-quality-remote-audio-{}.mka", attempt_stamp));
    let trim_range = offset_clip_range(clip_range, padded_range);

    let result = (|| {
        emit_simple_progress(
            app,
            Some(8.0),
            format!(
                "{}: 2K/4K video stream uzak kesimle indiriliyor...",
                attempt_label
            ),
        );

        download_true_quality_remote_part(
            app,
            tools,
            video_url,
            &temp_video,
            temp_dir,
            padded_range,
            false,
            started_at_ms,
            8.0,
            47.0,
        )?;

        emit_simple_progress(
            app,
            Some(48.0),
            format!("{}: ses stream uzak kesimle indiriliyor...", attempt_label),
        );

        download_true_quality_remote_part(
            app,
            tools,
            audio_url,
            &temp_audio,
            temp_dir,
            padded_range,
            true,
            started_at_ms,
            48.0,
            78.0,
        )?;

        emit_simple_progress(
            app,
            Some(82.0),
            format!(
                "{}: uzak kesim parçaları local olarak birleştiriliyor...",
                attempt_label
            ),
        );

        merge_true_quality_clip_parts(
            app,
            tools,
            &temp_video,
            &temp_audio,
            final_path,
            download_dir,
            trim_range,
            expected_height,
            expected_duration,
            started_at_ms,
        )
    })();

    let _ = fs::remove_file(&temp_video);
    let _ = fs::remove_file(&temp_audio);

    result
}

#[allow(clippy::too_many_arguments)]
fn run_true_quality_remote_split_download(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    temp_dir: &Path,
    video_url: &str,
    audio_url: &str,
    output_base: &str,
    quality: &str,
    padded_range: ClipRange,
    clip_range: ClipRange,
    expected_height: Option<u32>,
    expected_duration: f64,
    started_at_ms: u128,
    attempt_label: &str,
) -> Result<DownloadResult, String> {
    let temp_output_base = format!("mediadrop-temp-true-quality-remote-{}", unique_stamp());
    let temp_final_path = unique_output_path(download_dir, &temp_output_base, "mp4");

    match run_true_quality_remote_split_attempt(
        app,
        tools,
        download_dir,
        temp_dir,
        video_url,
        audio_url,
        &temp_final_path,
        padded_range,
        clip_range,
        expected_height,
        expected_duration,
        started_at_ms,
        attempt_label,
    ) {
        Ok(()) => {
            emit_simple_progress(app, Some(100.0), "True Quality 2K/4K klip tamamlandı.");

            let final_path =
                finalize_pretty_output_file(&temp_final_path, download_dir, output_base);
            let file_size = file_size(&final_path).unwrap_or(0);
            let file_path = final_path.to_string_lossy().to_string();

            Ok(DownloadResult {
                message: format!(
                    "Klip {} True Quality uzak split moduyla indirildi. Dosya: {}",
                    filename_quality_label("video", quality),
                    file_path
                ),
                file_path,
                output_dir: download_dir.to_string_lossy().to_string(),
                mode: format!(
                    "Klip modu / True Quality remote split fallback ({})",
                    attempt_label
                ),
                file_size,
            })
        }
        Err(err) => {
            let _ = fs::remove_file(&temp_final_path);
            Err(err)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_true_quality_clip_pipeline(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    clean_url: &str,
    selected_format_id: &str,
    quality: &str,
    title_hint: Option<&str>,
    clip_range: ClipRange,
    started_at_ms: u128,
) -> Result<DownloadResult, String> {
    let Some(expected_height) = quality_height_limit(quality) else {
        return Err("Yüksek kalite klip için kalite yüksekliği okunamadı.".to_string());
    };

    if expected_height <= 1080 {
        return Err("Yüksek kalite klip motoru yalnızca 2K/4K seçimlerinde çalışır.".to_string());
    }

    let attempts = make_true_quality_clip_attempts(selected_format_id, quality);
    let temp_dir = mediadrop_config_dir()?.join("clip-true-quality-temp");

    fs::create_dir_all(&temp_dir)
        .map_err(|err| format!("Yüksek kalite geçici klip klasörü oluşturulamadı: {}", err))?;

    let padded_range = hls_padded_clip_range(clip_range);
    let expected_duration = (clip_range.end - clip_range.start).max(1.0);
    let title = title_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| ytdlp_print_title(tools, clean_url));

    let output_base = pretty_output_base("video", Some(&title), quality, Some(clip_range));

    let mut errors = Vec::new();

    emit_simple_progress(
        app,
        Some(0.0),
        format!(
            "{} klip için DASH byte-range motoru hazırlanıyor...",
            filename_quality_label("video", quality)
        ),
    );

    for attempt in attempts {
        let attempt_stamp = unique_stamp();
        let temp_video = temp_dir.join(format!("true-quality-range-video-{}.mp4", attempt_stamp));
        let temp_audio = temp_dir.join(format!("true-quality-range-audio-{}.m4a", attempt_stamp));
        let temp_final_path = unique_output_path(
            download_dir,
            &format!("mediadrop-temp-true-quality-{}", attempt_stamp),
            "mp4",
        );

        emit_simple_progress(
            app,
            Some(2.0),
            format!("{}: 2K/4K stream URL'leri çözülüyor...", attempt.label),
        );

        let video_urls = match ytdlp_stream_urls(
            &tools.yt_dlp,
            clean_url,
            &attempt.video_selector,
            attempt.extractor_args,
            attempt.force_ipv4,
            attempt.force_ipv6,
            YTDLP_STREAM_RESOLVE_TIMEOUT,
        ) {
            Ok(urls) => urls,
            Err(err) => {
                errors.push(format!(
                    "[{}] 2K/4K video URL çözülemedi:\n{}",
                    attempt.label,
                    sanitize_report_text(&err)
                ));
                continue;
            }
        };

        let audio_urls = match ytdlp_stream_urls(
            &tools.yt_dlp,
            clean_url,
            &attempt.audio_selector,
            attempt.extractor_args,
            attempt.force_ipv4,
            attempt.force_ipv6,
            YTDLP_STREAM_RESOLVE_TIMEOUT,
        ) {
            Ok(urls) => urls,
            Err(err) => {
                errors.push(format!(
                    "[{}] Ses URL çözülemedi:\n{}",
                    attempt.label,
                    sanitize_report_text(&err)
                ));
                continue;
            }
        };

        let video_url = video_urls.first().cloned().unwrap_or_default();
        let audio_url = audio_urls.first().cloned().unwrap_or_default();

        if video_url.is_empty() || audio_url.is_empty() {
            errors.push(format!(
                "[{}] Video/ses stream URL listesi boş geldi.",
                attempt.label
            ));
            continue;
        }

        emit_simple_progress(
            app,
            Some(8.0),
            format!(
                "{}: sidx indeksinden 2K/4K byte aralıkları hesaplanıyor...",
                attempt.label
            ),
        );

        let video_part = match download_true_quality_dash_range_part(
            app,
            &video_url,
            &temp_video,
            padded_range,
            false,
            8.0,
            55.0,
            started_at_ms,
        ) {
            Ok(part) => part,
            Err(err) => {
                let _ = fs::remove_file(&temp_video);
                let _ = fs::remove_file(&temp_audio);
                errors.push(format!(
                    "[{}] 2K/4K video byte-range parçası indirilemedi:\n{}",
                    attempt.label,
                    sanitize_report_text(&err)
                ));

                match run_true_quality_remote_split_download(
                    app,
                    tools,
                    download_dir,
                    &temp_dir,
                    &video_url,
                    &audio_url,
                    &output_base,
                    quality,
                    padded_range,
                    clip_range,
                    Some(expected_height),
                    expected_duration,
                    started_at_ms,
                    &attempt.label,
                ) {
                    Ok(result) => return Ok(result),
                    Err(remote_err) => {
                        if remote_err == PAUSED_SIGNAL || remote_err == CANCELLED_SIGNAL {
                            return Err(remote_err);
                        }

                        errors.push(format!(
                            "[{}] Uzak split fallback başarısız:\n{}",
                            attempt.label,
                            sanitize_report_text(&remote_err)
                        ));
                        continue;
                    }
                }
            }
        };

        emit_simple_progress(
            app,
            Some(56.0),
            format!("{}: ses byte aralıkları indiriliyor...", attempt.label),
        );

        let audio_part = match download_true_quality_dash_range_part(
            app,
            &audio_url,
            &temp_audio,
            padded_range,
            true,
            56.0,
            78.0,
            started_at_ms,
        ) {
            Ok(part) => part,
            Err(err) => {
                let _ = fs::remove_file(&temp_video);
                let _ = fs::remove_file(&temp_audio);
                errors.push(format!(
                    "[{}] Ses byte-range parçası indirilemedi:\n{}",
                    attempt.label,
                    sanitize_report_text(&err)
                ));

                match run_true_quality_remote_split_download(
                    app,
                    tools,
                    download_dir,
                    &temp_dir,
                    &video_url,
                    &audio_url,
                    &output_base,
                    quality,
                    padded_range,
                    clip_range,
                    Some(expected_height),
                    expected_duration,
                    started_at_ms,
                    &attempt.label,
                ) {
                    Ok(result) => return Ok(result),
                    Err(remote_err) => {
                        if remote_err == PAUSED_SIGNAL || remote_err == CANCELLED_SIGNAL {
                            return Err(remote_err);
                        }

                        errors.push(format!(
                            "[{}] Uzak split fallback başarısız:\n{}",
                            attempt.label,
                            sanitize_report_text(&remote_err)
                        ));
                        continue;
                    }
                }
            }
        };

        emit_simple_progress(
            app,
            Some(82.0),
            format!(
                "{}: 2K/4K byte-range parçaları local olarak birleştiriliyor...",
                attempt.label
            ),
        );

        let merge_result = merge_true_quality_dash_range_parts(
            app,
            tools,
            &temp_video,
            &temp_audio,
            &temp_final_path,
            download_dir,
            &video_part,
            &audio_part,
            clip_range,
            Some(expected_height),
            expected_duration,
            started_at_ms,
        );

        let _ = fs::remove_file(&temp_video);
        let _ = fs::remove_file(&temp_audio);

        match merge_result {
            Ok(()) => {
                emit_simple_progress(app, Some(100.0), "True Quality 2K/4K klip tamamlandı.");

                let final_path =
                    finalize_pretty_output_file(&temp_final_path, download_dir, &output_base);
                let file_size = file_size(&final_path).unwrap_or(0);
                let file_path = final_path.to_string_lossy().to_string();

                return Ok(DownloadResult {
                    message: format!(
                        "Klip {} True Quality DASH byte-range moduyla indirildi. Dosya: {}",
                        filename_quality_label("video", quality),
                        file_path
                    ),
                    file_path,
                    output_dir: download_dir.to_string_lossy().to_string(),
                    mode: format!(
                        "Klip modu / True Quality DASH byte-range ({}, video {:.2} MB, ses {:.2} MB)",
                        attempt.label,
                        video_part.bytes_written as f64 / 1024.0 / 1024.0,
                        audio_part.bytes_written as f64 / 1024.0 / 1024.0
                    ),
                    file_size,
                });
            }
            Err(err) => {
                if err == PAUSED_SIGNAL || err == CANCELLED_SIGNAL {
                    let _ = fs::remove_file(&temp_final_path);
                    return Err(err);
                }

                let _ = fs::remove_file(&temp_final_path);
                errors.push(format!(
                    "[{}] DASH byte-range parçaları birleştirilemedi:\n{}",
                    attempt.label,
                    sanitize_report_text(&err)
                ));

                match run_true_quality_remote_split_download(
                    app,
                    tools,
                    download_dir,
                    &temp_dir,
                    &video_url,
                    &audio_url,
                    &output_base,
                    quality,
                    padded_range,
                    clip_range,
                    Some(expected_height),
                    expected_duration,
                    started_at_ms,
                    &attempt.label,
                ) {
                    Ok(result) => return Ok(result),
                    Err(remote_err) => {
                        if remote_err == PAUSED_SIGNAL || remote_err == CANCELLED_SIGNAL {
                            return Err(remote_err);
                        }

                        errors.push(format!(
                            "[{}] Uzak split fallback başarısız:\n{}",
                            attempt.label,
                            sanitize_report_text(&remote_err)
                        ));
                    }
                }
            }
        }
    }

    Err(format!(
        "True Quality DASH byte-range motoru doğrulanmış 2K/4K çıktı üretemedi. Seçilen kalite: {}.\n\n{}",
        quality,
        errors.join("\n\n---\n\n")
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_ytdlp_clip_fallback(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    output_template: &str,
    final_output_base: &str,
    clean_kind: &str,
    clean_url: &str,
    _clean_format_id: &str,
    quality: &str,
    _fast_mode: bool,
    clip_range: ClipRange,
    started_at_ms: u128,
) -> Result<DownloadResult, String> {
    if clean_kind == "audio" {
        return Err("Klip modu ses-only formatlarda desteklenmiyor.".to_string());
    }

    let attempts = make_youtube_hls_clip_attempts(quality);
    let expected_height = youtube_hls_clip_height_limit(quality);
    let expected_duration = clip_range.end - clip_range.start;
    let padded_range = hls_padded_clip_range(clip_range);
    let trim_range = offset_clip_range(clip_range, padded_range);
    let temp_dir = mediadrop_config_dir()?.join("clip-hls-temp");

    fs::create_dir_all(&temp_dir)
        .map_err(|err| format!("HLS geçici klip klasörü oluşturulamadı: {}", err))?;

    let mut errors: Vec<String> = Vec::new();

    emit_simple_progress(app, Some(0.0), "Klip için HLS segment akışı aranıyor...");

    for attempt in attempts {
        let temp_prefix = format!("md-hls-{}", unique_stamp());
        let temp_output_template = format!("{}-{}", temp_prefix, output_template);
        let temp_started_at_ms = now_ms();

        emit_simple_progress(
            app,
            Some(0.0),
            format!(
                "{} deneniyor... Başlangıç tamponu: {:.0} sn",
                attempt.label,
                (clip_range.start - padded_range.start).max(0.0)
            ),
        );

        match run_download_attempt(
            app,
            tools,
            &temp_dir,
            &temp_output_template,
            clean_kind,
            clean_url,
            &attempt,
            temp_started_at_ms,
            Some(padded_range),
        ) {
            Ok(()) => {
                let temp_file = match take_ytdlp_final_output(&temp_dir) {
                    Some(path) => path,
                    None => {
                        errors.push(format!(
                            "[{}] HLS klip tamamlandı gibi göründü ama geçici çıktı bulunamadı.",
                            attempt.label
                        ));
                        continue;
                    }
                };

                let final_path = unique_pretty_output_path(download_dir, final_output_base, "mp4");

                match trim_hls_downloaded_clip(
                    app,
                    tools,
                    &temp_file,
                    &final_path,
                    download_dir,
                    trim_range,
                    expected_height,
                    expected_duration,
                    started_at_ms,
                ) {
                    Ok(()) => {
                        let _ = fs::remove_file(&temp_file);
                        emit_simple_progress(app, Some(100.0), "Klip tamamlandı.");

                        let file_size = file_size(&final_path).unwrap_or(0);
                        let file_path = final_path.to_string_lossy().to_string();
                        let mut message = format!("Klip indirildi. Dosya: {}", file_path);

                        if let (Some(selected_height), Some(hls_height)) = (
                            quality_height_limit(quality),
                            youtube_hls_clip_height_limit(quality),
                        ) {
                            if selected_height > hls_height {
                                message = format!(
                                    "Klip hızlı HLS moduyla {} olarak indirildi. Dosya: {}",
                                    youtube_hls_clip_quality_label(quality),
                                    file_path
                                );
                            }
                        }

                        return Ok(DownloadResult {
                            message,
                            file_path,
                            output_dir: download_dir.to_string_lossy().to_string(),
                            mode: format!(
                                "Klip modu / HLS segment + temiz kesim ({})",
                                attempt.label
                            ),
                            file_size,
                        });
                    }
                    Err(err) => {
                        let _ = fs::remove_file(&final_path);
                        let _ = fs::remove_file(&temp_file);
                        errors.push(format!(
                            "[{}] HLS geçici klip indirildi ama temiz kesim başarısız:\n{}",
                            attempt.label,
                            sanitize_report_text(&err)
                        ));
                    }
                }
            }
            Err(error) => {
                if error == PAUSED_SIGNAL || error == CANCELLED_SIGNAL {
                    return Err(error);
                }

                errors.push(format!(
                    "[{}] HLS klip başarısız:\n{}",
                    attempt.label,
                    sanitize_report_text(&error)
                ));
            }
        }

        emit_simple_progress(
            app,
            Some(0.0),
            "Bu HLS modu başarısız oldu. Sonraki HLS modu deneniyor...",
        );
    }

    Err(format!(
        "Hızlı HLS klip yöntemi doğrulanmış çıktı üretemedi. Seçilen kalite: {}.\n\n{}",
        quality,
        errors.join("\n\n---\n\n")
    ))
}

fn build_ffmpeg_local_clip_command(
    tools: &RuntimeTools,
    input_path: &Path,
    output_path: &Path,
    clip_range: ClipRange,
    encode_mode: ClipEncodeMode,
) -> Command {
    let ffmpeg = tools.ffmpeg_dir.join("ffmpeg.exe");
    let duration = (clip_range.end - clip_range.start).max(1.0);
    let seek_margin = if encode_mode == ClipEncodeMode::Copy {
        0.0
    } else {
        3.0
    };
    let preseek = (clip_range.start - seek_margin).max(0.0);
    let fine_seek = (clip_range.start - preseek).max(0.0);

    let mut command = hidden_command(ffmpeg);
    prepend_path(&mut command, &tools.ffmpeg_dir);

    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-fflags")
        .arg("+genpts")
        .arg("-ss")
        .arg(format!("{:.3}", preseek))
        .arg("-i")
        .arg(input_path)
        .arg("-ss")
        .arg(format!("{:.3}", fine_seek))
        .arg("-t")
        .arg(format!("{:.3}", duration))
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("0:a:0?");

    match encode_mode {
        ClipEncodeMode::Copy | ClipEncodeMode::RemoteCopy => {
            command
                .arg("-c")
                .arg("copy")
                .arg("-avoid_negative_ts")
                .arg("make_zero");
        }
        ClipEncodeMode::Nvenc => {
            command
                .arg("-c:v")
                .arg("h264_nvenc")
                .arg("-preset")
                .arg("p4")
                .arg("-cq")
                .arg("21")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("160k");
        }
        ClipEncodeMode::X264 => {
            command
                .arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg("veryfast")
                .arg("-crf")
                .arg("20")
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("160k");
        }
    }

    command
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    command
}

#[allow(clippy::too_many_arguments)]
fn trim_hls_downloaded_clip(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    source_path: &Path,
    output_path: &Path,
    download_dir: &Path,
    trim_range: ClipRange,
    expected_height: Option<u32>,
    expected_duration: f64,
    started_at_ms: u128,
) -> Result<(), String> {
    let mut errors = Vec::new();

    for encode_mode in hls_clean_clip_encode_modes() {
        emit_simple_progress(
            app,
            Some(88.0),
            format!("HLS klip temizleniyor: {}", encode_mode.label()),
        );

        let command = build_ffmpeg_local_clip_command(
            tools,
            source_path,
            output_path,
            trim_range,
            encode_mode,
        );

        match run_ffmpeg_clip_process_with_progress_range(
            app,
            command,
            download_dir,
            output_path,
            started_at_ms,
            expected_duration,
            encode_mode,
            88.0,
            99.0,
        ) {
            Ok(()) => {
                match validate_clip_output(tools, output_path, expected_height, expected_duration) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        let _ = fs::remove_file(output_path);
                        errors.push(format!(
                            "[HLS clean / {}] Doğrulama başarısız:
{}",
                            encode_mode.label(),
                            sanitize_report_text(&err)
                        ));
                    }
                }
            }
            Err(err) => {
                if err == PAUSED_SIGNAL || err == CANCELLED_SIGNAL {
                    return Err(err);
                }

                let _ = fs::remove_file(output_path);
                errors.push(format!(
                    "[HLS clean / {}] ffmpeg başarısız:
{}",
                    encode_mode.label(),
                    sanitize_report_text(&err)
                ));
            }
        }
    }

    Err(format!(
        "HLS segmentinden temiz klip üretilemedi.

{}",
        errors.join(
            "

---

"
        )
    ))
}

fn build_download_command(
    tools: &RuntimeTools,
    download_dir: &Path,
    output_template: &str,
    clean_kind: &str,
    attempt: &DownloadAttempt,
    clip_range: Option<ClipRange>,
) -> Command {
    let mut command = ytdlp_command(&tools.yt_dlp);

    // ffmpeg, ffprobe ve deno aynı runtime bin klasöründe tutuluyor.
    // Bu yüzden bu klasörü PATH başına koymak yt-dlp'nin Deno'yu görmesi için de yeterli.
    prepend_path(&mut command, &tools.ffmpeg_dir);

    command
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--no-overwrites")
        .arg("--windows-filenames")
        .arg("--restrict-filenames")
        .arg("--continue")
        .arg("--newline")
        .arg("--progress")
        .arg("--progress-template")
        .arg(YTDLP_PROGRESS_TEMPLATE)
        .arg("--concurrent-fragments")
        .arg("4")
        .arg("--print")
        .arg(format!(
            "after_move:{}%(filepath)s",
            YTDLP_FINAL_PATH_MARKER
        ))
        .arg("--socket-timeout")
        .arg("30")
        .arg("--retries")
        .arg("10")
        .arg("--fragment-retries")
        .arg("10")
        .arg("--ffmpeg-location")
        .arg(&tools.ffmpeg_dir)
        .arg("-P")
        .arg(download_dir)
        .arg("-o")
        .arg(output_template);

    if attempt.force_ipv4 {
        command.arg("--force-ipv4");
    }

    if attempt.force_ipv6 {
        command.arg("--force-ipv6");
    }

    if attempt.impersonate_chrome {
        command.arg("--impersonate").arg("chrome");
    }

    if let Some(size) = attempt.http_chunk_size.as_deref() {
        command.arg("--http-chunk-size").arg(size);
    }

    if let Some(args) = attempt.extractor_args.as_deref() {
        command.arg("--extractor-args").arg(args);
    }

    if let Some(range) = clip_range {
        command
            .arg("--download-sections")
            .arg(clip_download_sections_arg(range));

        if attempt.force_keyframes_at_cuts {
            command.arg("--force-keyframes-at-cuts");
        }
    }

    match attempt.external_downloader {
        ExternalDownloader::Native => {}
        ExternalDownloader::Aria2c => {
            command
                .arg("--downloader")
                .arg(&tools.aria2c)
                .arg("--downloader")
                .arg("dash,m3u8:native")
                .arg("--downloader-args")
                .arg("aria2c:-x 8 -s 8 -k 1M --file-allocation=none --summary-interval=1 --console-log-level=warn");
        }
        ExternalDownloader::Curl => {
            command
                .arg("--downloader")
                .arg("curl")
                .arg("--downloader-args")
                .arg("curl:-L --retry 5 --retry-delay 1 --connect-timeout 30");
        }
    }

    if clean_kind == "audio" {
        command
            .arg("-f")
            .arg(&attempt.format_selector)
            .arg("-x")
            .arg("--audio-format")
            .arg("mp3")
            .arg("--audio-quality")
            .arg("0");
    } else {
        command
            .arg("-f")
            .arg(&attempt.format_selector)
            .arg("--merge-output-format")
            .arg("mp4");

        if clip_range.is_some() && attempt.recode_video {
            command.arg("--recode-video").arg("mp4");
        }
    }

    command
}

#[allow(clippy::too_many_arguments)]
fn run_download_attempt_with_progress_mode(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    output_template: &str,
    clean_kind: &str,
    clean_url: &str,
    attempt: &DownloadAttempt,
    started_at_ms: u128,
    clip_range: Option<ClipRange>,
    progress_mode: DownloadProgressMode,
    stop_generation: Option<u64>,
) -> Result<(), String> {
    match progress_mode {
        DownloadProgressMode::Default => {
            emit_simple_progress(app, Some(0.0), format!("{} deneniyor...", attempt.label));
        }
        DownloadProgressMode::TwitterPostMp4Download => {
            emit_simple_progress(app, None, "Gönderi videosu indiriliyor...");
        }
    }

    let mut command = build_download_command(
        tools,
        download_dir,
        output_template,
        clean_kind,
        attempt,
        clip_range,
    );
    let _cookie_file = add_registered_ytdlp_cookies(&mut command, clean_url)?;
    let youtube_info_file = add_ytdlp_media_source(&mut command, clean_url)?;
    let used_cached_analysis = youtube_info_file.is_some();

    let result = if clip_range.is_some() {
        run_ytdlp_process_with_watchdog(
            app,
            command,
            download_dir,
            started_at_ms,
            Some(YtdlpWatchdogConfig::clip_download()),
            progress_mode,
            stop_generation,
        )
    } else {
        run_ytdlp_process_with_watchdog(
            app,
            command,
            download_dir,
            started_at_ms,
            Some(YtdlpWatchdogConfig::full_download()),
            progress_mode,
            stop_generation,
        )
    };

    if used_cached_analysis {
        if let Err(error) = &result {
            if error != PAUSED_SIGNAL && error != CANCELLED_SIGNAL {
                invalidate_youtube_analysis(clean_url);
            }
        }
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn run_download_attempt(
    app: &tauri::AppHandle,
    tools: &RuntimeTools,
    download_dir: &Path,
    output_template: &str,
    clean_kind: &str,
    clean_url: &str,
    attempt: &DownloadAttempt,
    started_at_ms: u128,
    clip_range: Option<ClipRange>,
) -> Result<(), String> {
    run_download_attempt_with_progress_mode(
        app,
        tools,
        download_dir,
        output_template,
        clean_kind,
        clean_url,
        attempt,
        started_at_ms,
        clip_range,
        DownloadProgressMode::Default,
        None,
    )
}

fn make_youtube_attempts(
    clean_kind: &str,
    selected_format_id: &str,
    quality: &str,
    fast_mode: bool,
    clip_active: bool,
) -> Vec<DownloadAttempt> {
    let mut attempts = Vec::new();

    let selected_selector = if clean_kind == "audio" {
        selected_format_id.to_string()
    } else {
        youtube_selected_selector(selected_format_id, quality)
    };

    if fast_mode && !clip_active {
        let mut attempt = download_attempt("Hızlı mod / aria2c", &selected_selector);
        attempt.external_downloader = ExternalDownloader::Aria2c;
        attempt.force_ipv4 = true;
        attempts.push(attempt);
    }

    let mut stable = download_attempt(
        if clip_active {
            "Klip modu / hızlı section cut"
        } else {
            "Stabil mod"
        },
        &selected_selector,
    );
    stable.force_ipv4 = true;
    // Klipte varsayılan olarak force-keyframes kullanmıyoruz; bu seçenek ffmpeg re-encode'u
    // tetikleyip küçük kliplerde bile dakikalarca bekletebiliyor. Hassas kesim ayrı fallback.
    attempts.push(stable);

    if clip_active {
        let mut compat = download_attempt("Klip modu / uyumluluk kesimi", &selected_selector);
        compat.force_ipv4 = true;
        compat.force_keyframes_at_cuts = true;
        compat.recode_video = true;
        attempts.push(compat);
    }

    let mut chrome = download_attempt("Chrome uyumluluk modu", &selected_selector);
    chrome.force_ipv4 = true;
    chrome.impersonate_chrome = true;
    attempts.push(chrome);

    let mut web = download_attempt("Web client modu", &selected_selector);
    web.force_ipv4 = true;
    web.extractor_args = Some("youtube:player_client=web".to_string());
    attempts.push(web);

    // Aşağıdaki denemeler yalnızca önceki denemelerde SSL/ağ hatası tespit edilirse çalışır.
    // Kullanıcının seçtiği kalite korunur; 720p/tek dosya gibi kalite düşüren fallback yapılmaz.
    let mut chunk = download_attempt(
        "YouTube ağ kurtarma / küçük HTTP parçaları",
        &selected_selector,
    );
    chunk.force_ipv4 = true;
    chunk.http_chunk_size = Some("1M".to_string());
    chunk.only_when_ssl_error = true;
    attempts.push(chunk);

    if !clip_active {
        let mut aria_rescue = download_attempt("YouTube ağ kurtarma / aria2c", &selected_selector);
        aria_rescue.external_downloader = ExternalDownloader::Aria2c;
        aria_rescue.force_ipv4 = true;
        aria_rescue.only_when_ssl_error = true;
        attempts.push(aria_rescue);

        if find_in_path("curl").is_some() {
            let mut curl_rescue =
                download_attempt("YouTube ağ kurtarma / curl", &selected_selector);
            curl_rescue.external_downloader = ExternalDownloader::Curl;
            curl_rescue.force_ipv4 = true;
            curl_rescue.only_when_ssl_error = true;
            attempts.push(curl_rescue);
        }
    }

    let mut android = download_attempt("YouTube ağ kurtarma / Android client", &selected_selector);
    android.force_ipv4 = true;
    android.extractor_args = Some("youtube:player_client=android".to_string());
    android.only_when_ssl_error = true;
    attempts.push(android);

    let mut mweb = download_attempt("YouTube ağ kurtarma / mweb client", &selected_selector);
    mweb.force_ipv4 = true;
    mweb.extractor_args = Some("youtube:player_client=mweb".to_string());
    mweb.only_when_ssl_error = true;
    attempts.push(mweb);

    let mut ipv6 = download_attempt("YouTube ağ kurtarma / IPv6", &selected_selector);
    ipv6.force_ipv6 = true;
    ipv6.only_when_ssl_error = true;
    attempts.push(ipv6);

    attempts
}

fn make_generic_attempts(
    clean_kind: &str,
    clean_format_id: &str,
    is_twitter: bool,
    is_instagram: bool,
    is_tiktok: bool,
    fast_mode: bool,
) -> Vec<DownloadAttempt> {
    let format_selector = if is_twitter || is_instagram || is_tiktok {
        social_format_selector(clean_format_id)
    } else if clean_kind == "audio" {
        clean_format_id.to_string()
    } else {
        format!(
            "{}+bestaudio[ext=m4a]/{}+bestaudio/best",
            clean_format_id, clean_format_id
        )
    };

    let mut attempts = Vec::new();

    if fast_mode {
        let mut attempt = download_attempt("Hızlı mod / aria2c", &format_selector);
        attempt.external_downloader = ExternalDownloader::Aria2c;
        attempts.push(attempt);
    }

    attempts.push(download_attempt("Stabil mod", &format_selector));

    let mut rescue = download_attempt("Ağ kurtarma modu", &format_selector);
    rescue.force_ipv4 = true;
    attempts.push(rescue);

    attempts
}

fn user_friendly_download_error(is_youtube: bool, errors: &[(String, String)]) -> String {
    let last_error = errors
        .last()
        .map(|(_, error)| error.as_str())
        .unwrap_or("Bilinmeyen hata.");

    let combined = errors
        .iter()
        .map(|(label, error)| format!("--- {} ---\n{}", label, error.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");

    if is_youtube && is_ssl_or_network_error(&combined) {
        return format!(
            "YouTube indirme başarısız oldu. Bu PC/ağ YouTube googlevideo CDN bağlantısını kesiyor olabilir.\n\n\
            Denenecekler: telefon hotspot, VPN kapatma/açma, antivirüs Web Shield/HTTPS scanning kapatma, IPv6 kapatma veya farklı DNS.\n\n\
            Son hata:\n{}",
            last_error.trim()
        );
    }

    format!(
        "İndirme başarısız oldu. Denenen modlar sonuç vermedi.\n\nSon hata:\n{}",
        last_error.trim()
    )
}

fn find_first_thumbnail_file(dir: &Path) -> Option<PathBuf> {
    let allowed = ["jpg", "jpeg", "png", "webp"];

    let mut files = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| allowed.contains(&ext.to_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    files.sort_by(|a, b| {
        let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
        let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    files.into_iter().next()
}

fn image_mime_from_path(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "image/jpeg",
    }
}

fn file_to_data_url(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("Thumbnail dosyası okunamadı: {}", err))?;

    if bytes.is_empty() {
        return Err("Thumbnail dosyası boş.".to_string());
    }

    let mime = image_mime_from_path(path);
    let encoded = general_purpose::STANDARD.encode(bytes);

    Ok(format!("data:{};base64,{}", mime, encoded))
}

fn twitter_avatar_host_allowed(host: &str) -> bool {
    let clean = host
        .trim()
        .trim_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();

    clean == "pbs.twimg.com" || clean == "abs.twimg.com" || clean.ends_with(".twimg.com")
}

fn twitter_avatar_url_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .map(twitter_avatar_host_allowed)
            .unwrap_or(false)
}

fn twitter_avatar_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_TWITTER_AVATAR_REDIRECTS {
            return attempt.error("Avatar redirect limiti aşıldı.");
        }

        if !twitter_avatar_url_allowed(attempt.url()) {
            return attempt.error("Avatar redirect hedefi güvenli değil.");
        }

        attempt.follow()
    })
}

fn twitter_profile_host_allowed(host: &str) -> bool {
    let clean = host.trim().trim_matches('.').to_lowercase();

    clean == "x.com"
        || clean == "twitter.com"
        || clean.ends_with(".x.com")
        || clean.ends_with(".twitter.com")
}

fn twitter_profile_url_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .map(twitter_profile_host_allowed)
            .unwrap_or(false)
}

fn twitter_profile_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_TWITTER_AVATAR_REDIRECTS {
            return attempt.error("Profil redirect limiti aşıldı.");
        }

        if !twitter_profile_url_allowed(attempt.url()) {
            return attempt.error("Profil redirect hedefi güvenli değil.");
        }

        attempt.follow()
    })
}

fn clean_twitter_handle(value: &str) -> Result<String, String> {
    let clean = value
        .trim()
        .trim_start_matches('@')
        .split(['/', '?', '#', ' '])
        .next()
        .unwrap_or("")
        .trim();

    if clean.is_empty()
        || clean.len() > 15
        || !clean
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err("Twitter kullanıcı adı geçersiz.".to_string());
    }

    Ok(clean.to_string())
}

fn avatar_mime_from_text(value: &str) -> Option<&'static str> {
    let clean = value.split(';').next().unwrap_or("").trim().to_lowercase();

    match clean.as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

fn avatar_mime_from_url_path(path: &str) -> Option<&'static str> {
    let clean = path.to_lowercase();

    if clean.ends_with(".jpg") || clean.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if clean.ends_with(".png") {
        Some("image/png")
    } else if clean.ends_with(".webp") {
        Some("image/webp")
    } else {
        None
    }
}

fn avatar_mime_from_format_value(value: &str) -> Option<&'static str> {
    match value.trim().trim_start_matches('.').to_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn avatar_mime_from_url(url: &reqwest::Url) -> Option<&'static str> {
    avatar_mime_from_url_path(url.path()).or_else(|| {
        url.query_pairs().find_map(|(key, value)| {
            if key.eq_ignore_ascii_case("format") {
                avatar_mime_from_format_value(&value)
            } else {
                None
            }
        })
    })
}

fn avatar_bytes_match_mime(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn image_bytes_to_data_url(bytes: &[u8], mime: &str) -> String {
    let encoded = general_purpose::STANDARD.encode(bytes);
    format!("data:{};base64,{}", mime, encoded)
}

fn read_limited_body<R: Read>(mut reader: R, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16 * 1024];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("Avatar verisi okunamadı: {}", err))?;

        if read == 0 {
            break;
        }

        if bytes.len().saturating_add(read) > limit {
            return Err("Avatar dosyası çok büyük.".to_string());
        }

        bytes.extend_from_slice(&buffer[..read]);
    }

    Ok(bytes)
}

fn read_response_body_with_limit(
    response: reqwest::blocking::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    read_limited_body(response, limit)
}

#[derive(Clone)]
enum GalleryAuthAttempt {
    None,
    RegisteredTwitterCookies,
    BrowserCookies {
        browser_id: &'static str,
        browser_label: &'static str,
        label: &'static str,
        save: bool,
    },
    PreparedInstagramCookies {
        token: String,
    },
    SavedInstagramCookies,
}

#[derive(Clone, Copy)]
struct BrowserAuthDefinition {
    browser_id: &'static str,
    browser_label: &'static str,
    label: &'static str,
}

fn browser_auth_definitions(browser_id: &str) -> Vec<BrowserAuthDefinition> {
    match browser_id {
        "opera_gx" => vec![BrowserAuthDefinition {
            browser_id: "opera_gx",
            browser_label: "Opera GX",
            label: "opera gx instagram cookies",
        }],
        "opera" => vec![BrowserAuthDefinition {
            browser_id: "opera",
            browser_label: "Opera",
            label: "opera instagram cookies",
        }],
        "chrome" => vec![BrowserAuthDefinition {
            browser_id: "chrome",
            browser_label: "Google Chrome",
            label: "chrome instagram cookies",
        }],
        "edge" => vec![BrowserAuthDefinition {
            browser_id: "edge",
            browser_label: "Microsoft Edge",
            label: "edge instagram cookies",
        }],
        "firefox" => vec![BrowserAuthDefinition {
            browser_id: "firefox",
            browser_label: "Firefox",
            label: "firefox instagram cookies",
        }],
        _ => Vec::new(),
    }
}

fn browser_auth_attempts(browser_id: &str, save: bool) -> Vec<GalleryAuthAttempt> {
    browser_auth_definitions(browser_id)
        .into_iter()
        .map(|definition| GalleryAuthAttempt::BrowserCookies {
            browser_id: definition.browser_id,
            browser_label: definition.browser_label,
            label: definition.label,
            save,
        })
        .collect()
}

impl GalleryAuthAttempt {
    fn label(&self) -> &'static str {
        match self {
            GalleryAuthAttempt::None => "public",
            GalleryAuthAttempt::RegisteredTwitterCookies => "registered X/Twitter cookies",
            GalleryAuthAttempt::BrowserCookies { label, .. } => label,
            GalleryAuthAttempt::PreparedInstagramCookies { .. } => "prepared instagram cookies",
            GalleryAuthAttempt::SavedInstagramCookies => "saved instagram cookies",
        }
    }
}

fn gallery_auth_attempts(auth_mode: Option<&str>) -> Vec<GalleryAuthAttempt> {
    let clean = auth_mode.unwrap_or("browserAuto").trim();

    if clean == "registered:twitter" {
        return vec![GalleryAuthAttempt::RegisteredTwitterCookies];
    }

    if clean == "saved:instagram" {
        return vec![GalleryAuthAttempt::SavedInstagramCookies];
    }

    if let Some(token) = clean.strip_prefix("prepared:instagram:") {
        let token = token.trim();
        if !token.is_empty() {
            return vec![GalleryAuthAttempt::PreparedInstagramCookies {
                token: token.to_string(),
            }];
        }
    }

    if let Some(rest) = clean.strip_prefix("browser:") {
        let (browser_id, save) = rest
            .strip_suffix(":save")
            .map(|value| (value, true))
            .unwrap_or((rest, false));
        let attempts = browser_auth_attempts(browser_id, save);
        if !attempts.is_empty() {
            return attempts;
        }
    }

    // An unspecified/legacy `browserAuto` mode is deliberately public-only.
    // Reading browser profiles is permitted only after the frontend has
    // selected one explicitly (or supplied a saved/prepared Instagram jar).
    vec![GalleryAuthAttempt::None]
}

fn cookie_browser_definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        ("opera_gx", "Opera GX"),
        ("opera", "Opera"),
        ("chrome", "Google Chrome"),
        ("edge", "Microsoft Edge"),
        ("firefox", "Firefox"),
    ]
}

fn cookie_browser_label(browser_id: &str) -> &'static str {
    cookie_browser_definitions()
        .into_iter()
        .find_map(|(id, label)| (id == browser_id).then_some(label))
        .unwrap_or("Tarayici")
}

fn cookie_browser_profile_root(browser_id: &str) -> Option<PathBuf> {
    match browser_id {
        "opera_gx" => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Opera Software").join("Opera GX Stable")),
        "opera" => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Opera Software").join("Opera Stable")),
        "chrome" => std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Google").join("Chrome").join("User Data")),
        "edge" => std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Microsoft").join("Edge").join("User Data")),
        "firefox" => std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Mozilla").join("Firefox")),
        _ => None,
    }
}

fn ytdlp_cookie_browser_spec(browser_id: &str) -> Result<String, String> {
    let clean = browser_id.trim();
    match clean {
        "chrome" | "edge" | "firefox" | "opera" => Ok(clean.to_string()),
        "opera_gx" => cookie_browser_profile_root(clean)
            .filter(|path| path.is_dir())
            .map(|path| format!("opera:{}", path.to_string_lossy()))
            .ok_or_else(|| "Opera GX profil klasörü bulunamadı.".to_string()),
        _ => Err("Desteklenmeyen YouTube oturum tarayıcısı seçildi.".to_string()),
    }
}

fn is_youtube_cookie_host(host: &str) -> bool {
    let clean = host.trim().trim_matches('.').to_ascii_lowercase();
    clean == "youtube.com" || clean.ends_with(".youtube.com")
}

fn is_twitter_cookie_host(host: &str) -> bool {
    let clean = host.trim().trim_matches('.').to_ascii_lowercase();
    clean == "x.com"
        || clean.ends_with(".x.com")
        || clean == "twitter.com"
        || clean.ends_with(".twitter.com")
}

fn filtered_ytdlp_cookie_text(url: &str, text: &str) -> Result<String, String> {
    let youtube = is_youtube_url(url);
    let twitter = is_twitter_url(url);
    if !youtube && !twitter {
        return Err("Bu platform için tarayıcı oturumu desteklenmiyor.".to_string());
    }

    let now = unix_time_seconds();
    let cookies = text
        .lines()
        .filter_map(parse_netscape_cookie_line)
        .filter(|cookie| {
            (if youtube {
                is_youtube_cookie_host(&cookie.domain)
            } else {
                is_twitter_cookie_host(&cookie.domain)
            })
                && !cookie.name.trim().is_empty()
                && !cookie.value.trim().is_empty()
                && (cookie.expires <= 0 || cookie.expires > now)
        })
        .collect::<Vec<_>>();

    if cookies.is_empty() {
        return Err(if twitter {
            structured_backend_error(
                "twitter_auth_failed",
                "Seçili tarayıcıda kullanılabilir X/Twitter oturumu bulunamadı. X'e giriş yaptığın başka bir tarayıcı seç.",
            )
        } else {
            "Seçili tarayıcıda kullanılabilir YouTube oturum cookie'si bulunamadı.".to_string()
        });
    }

    if twitter
        && !cookies
            .iter()
            .any(|cookie| cookie.name.eq_ignore_ascii_case("auth_token"))
    {
        return Err(structured_backend_error(
            "twitter_auth_failed",
            "Seçili tarayıcıda giriş yapılmış bir X/Twitter oturumu bulunamadı. X'e giriş yaptığın başka bir tarayıcı seç.",
        ));
    }

    Ok(cookie_jar_to_netscape(&cookies))
}

fn register_ytdlp_cookie_jar(url: &str, browser_id: &str, text: &str) -> Result<(), String> {
    ytdlp_cookie_browser_spec(browser_id)?;
    let cookie = PreparedYtdlpCookie {
        text: filtered_ytdlp_cookie_text(url, text)?,
        browser_id: browser_id.trim().to_string(),
    };
    prepared_ytdlp_cookies()
        .lock()
        .map_err(|_| "Tarayıcı oturum cookie'si kaydedilemedi.".to_string())?
        .insert(url.trim().to_string(), cookie);
    if is_youtube_url(url) {
        invalidate_youtube_analysis(url);
    }
    Ok(())
}

fn registered_ytdlp_cookie(url: &str) -> Option<PreparedYtdlpCookie> {
    prepared_ytdlp_cookies()
        .lock()
        .ok()?
        .get(url.trim())
        .cloned()
}

fn add_registered_ytdlp_cookies(
    command: &mut Command,
    url: &str,
) -> Result<Option<TempArtifact>, String> {
    if !is_youtube_url(url) && !is_twitter_url(url) {
        return Ok(None);
    }

    let Some(cookie) = registered_ytdlp_cookie(url) else {
        return Ok(None);
    };
    let artifact = TempArtifact::write(
        &std::env::temp_dir(),
        &format!("mediadrop-ytdlp-cookies-{}-", cookie.browser_id),
        ".txt",
        cookie.text.as_bytes(),
    )?;
    command.arg("--cookies").arg(artifact.path());
    Ok(Some(artifact))
}

fn cookie_browser_installed(browser_id: &str) -> bool {
    cookie_browser_profile_root(browser_id)
        .map(|path| path.is_dir())
        .unwrap_or(false)
}

fn cookie_browser_id_from_progid(value: &str) -> Option<&'static str> {
    let lower = value.to_lowercase();

    if lower.contains("operagx") || lower.contains("opera gx") {
        Some("opera_gx")
    } else if lower.contains("opera") {
        Some("opera")
    } else if lower.contains("chrome") {
        Some("chrome")
    } else if lower.contains("msegehtm") || lower.contains("edge") {
        Some("edge")
    } else if lower.contains("firefox") {
        Some("firefox")
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn default_cookie_browser_id() -> Option<&'static str> {
    let mut command = hidden_command("reg");
    command.args([
        "query",
        r"HKEY_CURRENT_USER\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        "/v",
        "ProgId",
    ]);

    let output = capture_command_with_timeout(command, Duration::from_secs(2)).ok()?;
    if !output.status.success() {
        return None;
    }

    cookie_browser_id_from_progid(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "windows"))]
fn default_cookie_browser_id() -> Option<&'static str> {
    None
}

#[tauri::command]
fn list_cookie_browsers() -> Vec<CookieBrowserInfo> {
    let definitions = cookie_browser_definitions();
    let default_id = default_cookie_browser_id();
    let installed_ids: HashSet<&str> = definitions
        .iter()
        .filter_map(|(id, _)| cookie_browser_installed(id).then_some(*id))
        .collect();
    let recommended_id = default_id
        .filter(|id| installed_ids.contains(*id))
        .or_else(|| {
            definitions
                .iter()
                .find_map(|(id, _)| installed_ids.contains(*id).then_some(*id))
        })
        .or(default_id)
        .unwrap_or("opera_gx");

    definitions
        .into_iter()
        .map(|(id, label)| CookieBrowserInfo {
            id: id.to_string(),
            label: label.to_string(),
            installed: installed_ids.contains(id),
            recommended: id == recommended_id,
            default_browser: default_id == Some(id),
        })
        .collect()
}

#[derive(Clone, Debug)]
struct BrowserRuntimeProcess {
    pid: u32,
    parent_pid: Option<u32>,
    executable_path: Option<PathBuf>,
}

fn cookie_browser_process_names(browser_id: &str) -> Vec<&'static str> {
    match browser_id {
        "opera_gx" | "opera" => vec!["opera.exe"],
        "chrome" => vec!["chrome.exe"],
        "edge" => vec!["msedge.exe"],
        "firefox" => vec!["firefox.exe"],
        _ => Vec::new(),
    }
}

fn cookie_browser_process_path_matches(browser_id: &str, path: Option<&Path>) -> bool {
    let Some(path) = path else {
        return !matches!(browser_id, "opera_gx" | "opera");
    };
    let lower = path.to_string_lossy().to_ascii_lowercase();

    match browser_id {
        "opera_gx" => lower.contains("opera gx"),
        "opera" => lower.contains("opera") && !lower.contains("opera gx"),
        "chrome" => lower.contains("chrome"),
        "edge" => lower.contains("edge") || lower.contains("msedge"),
        "firefox" => lower.contains("firefox"),
        _ => false,
    }
}

fn parse_wmic_process_list(text: &str, browser_id: &str) -> Vec<BrowserRuntimeProcess> {
    let mut processes = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_pid: Option<u32> = None;
    let mut current_parent_pid: Option<u32> = None;

    let push_current = |processes: &mut Vec<BrowserRuntimeProcess>,
                        current_path: &mut Option<PathBuf>,
                        current_pid: &mut Option<u32>,
                        current_parent_pid: &mut Option<u32>| {
        if let Some(pid) = current_pid.take() {
            if cookie_browser_process_path_matches(browser_id, current_path.as_deref()) {
                processes.push(BrowserRuntimeProcess {
                    pid,
                    parent_pid: *current_parent_pid,
                    executable_path: current_path.clone(),
                });
            }
        }
        *current_path = None;
        *current_parent_pid = None;
    };

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            push_current(
                &mut processes,
                &mut current_path,
                &mut current_pid,
                &mut current_parent_pid,
            );
            continue;
        }

        if let Some(value) = line.strip_prefix("ExecutablePath=") {
            let clean = value.trim();
            current_path = (!clean.is_empty()).then(|| PathBuf::from(clean));
        } else if let Some(value) = line.strip_prefix("ParentProcessId=") {
            current_parent_pid = value.trim().parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("ProcessId=") {
            current_pid = value.trim().parse::<u32>().ok();
        }
    }

    push_current(
        &mut processes,
        &mut current_path,
        &mut current_pid,
        &mut current_parent_pid,
    );

    let mut seen = HashSet::new();
    processes
        .into_iter()
        .filter(|process| seen.insert(process.pid))
        .collect()
}

#[cfg(target_os = "windows")]
fn list_cookie_browser_processes(browser_id: &str) -> Vec<BrowserRuntimeProcess> {
    let mut processes = Vec::new();

    for process_name in cookie_browser_process_names(browser_id) {
        let query = format!("name='{}'", process_name);
        let mut command = hidden_command("wmic.exe");
        command
            .arg("process")
            .arg("where")
            .arg(query)
            .arg("get")
            .arg("ExecutablePath,ParentProcessId,ProcessId")
            .arg("/format:list");

        let Ok(output) = capture_command_with_timeout(command, Duration::from_secs(4)) else {
            continue;
        };
        if !output.status.success() {
            continue;
        }

        processes.extend(parse_wmic_process_list(
            &String::from_utf8_lossy(&output.stdout),
            browser_id,
        ));
    }

    if processes.is_empty() {
        processes.extend(list_cookie_browser_processes_with_powershell(browser_id));
    }

    let mut seen = HashSet::new();
    processes
        .into_iter()
        .filter(|process| seen.insert(process.pid))
        .collect()
}

#[cfg(target_os = "windows")]
fn list_cookie_browser_processes_with_powershell(browser_id: &str) -> Vec<BrowserRuntimeProcess> {
    let names = cookie_browser_process_names(browser_id);
    if names.is_empty() {
        return Vec::new();
    }

    let names_literal = names
        .iter()
        .map(|name| format!("'{}'", name.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$names=@({}); Get-CimInstance Win32_Process | Where-Object {{ $names -contains $_.Name }} | Select-Object ProcessId,ParentProcessId,ExecutablePath | ConvertTo-Json -Compress",
        names_literal
    );
    let mut command = hidden_command("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script);

    let Ok(output) = capture_command_with_timeout(command, Duration::from_secs(5)) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let rows = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        vec![value]
    } else {
        Vec::new()
    };

    rows.into_iter()
        .filter_map(|row| {
            let pid = row
                .get("ProcessId")
                .and_then(|item| item.as_u64())
                .and_then(|value| u32::try_from(value).ok())?;
            let executable_path = row
                .get("ExecutablePath")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            let parent_pid = row
                .get("ParentProcessId")
                .and_then(|item| item.as_u64())
                .and_then(|value| u32::try_from(value).ok());

            cookie_browser_process_path_matches(browser_id, executable_path.as_deref()).then_some(
                BrowserRuntimeProcess {
                    pid,
                    parent_pid,
                    executable_path,
                },
            )
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn list_cookie_browser_processes(_browser_id: &str) -> Vec<BrowserRuntimeProcess> {
    Vec::new()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn cookie_browser_executable_candidates(browser_id: &str) -> Vec<PathBuf> {
    let local = env_path("LOCALAPPDATA");
    let program_files = env_path("PROGRAMFILES");
    let program_files_x86 = env_path("PROGRAMFILES(X86)");
    let mut candidates = Vec::new();

    match browser_id {
        "opera_gx" => {
            if let Some(root) = local.as_ref() {
                candidates.push(root.join("Programs").join("Opera GX").join("opera.exe"));
                candidates.push(root.join("Programs").join("Opera GX").join("launcher.exe"));
            }
        }
        "opera" => {
            if let Some(root) = local.as_ref() {
                candidates.push(root.join("Programs").join("Opera").join("opera.exe"));
                candidates.push(root.join("Programs").join("Opera").join("launcher.exe"));
            }
        }
        "chrome" => {
            if let Some(root) = program_files.as_ref() {
                candidates.push(
                    root.join("Google")
                        .join("Chrome")
                        .join("Application")
                        .join("chrome.exe"),
                );
            }
            if let Some(root) = program_files_x86.as_ref() {
                candidates.push(
                    root.join("Google")
                        .join("Chrome")
                        .join("Application")
                        .join("chrome.exe"),
                );
            }
            if let Some(root) = local.as_ref() {
                candidates.push(
                    root.join("Google")
                        .join("Chrome")
                        .join("Application")
                        .join("chrome.exe"),
                );
            }
        }
        "edge" => {
            if let Some(root) = program_files.as_ref() {
                candidates.push(
                    root.join("Microsoft")
                        .join("Edge")
                        .join("Application")
                        .join("msedge.exe"),
                );
            }
            if let Some(root) = program_files_x86.as_ref() {
                candidates.push(
                    root.join("Microsoft")
                        .join("Edge")
                        .join("Application")
                        .join("msedge.exe"),
                );
            }
            if let Some(root) = local.as_ref() {
                candidates.push(
                    root.join("Microsoft")
                        .join("Edge")
                        .join("Application")
                        .join("msedge.exe"),
                );
            }
        }
        "firefox" => {
            if let Some(root) = program_files.as_ref() {
                candidates.push(root.join("Mozilla Firefox").join("firefox.exe"));
            }
            if let Some(root) = program_files_x86.as_ref() {
                candidates.push(root.join("Mozilla Firefox").join("firefox.exe"));
            }
        }
        _ => {}
    }

    candidates
}

fn extension_browser_url(browser_id: &str) -> Option<&'static str> {
    match browser_id {
        "opera_gx" | "opera" => Some("opera://extensions"),
        "chrome" => Some("chrome://extensions"),
        "edge" => Some("edge://extensions"),
        _ => None,
    }
}

fn extension_browser_executable(browser_id: &str) -> Option<PathBuf> {
    extension_browser_url(browser_id)?;
    cookie_browser_executable_candidates(browser_id)
        .into_iter()
        .find(|path| path.is_file())
}

fn extension_resource_path(app: &tauri::AppHandle) -> ApiResult<PathBuf> {
    let packaged = app
        .path()
        .resource_dir()
        .map_err(|error| {
            ApiError::new(
                "extension_resource_missing",
                format!("Uygulama kaynak klasörü bulunamadı: {error}"),
            )
        })?
        .join("browser-extension");
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("browser-extension")
        .join("dist");

    [packaged, development]
        .into_iter()
        .find(|path| path.join("manifest.json").is_file())
        .ok_or_else(|| {
            ApiError::new(
                "extension_resource_missing",
                "MediaDrop tarayıcı eklentisi kurulum dosyaları bulunamadı.",
            )
        })
}

fn extension_setup_browsers() -> Vec<CookieBrowserInfo> {
    let definitions = [
        ("opera_gx", "Opera GX"),
        ("opera", "Opera"),
        ("chrome", "Google Chrome"),
        ("edge", "Microsoft Edge"),
    ];
    let default_id = default_cookie_browser_id();
    let installed_ids = definitions
        .iter()
        .filter_map(|(id, _)| extension_browser_executable(id).map(|_| *id))
        .collect::<HashSet<_>>();
    let recommended_id = default_id
        .filter(|id| installed_ids.contains(id))
        .or_else(|| {
            definitions
                .iter()
                .find_map(|(id, _)| installed_ids.contains(id).then_some(*id))
        })
        .unwrap_or("opera_gx");

    definitions
        .into_iter()
        .map(|(id, label)| CookieBrowserInfo {
            id: id.to_string(),
            label: label.to_string(),
            installed: installed_ids.contains(id),
            recommended: id == recommended_id,
            default_browser: default_id == Some(id),
        })
        .collect()
}

fn extension_path_text(path: &Path) -> String {
    let text = fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

fn extension_setup_info(app: &tauri::AppHandle) -> ApiResult<ExtensionSetupInfo> {
    let extension_path = extension_resource_path(app)?;
    Ok(ExtensionSetupInfo {
        extension_path: extension_path_text(&extension_path),
        browsers: extension_setup_browsers(),
        connected: companion::companion_extension_connected(),
    })
}

#[tauri::command]
fn get_extension_setup_info(app: tauri::AppHandle) -> ApiResult<ExtensionSetupInfo> {
    extension_setup_info(&app)
}

#[tauri::command]
fn open_extension_setup(
    app: tauri::AppHandle,
    browser_id: String,
) -> ApiResult<ExtensionSetupInfo> {
    let clean_id = browser_id.trim();
    let page = extension_browser_url(clean_id).ok_or_else(|| {
        ApiError::new("browser_unsupported", "Desteklenmeyen tarayıcı seçildi.")
    })?;
    let executable = extension_browser_executable(clean_id).ok_or_else(|| {
        ApiError::new(
            "browser_not_found",
            "Seçilen tarayıcı bu bilgisayarda bulunamadı.",
        )
    })?;

    Command::new(executable)
        .arg(page)
        .spawn()
        .map_err(|error| {
            ApiError::new(
                "browser_open_failed",
                format!("Tarayıcı eklenti sayfası açılamadı: {error}"),
            )
        })?;

    extension_setup_info(&app)
}

fn cookie_browser_relaunch_path(
    browser_id: &str,
    processes: &[BrowserRuntimeProcess],
) -> Option<PathBuf> {
    processes
        .iter()
        .find_map(|process| process.executable_path.clone())
        .filter(|path| path.is_file())
        .or_else(|| {
            cookie_browser_executable_candidates(browser_id)
                .into_iter()
                .find(|path| path.is_file())
        })
}

fn cookie_browser_runtime_state_blocking(browser_id: &str) -> CookieBrowserRuntimeState {
    let label = cookie_browser_label(browser_id).to_string();
    let installed = cookie_browser_installed(browser_id);
    let processes = list_cookie_browser_processes(browser_id);
    let relaunch_path = cookie_browser_relaunch_path(browser_id, &processes);

    CookieBrowserRuntimeState {
        browser_id: browser_id.to_string(),
        label,
        installed,
        running: !processes.is_empty(),
        process_count: processes.len(),
        relaunch_supported: relaunch_path.is_some(),
        executable_path: relaunch_path.map(|path| path.to_string_lossy().to_string()),
    }
}

#[tauri::command]
async fn get_cookie_browser_runtime_state(
    browser_id: String,
) -> ApiResult<CookieBrowserRuntimeState> {
    let result: Result<CookieBrowserRuntimeState, String> =
        tauri::async_runtime::spawn_blocking(move || {
            let clean = browser_id.trim().to_string();
            if browser_auth_definitions(&clean).is_empty() {
                return Err("Desteklenmeyen tarayici secildi.".to_string());
            }
            Ok(cookie_browser_runtime_state_blocking(&clean))
        })
        .await
        .map_err(|err| {
            ApiError::new(
                "thread_error",
                format!("Tarayici durumu thread hatasi: {}", err),
            )
        })?;
    result.map_err(ApiError::from)
}

#[cfg(target_os = "windows")]
fn close_browser_processes(processes: &[BrowserRuntimeProcess], force: bool) -> Result<(), String> {
    let mut errors = Vec::new();
    let process_ids = processes
        .iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    let root_processes = processes
        .iter()
        .filter(|process| {
            process
                .parent_pid
                .map(|parent_pid| !process_ids.contains(&parent_pid))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let targets = if root_processes.is_empty() {
        processes.iter().collect::<Vec<_>>()
    } else {
        root_processes
    };

    for process in targets {
        let mut command = hidden_command("taskkill.exe");
        command.arg("/PID").arg(process.pid.to_string()).arg("/T");
        if force {
            command.arg("/F");
        }

        match capture_command_with_timeout(command, Duration::from_secs(if force { 4 } else { 3 }))
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let text = command_output_text(&output);
                if !text.to_lowercase().contains("not found") {
                    errors.push(format!("{}: {}", process.pid, sanitize_report_text(&text)));
                }
            }
            Err(err) => errors.push(format!("{}: {}", process.pid, err)),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" | "))
    }
}

#[cfg(not(target_os = "windows"))]
fn close_browser_processes(
    _processes: &[BrowserRuntimeProcess],
    _force: bool,
) -> Result<(), String> {
    Err("Tarayici yeniden baslatma yalnizca Windows'ta destekleniyor.".to_string())
}

fn wait_for_cookie_browser_exit(browser_id: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if list_cookie_browser_processes(browser_id).is_empty() {
            return true;
        }
        thread::sleep(Duration::from_millis(180));
    }

    list_cookie_browser_processes(browser_id).is_empty()
}

fn relaunch_cookie_browser(path: &Path) -> Result<(), String> {
    let mut command = hidden_command(path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Tarayici yeniden acilamadi: {}", err))
}

fn close_cookie_browser_for_cookie_read(
    browser_id: &str,
    force: bool,
    auth_failure_code: &str,
) -> Result<Option<PathBuf>, String> {
    let processes = list_cookie_browser_processes(browser_id);
    if processes.is_empty() {
        return Ok(None);
    }
    let label = cookie_browser_label(browser_id);
    let relaunch_path = cookie_browser_relaunch_path(browser_id, &processes).ok_or_else(|| {
        structured_backend_error(
            auth_failure_code,
            &format!(
                "{} yeniden açma yolu bulunamadığı için güvenli biçimde kapatılamadı.",
                label
            ),
        )
    })?;

    if force {
        close_browser_processes(&processes, true)?;
    } else {
        let _ = close_browser_processes(&processes, false);
    }

    if !wait_for_cookie_browser_exit(browser_id, Duration::from_secs(5)) {
        let (code, message) = if force {
            (
                auth_failure_code,
                format!("{} kapatildiktan sonra bile acik gorunuyor.", label),
            )
        } else {
            (
                "browser_still_running",
                format!(
                    "{} kapanmadi. Kaydedilmemis islerin varsa kaydedip zorla kapatmaya izin verebilirsin.",
                    label
                ),
            )
        };
        return Err(structured_backend_error(code, &message));
    }

    thread::sleep(Duration::from_millis(350));
    Ok(Some(relaunch_path))
}

fn instagram_cookie_error_is_browser_lock(error: &str) -> bool {
    let lower = error.to_lowercase();
    [
        "database is locked",
        "database is busy",
        "sharing violation",
        "used by another process",
        "being used by another process",
        "baÅŸka bir iÅŸlem tarafÄ±ndan kullanÄ±ldÄ±ÄŸÄ±ndan",
        "baska bir islem tarafindan kullanildigindan",
        "os error 32",
        "sqlite_busy",
        "sqlite_locked",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[tauri::command]
async fn prepare_instagram_cookie_auth(
    browser_id: String,
    remember: bool,
    restart_browser: bool,
    force_close: bool,
) -> ApiResult<PrepareInstagramCookieAuthResult> {
    let result: Result<PrepareInstagramCookieAuthResult, String> = tauri::async_runtime::spawn_blocking(move || {
        let clean_browser_id = browser_id.trim().to_string();
        let Some(definition) = browser_auth_definitions(&clean_browser_id).first().copied() else {
            return Err("Desteklenmeyen tarayici secildi.".to_string());
        };

        if !cookie_browser_installed(&clean_browser_id) {
            return Err(format!(
                "{} kurulu gorunmuyor. Instagram'a giris yaptigin kurulu tarayiciyi sec.",
                definition.browser_label
            ));
        }

        let initial_processes = list_cookie_browser_processes(&clean_browser_id);
        let was_running = !initial_processes.is_empty();
        let relaunch_path = cookie_browser_relaunch_path(&clean_browser_id, &initial_processes);
        let mut restarted = false;
        // Rookie and the safe SQLite snapshot path can usually read cookies
        // while Chromium/Firefox is open. Restart is only a recovery for a
        // proven file-lock failure after both readers have been exhausted.
        let jar = match best_instagram_cookie_jar(&clean_browser_id, definition.browser_label) {
            Ok(jar) => jar,
            Err(read_error) => {
                if !was_running || !instagram_cookie_error_is_browser_lock(&read_error) {
                    return Err(read_error);
                }
                if !restart_browser {
                    return Err(structured_backend_error(
                        "browser_restart_required",
                        &format!(
                            "{} cookie veritabani baska bir islem tarafindan kilitli. Yeniden baslatma izni gerekiyor.",
                            definition.browser_label
                        ),
                    ));
                }

                if force_close {
                    close_browser_processes(&initial_processes, true)?;
                    if !wait_for_cookie_browser_exit(&clean_browser_id, Duration::from_secs(5)) {
                        return Err(format!(
                            "{} kapatildiktan sonra bile acik gorunuyor.",
                            definition.browser_label
                        ));
                    }
                } else {
                    let _ = close_browser_processes(&initial_processes, false);
                    if !wait_for_cookie_browser_exit(&clean_browser_id, Duration::from_secs(5)) {
                        return Err(structured_backend_error(
                            "browser_still_running",
                            &format!(
                                "{} kapanmadi. Kaydedilmemis islerin varsa kaydedip zorla kapatmaya izin verebilirsin.",
                                definition.browser_label
                            ),
                        ));
                    }
                }

                restarted = true;
                thread::sleep(Duration::from_millis(350));
                best_instagram_cookie_jar(&clean_browser_id, definition.browser_label)?
            }
        };
        let cookie_count = jar.cookies.len();
        let (auth_mode, saved) = if remember {
            save_instagram_cookie_jar(&jar)?;
            ("saved:instagram".to_string(), true)
        } else {
            let token = store_prepared_instagram_cookie_jar(&jar)?;
            (format!("prepared:instagram:{}", token), false)
        };

        let mut relaunched = false;
        let mut relaunch_error = None;
        if restarted {
            if let Some(path) = relaunch_path.as_ref() {
                match relaunch_cookie_browser(path) {
                    Ok(()) => relaunched = true,
                    Err(err) => relaunch_error = Some(err),
                }
            } else {
                relaunch_error = Some(format!(
                    "{} icin yeniden acma yolu bulunamadi.",
                    definition.browser_label
                ));
            }
        }

        let message = if restarted && relaunched {
            format!("{} yeniden baslatildi ve Instagram cerezleri hazirlandi.", definition.browser_label)
        } else if restarted {
            format!(
                "Instagram cerezleri hazirlandi ama {} otomatik acilamadi.",
                definition.browser_label
            )
        } else {
            format!("Instagram cerezleri {} uzerinden hazirlandi.", definition.browser_label)
        };

        Ok(PrepareInstagramCookieAuthResult {
            auth_mode,
            browser_id: clean_browser_id,
            label: definition.browser_label.to_string(),
            saved,
            restarted,
            relaunched,
            relaunch_error,
            cookie_count,
            message,
        })
    })
    .await
    .map_err(|err| ApiError::new("thread_error", format!("Instagram cookie hazirlama thread hatasi: {}", err)))?;
    result.map_err(ApiError::from)
}

fn command_output_text(output: &TimedCommandOutput) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{}\n{}", stdout, stderr).trim().to_string()
}

#[derive(Clone, Debug)]
struct ChromiumCookieCandidate {
    path: PathBuf,
    relative_path: String,
    size: u64,
    modified_ms: u128,
}

fn metadata_modified_ms(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn display_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_chromium_cookie_database(path: &Path) -> bool {
    let is_cookies_file = path
        .file_name()
        .map(|name| name.to_string_lossy().eq_ignore_ascii_case("cookies"))
        .unwrap_or(false);
    let parent_is_network = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|name| name.to_string_lossy().eq_ignore_ascii_case("network"))
        .unwrap_or(false);

    is_cookies_file && parent_is_network
}

fn should_skip_chromium_cookie_scan_dir(path: &Path) -> bool {
    path.file_name()
        .map(|name| {
            matches!(
                name.to_string_lossy().to_ascii_lowercase().as_str(),
                "cache"
                    | "code cache"
                    | "component_crx_cache"
                    | "crash reports"
                    | "extensions_crx_cache"
                    | "graphitedawncache"
                    | "grshadercache"
                    | "shadercache"
                    | "safe browsing"
            )
        })
        .unwrap_or(false)
}

fn collect_chromium_cookie_candidates(
    root: &Path,
    dir: &Path,
    depth: usize,
    candidates: &mut Vec<ChromiumCookieCandidate>,
) {
    if depth > 5 || should_skip_chromium_cookie_scan_dir(dir) {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            collect_chromium_cookie_candidates(root, &path, depth + 1, candidates);
            continue;
        }

        if !file_type.is_file() || !is_chromium_cookie_database(&path) {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            candidates.push(ChromiumCookieCandidate {
                relative_path: display_relative_path(root, &path),
                modified_ms: metadata_modified_ms(&metadata),
                size: metadata.len(),
                path,
            });
        }
    }
}

fn chromium_cookie_candidates(root: &Path) -> Vec<ChromiumCookieCandidate> {
    let mut candidates = Vec::new();
    collect_chromium_cookie_candidates(root, root, 0, &mut candidates);
    candidates.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| b.size.cmp(&a.size))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    candidates
}

fn is_instagram_cookie_host(host: &str) -> bool {
    let clean = host.trim().trim_matches('.').to_ascii_lowercase();

    clean == "instagram.com" || clean.ends_with(".instagram.com")
}

fn chrome_time_to_unix(expires_utc: i64) -> i64 {
    if expires_utc <= 0 {
        0
    } else {
        (expires_utc / 1_000_000).saturating_sub(11_644_473_600)
    }
}

fn cookie_profile_score(cookies: &[NetscapeCookie]) -> u32 {
    let mut score = 0u32;

    for cookie in cookies {
        match cookie.name.as_str() {
            "sessionid" if !cookie.value.is_empty() => score += 140,
            "ds_user_id" if !cookie.value.is_empty() => score += 60,
            "csrftoken" if !cookie.value.is_empty() => score += 28,
            "mid" | "ig_did" | "rur" if !cookie.value.is_empty() => score += 8,
            _ if !cookie.value.is_empty() => score += 1,
            _ => {}
        }
    }

    score
}

fn cookie_jar_has_login_session(cookies: &[NetscapeCookie]) -> bool {
    let now = unix_time_seconds();
    let has_live_cookie = |name: &str| {
        cookies.iter().any(|cookie| {
            cookie.name == name
                && !cookie.value.trim().is_empty()
                && is_instagram_cookie_host(&cookie.domain)
                && (cookie.expires <= 0 || cookie.expires > now)
        })
    };
    has_live_cookie("sessionid") && has_live_cookie("ds_user_id")
}

fn netscape_field(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

fn cookie_jar_to_netscape(cookies: &[NetscapeCookie]) -> String {
    let mut text = String::from("# Netscape HTTP Cookie File\n# Generated by MediaDrop\n");
    let mut sorted = cookies.to_vec();
    sorted.sort_by(|a, b| {
        a.domain
            .cmp(&b.domain)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.name.cmp(&b.name))
    });

    for cookie in sorted {
        if cookie.name.trim().is_empty() || cookie.value.trim().is_empty() {
            continue;
        }

        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            netscape_field(&cookie.domain),
            if cookie.include_subdomains {
                "TRUE"
            } else {
                "FALSE"
            },
            netscape_field(if cookie.path.is_empty() {
                "/"
            } else {
                &cookie.path
            }),
            if cookie.secure { "TRUE" } else { "FALSE" },
            cookie.expires.max(0),
            netscape_field(&cookie.name),
            netscape_field(&cookie.value),
        ));
    }

    text
}

fn materialize_instagram_cookie_jar(jar: &BrowserCookieJar) -> Result<TempArtifact, String> {
    let text = cookie_jar_to_netscape(&jar.cookies);
    if !netscape_cookie_has_instagram_session(&text) {
        return Err(format!(
            "{} icinde Instagram oturum cookie'si bulunamadi. Instagram'a giris yaptigin tarayiciyi sec.",
            jar.browser_label
        ));
    }

    TempArtifact::write(
        &std::env::temp_dir(),
        &format!("mediadrop-instagram-cookie-{}-", jar.browser_id),
        ".txt",
        text.as_bytes(),
    )
}

fn save_instagram_cookie_jar(jar: &BrowserCookieJar) -> Result<(), String> {
    let text = cookie_jar_to_netscape(&jar.cookies);

    if !netscape_cookie_has_instagram_session(&text) {
        return Ok(());
    }

    let protected = dpapi_protect_bytes(text.as_bytes())?;
    fs::write(instagram_cookie_store_path()?, protected)
        .map_err(|err| format!("Instagram cookie kaydi yazilamadi: {}", err))?;

    let meta = json!({
        "browserId": jar.browser_id,
        "label": jar.browser_label,
        "profileLabel": jar.profile_label,
        "updatedAtMs": now_ms(),
        "cookieCount": jar.cookies.len(),
        "profileScore": jar.score,
        "failedDecrypts": jar.failed_decrypts,
    });
    let meta_text = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Instagram cookie meta JSON olusturulamadi: {}", err))?;
    fs::write(instagram_cookie_meta_path()?, meta_text)
        .map_err(|err| format!("Instagram cookie meta yazilamadi: {}", err))?;

    Ok(())
}

fn rookie_browser_cookies(browser_id: &str) -> Result<Vec<rookie::enums::Cookie>, String> {
    let domains = Some(vec!["instagram.com".to_string()]);
    let result = match browser_id {
        "opera_gx" => rookie::opera_gx(domains),
        "opera" => rookie::opera(domains),
        "chrome" => rookie::chrome(domains),
        "edge" => rookie::edge(domains),
        "firefox" => rookie::firefox(domains),
        _ => return Err("rookie bu tarayiciyi desteklemiyor.".to_string()),
    };

    result.map_err(|err| sanitize_report_text(&format!("{:?}", err)))
}

fn rookie_instagram_cookie_jar(
    browser_id: &str,
    browser_label: &str,
) -> Result<BrowserCookieJar, String> {
    let cookies = rookie_browser_cookies(browser_id)?;
    let mut netscape = Vec::new();
    let mut seen = HashSet::new();

    for cookie in cookies {
        if !is_instagram_cookie_host(&cookie.domain) {
            continue;
        }

        let name = cookie.name.trim();
        let value = cookie.value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }

        let path = if cookie.path.trim().is_empty() {
            "/"
        } else {
            cookie.path.trim()
        };
        let key = format!("{}\t{}\t{}\t{}", cookie.domain, path, name, value);
        if !seen.insert(key) {
            continue;
        }

        netscape.push(NetscapeCookie {
            include_subdomains: cookie.domain.trim().starts_with('.'),
            domain: cookie.domain,
            path: path.to_string(),
            secure: cookie.secure,
            expires: cookie
                .expires
                .and_then(|value| i64::try_from(value).ok())
                .unwrap_or(0),
            name: name.to_string(),
            value: value.to_string(),
        });
    }

    if netscape.is_empty() {
        return Err("rookie Instagram cookie dondurmedi.".to_string());
    }

    let score = cookie_profile_score(&netscape);
    if !cookie_jar_has_login_session(&netscape) {
        return Err(format!(
            "rookie {} Instagram cookie okudu ama sessionid bulamadi. cookie sayisi: {}",
            browser_label,
            netscape.len()
        ));
    }

    Ok(BrowserCookieJar {
        browser_id: browser_id.to_string(),
        browser_label: browser_label.to_string(),
        profile_label: "rookie".to_string(),
        cookies: netscape,
        score,
        failed_decrypts: 0,
    })
}

fn snapshot_sqlite_database(source: &Path, label: &str) -> Result<PathBuf, String> {
    let target = std::env::temp_dir().join(format!(
        "mediadrop-cookie-snapshot-{}-{}.sqlite",
        label
            .to_ascii_lowercase()
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect::<String>(),
        unique_stamp()
    ));

    #[cfg(target_os = "windows")]
    {
        let _ = fs::remove_file(&target);
        let backup_result = (|| -> Result<PathBuf, String> {
            sqlite_win::backup_to(source, &target)
                .map_err(|err| format!("SQLite cookie snapshot alinamadi: {}", err))?;
            Ok(target.clone())
        })();

        match backup_result {
            Ok(path) => Ok(path),
            Err(backup_err) => copy_sqlite_database_snapshot(source, &target).map_err(|copy_err| {
                format!("{}; kopya snapshot da alinamadi: {}", backup_err, copy_err)
            }),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        copy_sqlite_database_snapshot(source, &target)
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "Cookies".to_string());
    path.with_file_name(format!("{}{}", file_name, suffix))
}

fn copy_sqlite_database_snapshot(source: &Path, target: &Path) -> Result<PathBuf, String> {
    let _ = fs::remove_file(target);
    copy_possibly_locked_file(source, target, "SQLite cookie dosyasi")?;

    for suffix in ["-wal", "-shm", "-journal"] {
        let source_sidecar = sqlite_sidecar_path(source, suffix);
        if !source_sidecar.is_file() {
            continue;
        }

        let target_sidecar = sqlite_sidecar_path(target, suffix);
        let _ = fs::remove_file(&target_sidecar);
        let _ = copy_possibly_locked_file(
            &source_sidecar,
            &target_sidecar,
            &format!("SQLite yan dosyasi {}", suffix),
        );
    }

    Ok(target.to_path_buf())
}

#[cfg(target_os = "windows")]
fn copy_file_with_windows_shared_read(
    source: &Path,
    target: &Path,
    label: &str,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("{} hedef klasoru olusturulamadi: {}", label, err))?;
    }

    let wide_path: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    const WINDOWS_GENERIC_READ: u32 = 0x8000_0000;
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            WINDOWS_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "{} paylasimli okuma ile acilamadi: {}",
            label,
            std::io::Error::last_os_error()
        ));
    }

    let result = (|| -> Result<(), String> {
        let mut output = fs::File::create(target)
            .map_err(|err| format!("{} hedef dosyasi olusturulamadi: {}", label, err))?;
        let mut buffer = vec![0u8; 1024 * 1024];

        loop {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                    ptr::null_mut(),
                )
            };

            if ok == 0 {
                return Err(format!(
                    "{} paylasimli okuma sirasinda hata verdi: {}",
                    label,
                    std::io::Error::last_os_error()
                ));
            }

            if read == 0 {
                break;
            }

            output
                .write_all(&buffer[..read as usize])
                .map_err(|err| format!("{} hedef dosyasina yazilamadi: {}", label, err))?;
        }

        output
            .flush()
            .map_err(|err| format!("{} hedef dosyasi tamamlanamadi: {}", label, err))?;
        Ok(())
    })();

    unsafe {
        CloseHandle(handle);
    }

    result
}

fn copy_possibly_locked_file(source: &Path, target: &Path, label: &str) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("{} hedef klasoru olusturulamadi: {}", label, err))?;
    }

    match fs::copy(source, target) {
        Ok(_) => Ok(()),
        Err(copy_err) => {
            #[cfg(target_os = "windows")]
            {
                let shared_read_result = copy_file_with_windows_shared_read(source, target, label);
                let shared_read_error = match shared_read_result {
                    Ok(()) if target.is_file() => return Ok(()),
                    Ok(()) => format!("{} paylasimli kopya hedef dosyayi olusturmadi", label),
                    Err(err) => err,
                };

                let mut command = hidden_command("esentutl.exe");
                command
                    .arg("/y")
                    .arg(source)
                    .arg("/d")
                    .arg(target)
                    .arg("/o");
                let output = capture_command_with_timeout(command, Duration::from_secs(20))
                    .map_err(|err| {
                        format!(
                            "{} normal kopya kilitlendi: {}; esentutl calistirilamadi: {}",
                            label, copy_err, err
                        )
                    })?;

                if output.status.success() && target.is_file() {
                    return Ok(());
                }

                Err(format!(
                    "{} kopyalanamadi. Normal kopya: {}; paylasimli kopya: {}; esentutl cikis kodu: {}",
                    label,
                    copy_err,
                    shared_read_error,
                    output.status.code().unwrap_or(-1)
                ))
            }

            #[cfg(not(target_os = "windows"))]
            {
                Err(format!("{} kopyalanamadi: {}", label, copy_err))
            }
        }
    }
}

fn cleanup_sqlite_snapshot(path: &Path) {
    let _ = fs::remove_file(path);
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(sqlite_sidecar_path(path, suffix));
    }
}

fn chromium_local_state_path(profile_root: &Path) -> PathBuf {
    profile_root.join("Local State")
}

#[cfg(target_os = "windows")]
fn chromium_master_key(profile_root: &Path) -> Result<Vec<u8>, String> {
    let path = chromium_local_state_path(profile_root);
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("Chromium Local State okunamadi: {}", err))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| format!("Chromium Local State JSON okunamadi: {}", err))?;
    let encoded = value
        .get("os_crypt")
        .and_then(|item| item.get("encrypted_key"))
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Chromium master key bulunamadi.".to_string())?;
    let mut encrypted = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|err| format!("Chromium master key base64 cozulmedi: {}", err))?;

    if encrypted.starts_with(b"DPAPI") {
        encrypted.drain(..5);
    }

    dpapi_unprotect_bytes(&encrypted)
}

#[cfg(not(target_os = "windows"))]
fn chromium_master_key(_profile_root: &Path) -> Result<Vec<u8>, String> {
    Err("Chromium cookie cozumu yalnizca Windows'ta destekleniyor.".to_string())
}

#[cfg(target_os = "windows")]
fn decrypt_chromium_cookie_value(
    host_key: &str,
    encrypted_value: &[u8],
    plain_value: &str,
    master_key: &[u8],
    meta_version: i32,
) -> Result<String, String> {
    if !plain_value.is_empty() {
        return Ok(plain_value.to_string());
    }

    if encrypted_value.is_empty() {
        return Ok(String::new());
    }

    let is_chromium_aes_cookie = encrypted_value.starts_with(b"v10")
        || encrypted_value.starts_with(b"v11")
        || encrypted_value.starts_with(b"v20");

    let mut decrypted = if is_chromium_aes_cookie && encrypted_value.len() > 3 + 12 + 16 {
        let nonce = &encrypted_value[3..15];
        let tag_start = encrypted_value.len().saturating_sub(16);
        let ciphertext = &encrypted_value[15..tag_start];
        let tag = &encrypted_value[tag_start..];
        aes_gcm_decrypt(master_key, nonce, ciphertext, tag)?
    } else {
        dpapi_unprotect_bytes(encrypted_value)?
    };

    if meta_version >= 24 && decrypted.len() >= 32 {
        decrypted.drain(..32);
    } else if meta_version >= 24 && !host_key.is_empty() {
        return Err("Chromium cookie host hash eksik.".to_string());
    }

    String::from_utf8(decrypted).map_err(|_| "Chromium cookie UTF-8 degil.".to_string())
}

#[cfg(not(target_os = "windows"))]
fn decrypt_chromium_cookie_value(
    _host_key: &str,
    _encrypted_value: &[u8],
    plain_value: &str,
    _master_key: &[u8],
    _meta_version: i32,
) -> Result<String, String> {
    Ok(plain_value.to_string())
}

fn read_sqlite_meta_version(snapshot: &Path) -> i32 {
    #[cfg(target_os = "windows")]
    {
        sqlite_win::read_meta_version(snapshot).unwrap_or(0)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = snapshot;
        0
    }
}

fn read_chromium_cookie_snapshot(
    browser_id: &str,
    browser_label: &str,
    _profile_root: &Path,
    candidate: &ChromiumCookieCandidate,
    master_key: &[u8],
) -> Result<BrowserCookieJar, String> {
    #[cfg(target_os = "windows")]
    {
        let snapshot = snapshot_sqlite_database(&candidate.path, browser_id)?;
        let result = (|| -> Result<BrowserCookieJar, String> {
            let meta_version = read_sqlite_meta_version(&snapshot);
            let rows = sqlite_win::read_chromium_cookie_rows(&snapshot)?;
            let mut cookies = Vec::new();
            let mut failed_decrypts = 0usize;

            for row in rows {
                let host_key = row.host_key;
                if !is_instagram_cookie_host(&host_key) {
                    continue;
                }

                let name = row.name;
                let value = match decrypt_chromium_cookie_value(
                    &host_key,
                    &row.encrypted_value,
                    &row.plain_value,
                    master_key,
                    meta_version,
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        failed_decrypts += 1;
                        continue;
                    }
                };

                if name.trim().is_empty() || value.trim().is_empty() {
                    continue;
                }

                let domain = if host_key.starts_with('.') {
                    host_key
                } else {
                    format!(".{}", host_key.trim_start_matches('.'))
                };

                cookies.push(NetscapeCookie {
                    include_subdomains: true,
                    domain,
                    path: {
                        let path = row.path;
                        if path.trim().is_empty() {
                            "/".to_string()
                        } else {
                            path
                        }
                    },
                    expires: chrome_time_to_unix(row.expires_utc),
                    secure: row.is_secure,
                    name,
                    value,
                });
            }

            let score = cookie_profile_score(&cookies);
            Ok(BrowserCookieJar {
                browser_id: browser_id.to_string(),
                browser_label: browser_label.to_string(),
                profile_label: candidate.relative_path.clone(),
                cookies,
                score,
                failed_decrypts,
            })
        })();
        cleanup_sqlite_snapshot(&snapshot);
        result
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            browser_id,
            browser_label,
            profile_root,
            candidate,
            master_key,
        );
        Err("Chromium cookie okuma yalnizca Windows'ta destekleniyor.".to_string())
    }
}

#[derive(Clone, Debug)]
struct FirefoxCookieCandidate {
    path: PathBuf,
    relative_path: String,
    modified_ms: u128,
}

fn collect_firefox_cookie_candidates(
    root: &Path,
    dir: &Path,
    depth: usize,
    candidates: &mut Vec<FirefoxCookieCandidate>,
) {
    if depth > 5 {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            collect_firefox_cookie_candidates(root, &path, depth + 1, candidates);
            continue;
        }

        let is_cookie_db = file_type.is_file()
            && path
                .file_name()
                .map(|name| {
                    name.to_string_lossy()
                        .eq_ignore_ascii_case("cookies.sqlite")
                })
                .unwrap_or(false);

        if !is_cookie_db {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            candidates.push(FirefoxCookieCandidate {
                relative_path: display_relative_path(root, &path),
                modified_ms: metadata_modified_ms(&metadata),
                path,
            });
        }
    }
}

fn firefox_cookie_candidates(root: &Path) -> Vec<FirefoxCookieCandidate> {
    let mut candidates = Vec::new();
    collect_firefox_cookie_candidates(root, root, 0, &mut candidates);
    candidates.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    candidates
}

fn read_firefox_cookie_snapshot(
    browser_id: &str,
    browser_label: &str,
    candidate: &FirefoxCookieCandidate,
) -> Result<BrowserCookieJar, String> {
    #[cfg(target_os = "windows")]
    {
        let snapshot = snapshot_sqlite_database(&candidate.path, browser_id)?;
        let result = (|| -> Result<BrowserCookieJar, String> {
            let rows = sqlite_win::read_firefox_cookie_rows(&snapshot)?;
            let mut cookies = Vec::new();

            for row in rows {
                let host = row.host;
                if !is_instagram_cookie_host(&host) {
                    continue;
                }

                let name = row.name;
                let value = row.value;
                if name.trim().is_empty() || value.trim().is_empty() {
                    continue;
                }

                let domain = if host.starts_with('.') {
                    host
                } else {
                    format!(".{}", host.trim_start_matches('.'))
                };

                cookies.push(NetscapeCookie {
                    domain,
                    include_subdomains: true,
                    path: {
                        let path = row.path;
                        if path.trim().is_empty() {
                            "/".to_string()
                        } else {
                            path
                        }
                    },
                    expires: row.expiry,
                    secure: row.is_secure,
                    name,
                    value,
                });
            }

            let score = cookie_profile_score(&cookies);
            Ok(BrowserCookieJar {
                browser_id: browser_id.to_string(),
                browser_label: browser_label.to_string(),
                profile_label: candidate.relative_path.clone(),
                cookies,
                score,
                failed_decrypts: 0,
            })
        })();
        cleanup_sqlite_snapshot(&snapshot);
        result
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (browser_id, browser_label, candidate);
        Err("Firefox cookie okuma yalnizca Windows'ta destekleniyor.".to_string())
    }
}

fn best_instagram_cookie_jar(
    browser_id: &str,
    browser_label: &str,
) -> Result<BrowserCookieJar, String> {
    let mut diagnostics = Vec::new();

    match rookie_instagram_cookie_jar(browser_id, browser_label) {
        Ok(jar) => return Ok(jar),
        Err(err) => diagnostics.push(format!("rookie: {}", sanitize_report_text(&err))),
    }

    let root = cookie_browser_profile_root(browser_id)
        .ok_or_else(|| format!("{} profil klasoru bulunamadi.", browser_label))?;

    if !root.is_dir() {
        return Err(format!(
            "{} kurulu gorunmuyor veya profil klasoru bulunamadi: {}",
            browser_label,
            root.to_string_lossy()
        ));
    }

    let mut jars = Vec::new();

    if browser_id == "firefox" {
        let candidates = firefox_cookie_candidates(&root);
        if candidates.is_empty() {
            return Err(format!(
                "{} cookies.sqlite bulunamadi. Instagram'a giris yaptigin tarayiciyi sec.",
                browser_label
            ));
        }

        for candidate in candidates.iter().take(12) {
            match read_firefox_cookie_snapshot(browser_id, browser_label, candidate) {
                Ok(jar) => jars.push(jar),
                Err(err) => diagnostics.push(format!(
                    "{}: {}",
                    candidate.relative_path,
                    sanitize_report_text(&err)
                )),
            }
        }
    } else {
        let master_key = chromium_master_key(&root)?;
        let candidates = chromium_cookie_candidates(&root);
        if candidates.is_empty() {
            return Err(format!(
                "{} cookie veritabani bulunamadi. Instagram'a giris yaptigin tarayiciyi sec.",
                browser_label
            ));
        }

        for candidate in candidates.iter().take(16) {
            match read_chromium_cookie_snapshot(
                browser_id,
                browser_label,
                &root,
                candidate,
                &master_key,
            ) {
                Ok(jar) => jars.push(jar),
                Err(err) => diagnostics.push(format!(
                    "{}: {}",
                    candidate.relative_path,
                    sanitize_report_text(&err)
                )),
            }
        }
    }

    jars.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                cookie_jar_has_login_session(&b.cookies)
                    .cmp(&cookie_jar_has_login_session(&a.cookies))
            })
            .then_with(|| b.cookies.len().cmp(&a.cookies.len()))
            .then_with(|| a.profile_label.cmp(&b.profile_label))
    });

    let Some(best) = jars.into_iter().next() else {
        return Err(format!(
            "{} profil cookie'leri okunamadi. Ayrinti: {}",
            browser_label,
            diagnostics.join(" | ")
        ));
    };

    if !cookie_jar_has_login_session(&best.cookies) {
        let candidate_summary = format!(
            "okunan profil: {}, cookie sayisi: {}, cozulemeyen cookie: {}",
            best.profile_label,
            best.cookies.len(),
            best.failed_decrypts
        );
        return Err(format!(
            "Secili tarayicida Instagram oturumu bulunamadi. Instagram'a giris yaptigin tarayiciyi sec. {}",
            candidate_summary
        ));
    }

    Ok(best)
}

fn run_gallerydl_dump(
    gallery_dl: &Path,
    clean_url: &str,
    auth: GalleryAuthAttempt,
    resolve_urls: bool,
    instagram_story_mode: bool,
) -> Result<String, String> {
    let mut command = hidden_command(gallery_dl);
    let mut cleanup_files = Vec::new();
    command.arg("--dump-json");

    // A direct Instagram post normally exposes only the author's basic fields.
    // Asking gallery-dl for extended user metadata also makes the profile image
    // available without turning the avatar itself into a downloadable item.
    if is_instagram_url(clean_url) {
        command
            .arg("--option")
            .arg("extractor.instagram.metadata=true");
        if instagram_story_mode {
            command
                .arg("--option")
                .arg("extractor.instagram.include=stories")
                .arg("--option")
                .arg("extractor.instagram.static-videos=false")
                .arg("--option")
                .arg("extractor.instagram.videos=merged");
        }
    }

    if is_twitter_url(clean_url) {
        command
            .arg("--option")
            .arg("extractor.twitter.quoted=true")
            .arg("--option")
            .arg("extractor.twitter.text-tweets=true")
            .arg("--option")
            .arg("extractor.twitter.previews=true");
    }

    if resolve_urls {
        command.arg("--resolve-json");
    }

    match auth {
        GalleryAuthAttempt::None => {}
        GalleryAuthAttempt::RegisteredTwitterCookies => {
            command
                .arg("--option")
                .arg("extractor.twitter.tweet-endpoint=detail")
                .arg("--option")
                .arg("extractor.twitter.cards=true");
            let cookie_path = add_registered_ytdlp_cookies(&mut command, clean_url)?
                .ok_or_else(|| {
                    structured_backend_error(
                        "twitter_auth_failed",
                        "X/Twitter oturumu bu bağlantı için hazırlanamadı. Linki yeniden analiz et.",
                    )
                })?;
            cleanup_files.push(cookie_path);
        }
        GalleryAuthAttempt::BrowserCookies {
            browser_id,
            browser_label,
            save,
            ..
        } => {
            let jar = best_instagram_cookie_jar(browser_id, browser_label)?;
            let cookie_path = materialize_instagram_cookie_jar(&jar)?;
            if save {
                save_instagram_cookie_jar(&jar)?;
            }
            command.arg("--cookies").arg(cookie_path.path());
            cleanup_files.push(cookie_path);
        }
        GalleryAuthAttempt::PreparedInstagramCookies { token } => {
            let cookie_path = materialize_prepared_instagram_cookie_file(&token)?;
            command.arg("--cookies").arg(cookie_path.path());
            cleanup_files.push(cookie_path);
        }
        GalleryAuthAttempt::SavedInstagramCookies => {
            let cookie_path = materialize_saved_instagram_cookie_file()?;
            command.arg("--cookies").arg(cookie_path.path());
            cleanup_files.push(cookie_path);
        }
    }

    command.arg(clean_url);

    let output_result = capture_command_with_timeout(command, Duration::from_secs(75));

    let output = match output_result {
        Ok(output) => output,
        Err(err) => {
            drop(cleanup_files);
            return Err(format!("gallery-dl analiz komutu tamamlanamadi: {}", err));
        }
    };

    let text = command_output_text(&output);

    if !output.status.success() {
        drop(cleanup_files);
        return Err(if text.is_empty() {
            "gallery-dl analiz sirasinda hata verdi.".to_string()
        } else {
            text
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if stdout.is_empty() {
        drop(cleanup_files);
        Err("gallery-dl bos analiz sonucu dondurdu.".to_string())
    } else {
        drop(cleanup_files);

        Ok(stdout)
    }
}

fn gallerydl_dump_with_fallback(
    gallery_dl: &Path,
    clean_url: &str,
    auth: GalleryAuthAttempt,
    instagram_story_mode: bool,
) -> Result<String, String> {
    match run_gallerydl_dump(
        gallery_dl,
        clean_url,
        auth.clone(),
        true,
        instagram_story_mode,
    ) {
        Ok(stdout) => Ok(stdout),
        Err(err) => {
            let lower = err.to_lowercase();
            if lower.contains("resolve-json")
                || lower.contains("resolve-urls")
                || lower.contains("unrecognized")
                || lower.contains("unknown option")
            {
                run_gallerydl_dump(gallery_dl, clean_url, auth, false, instagram_story_mode)
            } else {
                Err(err)
            }
        }
    }
}

fn media_platform_from_url(url: &str) -> String {
    if is_instagram_url(url) {
        "instagram"
    } else if is_twitter_url(url) {
        "twitter"
    } else if is_tiktok_url(url) {
        "tiktok"
    } else if is_youtube_url(url) {
        "youtube"
    } else {
        "generic"
    }
    .to_string()
}

fn gallery_media_title(platform: &str, _url: &str) -> String {
    match platform {
        "instagram" => "Instagram medyasi",
        "twitter" => "X medyasi",
        "tiktok" => "TikTok medyasi",
        _ => "MediaDrop medyasi",
    }
    .to_string()
}

fn instagram_public_post_path(url: &str) -> Option<(String, String)> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    if !host.ends_with("instagram.com") {
        return None;
    }

    let segments: Vec<_> = parsed.path_segments()?.collect();
    if segments.len() < 2 {
        return None;
    }

    let kind = segments[0].trim().to_lowercase();
    if !matches!(kind.as_str(), "p" | "reel" | "tv") {
        return None;
    }

    let shortcode = segments[1].trim().trim_matches('/');
    if shortcode.is_empty() {
        None
    } else {
        Some((kind, shortcode.to_string()))
    }
}

fn instagram_public_post_url(url: &str) -> Option<String> {
    let (kind, shortcode) = instagram_public_post_path(url)?;
    Some(format!("https://www.instagram.com/{}/{}/", kind, shortcode))
}

fn instagram_public_json_url(url: &str) -> Option<String> {
    let public_url = instagram_public_post_url(url)?;
    let mut parsed = reqwest::Url::parse(&public_url).ok()?;
    parsed
        .query_pairs_mut()
        .append_pair("__a", "1")
        .append_pair("__d", "dis");
    Some(parsed.to_string())
}

fn instagram_public_embed_urls(url: &str) -> Vec<(&'static str, String)> {
    let Some(public_url) = instagram_public_post_url(url) else {
        return Vec::new();
    };

    [
        ("instagram embed", "embed/"),
        ("instagram embed captioned", "embed/captioned/"),
    ]
    .into_iter()
    .map(|(label, suffix)| (label, format!("{}{}", public_url, suffix)))
    .collect()
}

fn instagram_public_oembed_url(url: &str) -> Option<String> {
    let public_url = instagram_public_post_url(url)?;
    let mut parsed = reqwest::Url::parse("https://www.instagram.com/oembed/").ok()?;
    parsed.query_pairs_mut().append_pair("url", &public_url);
    Some(parsed.to_string())
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn html_attr_value(tag: &str, attr: &str) -> Option<String> {
    let chars: Vec<char> = tag.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        while index < chars.len()
            && (chars[index].is_whitespace() || matches!(chars[index], '<' | '/' | '>'))
        {
            index += 1;
        }

        let name_start = index;
        while index < chars.len()
            && !chars[index].is_whitespace()
            && !matches!(chars[index], '=' | '/' | '>')
        {
            index += 1;
        }

        if name_start == index {
            index += 1;
            continue;
        }

        let name: String = chars[name_start..index].iter().collect();
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }

        if index >= chars.len() || chars[index] != '=' {
            continue;
        }

        index += 1;
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }

        if index >= chars.len() {
            break;
        }

        let value = if matches!(chars[index], '"' | '\'') {
            let quote = chars[index];
            index += 1;
            let value_start = index;
            while index < chars.len() && chars[index] != quote {
                index += 1;
            }
            let value: String = chars[value_start..index].iter().collect();
            index += usize::from(index < chars.len());
            value
        } else {
            let value_start = index;
            while index < chars.len() && !chars[index].is_whitespace() && chars[index] != '>' {
                index += 1;
            }
            chars[value_start..index].iter().collect()
        };

        if name.eq_ignore_ascii_case(attr) {
            return Some(html_unescape(&value));
        }
    }

    None
}

fn find_meta_content(html: &str, key: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut offset = 0;

    while let Some(relative_start) = lower[offset..].find("<meta") {
        let start = offset + relative_start;
        let Some(relative_end) = lower[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let tag = &html[start..end];
        let property = html_attr_value(tag, "property");
        let name = html_attr_value(tag, "name");
        let itemprop = html_attr_value(tag, "itemprop");
        let matches_key = [property.as_deref(), name.as_deref(), itemprop.as_deref()]
            .into_iter()
            .flatten()
            .any(|value| value.eq_ignore_ascii_case(key));

        if matches_key {
            if let Some(content) = html_attr_value(tag, "content") {
                if !content.trim().is_empty() {
                    return Some(content);
                }
            }
        }

        offset = end;
    }

    None
}

fn decode_embedded_url(raw: &str) -> String {
    html_unescape(
        raw.trim()
            .trim_matches('\\')
            .replace("\\/", "/")
            .replace("\\u0026", "&")
            .replace("\\u003d", "=")
            .replace("\\u002F", "/")
            .as_str(),
    )
}

fn instagram_cdn_host_allowed(host: &str) -> bool {
    let clean = host.trim().trim_matches('.').to_ascii_lowercase();
    clean == "cdninstagram.com"
        || clean.ends_with(".cdninstagram.com")
        || clean == "fbcdn.net"
        || clean.ends_with(".fbcdn.net")
}

fn instagram_avatar_url_allowed(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("");
    let path = parsed.path().to_lowercase();
    parsed.scheme() == "https"
        && instagram_cdn_host_allowed(host)
        && !host.to_ascii_lowercase().starts_with("static.")
        && !path.contains("/rsrc.php")
}

fn instagram_avatar_host_class(url: &reqwest::Url) -> String {
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim()
        .trim_matches('.')
        .to_ascii_lowercase();
    if host == "cdninstagram.com" || host.ends_with(".cdninstagram.com") {
        "instagram_cdn".to_string()
    } else if host == "fbcdn.net" || host.ends_with(".fbcdn.net") {
        "meta_fbcdn".to_string()
    } else {
        "untrusted".to_string()
    }
}

fn instagram_avatar_cache_key(owner_id: Option<&str>, handle: &str) -> Option<String> {
    if let Some(owner_id) = owner_id
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 128)
    {
        return Some(format!("owner-id:{owner_id}"));
    }
    let handle = instagram::parser::normalize_instagram_handle(handle);
    (!handle.is_empty() && handle.len() <= 30).then(|| format!("owner-handle:{handle}"))
}

#[cfg(test)]
fn validate_instagram_avatar_url_with_resolver<F>(
    url: &reqwest::Url,
    resolve: F,
) -> Result<(), String>
where
    F: FnOnce(&str, u16) -> Result<Vec<IpAddr>, String>,
{
    if !instagram_avatar_url_allowed(url.as_str()) {
        return Err("Instagram avatar linki izinli CDN sinirlarinin disinda.".to_string());
    }
    validate_preview_url_with_resolver(url, resolve)
}

fn validate_instagram_avatar_url(url: &reqwest::Url) -> Result<(), String> {
    if !instagram_avatar_url_allowed(url.as_str()) {
        return Err("Instagram avatar linki izinli CDN sinirlarinin disinda.".to_string());
    }
    validate_preview_url(url)
}

fn instagram_cdn_image_url_allowed(url: &str) -> bool {
    if !instagram_avatar_url_allowed(url) {
        return false;
    }

    extension_from_url(url)
        .map(|extension| supported_image_extension(&extension))
        .unwrap_or(false)
}

fn instagram_avatar_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_INSTAGRAM_AVATAR_REDIRECTS {
            return attempt.error("Instagram avatar redirect limiti asildi.");
        }
        if let Err(error) = validate_instagram_avatar_url(attempt.url()) {
            return attempt.error(error);
        }
        attempt.follow()
    })
}

#[derive(Clone)]
struct InstagramAvatarFetchResult {
    data_url: String,
    host_class: String,
    http_status: Option<u16>,
}

#[derive(Clone)]
struct InstagramAvatarFetchFailure {
    host_class: String,
    http_status: Option<u16>,
}

fn fetch_instagram_avatar_data_url(
    url: &str,
    owner_id: Option<&str>,
    handle: &str,
) -> Result<InstagramAvatarFetchResult, InstagramAvatarFetchFailure> {
    let parsed = reqwest::Url::parse(url).map_err(|_| InstagramAvatarFetchFailure {
        host_class: "invalid_url".to_string(),
        http_status: None,
    })?;
    let host_class = instagram_avatar_host_class(&parsed);
    validate_instagram_avatar_url(&parsed).map_err(|_| InstagramAvatarFetchFailure {
        host_class: host_class.clone(),
        http_status: None,
    })?;
    let cache_key = instagram_avatar_cache_key(owner_id, handle).ok_or_else(|| {
        InstagramAvatarFetchFailure {
            host_class: host_class.clone(),
            http_status: None,
        }
    })?;

    let now = now_ms();
    if let Ok(mut cache) = instagram_avatar_cache().lock() {
        cache.retain(|_, entry| {
            now.saturating_sub(entry.cached_at_ms) <= INSTAGRAM_AVATAR_CACHE_TTL_MS
        });
        if let Some(entry) = cache.get(&cache_key) {
            return Ok(InstagramAvatarFetchResult {
                data_url: entry.data_url.clone(),
                host_class: entry.host_class.clone(),
                http_status: entry.http_status,
            });
        }
    }

    let client = instagram_avatar_client().map_err(|_| InstagramAvatarFetchFailure {
            host_class: host_class.clone(),
            http_status: None,
        })?;
    let response = client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124.0 Safari/537.36")
        .header(reqwest::header::ACCEPT, "image/avif,image/webp,image/png,image/jpeg,image/*,*/*;q=0.8")
        .header(reqwest::header::REFERER, "https://www.instagram.com/")
        .send()
        .map_err(|_| InstagramAvatarFetchFailure {
            host_class: host_class.clone(),
            http_status: None,
        })?;
    let http_status = Some(response.status().as_u16());
    let final_host_class = instagram_avatar_host_class(response.url());
    if !response.status().is_success() {
        return Err(InstagramAvatarFetchFailure {
            host_class: final_host_class,
            http_status,
        });
    }
    validate_instagram_avatar_url(response.url()).map_err(|_| InstagramAvatarFetchFailure {
        host_class: final_host_class.clone(),
        http_status,
    })?;
    let content_mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(avatar_mime_from_text);
    let final_url = response.url().clone();
    let bytes =
        read_response_body_with_limit(response, MAX_INSTAGRAM_AVATAR_BYTES).map_err(|_| {
            InstagramAvatarFetchFailure {
                host_class: final_host_class.clone(),
                http_status,
            }
        })?;
    let mime = content_mime
        .or_else(|| avatar_mime_from_url(&final_url))
        .or_else(|| {
            ["image/jpeg", "image/png", "image/webp"]
                .into_iter()
                .find(|mime| avatar_bytes_match_mime(&bytes, mime))
        })
        .ok_or_else(|| InstagramAvatarFetchFailure {
            host_class: final_host_class.clone(),
            http_status,
        })?;
    if !avatar_bytes_match_mime(&bytes, mime) {
        return Err(InstagramAvatarFetchFailure {
            host_class: final_host_class,
            http_status,
        });
    }

    let data_url = image_bytes_to_data_url(&bytes, mime);
    if let Ok(mut cache) = instagram_avatar_cache().lock() {
        if cache.len() >= INSTAGRAM_AVATAR_CACHE_CAPACITY {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.cached_at_ms)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            cache_key,
            CachedInstagramAvatar {
                data_url: data_url.clone(),
                cached_at_ms: now,
                host_class: final_host_class.clone(),
                http_status,
            },
        );
    }
    Ok(InstagramAvatarFetchResult {
        data_url,
        host_class: final_host_class,
        http_status,
    })
}

fn instagram_likely_post_media_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };

    if !instagram_cdn_image_url_allowed(url) {
        return false;
    }

    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let path = parsed.path().to_lowercase();
    let query = parsed.query().unwrap_or("").to_lowercase();
    let combined = format!("{}?{}", path, query);

    if combined.contains("profile_pic")
        || combined.contains("profilepic")
        || combined.contains("avatar")
        || combined.contains("s150x150")
        || combined.contains("t51.2885-19")
    {
        return false;
    }

    host.starts_with("scontent")
        || path.contains("/v/t51.29350-15/")
        || path.contains("/v/t39.30808-6/")
        || path.contains("/v/t51.2885-15/")
        || combined.contains("dst-jpg")
        || combined.contains("dst-webp")
}

fn instagram_public_title_metadata(
    raw_title: &str,
    fallback_title: &str,
) -> (String, Option<String>, Option<String>) {
    let clean = raw_title.trim();
    let title = if clean.is_empty() {
        fallback_title
    } else {
        clean
    };
    let lower = title.to_lowercase();

    if let Some(index) = lower.find(" on instagram:") {
        let author = title[..index]
            .trim()
            .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '“' || ch == '”')
            .trim()
            .to_string();
        let text = title[index + " on instagram:".len()..]
            .trim()
            .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '“' || ch == '”')
            .trim()
            .to_string();

        let final_title = if text.is_empty() {
            title.to_string()
        } else {
            text.clone()
        };

        return (
            final_title,
            non_empty_string(author),
            non_empty_string(text),
        );
    }

    (title.to_string(), None, non_empty_string(title.to_string()))
}

fn instagram_embedded_url_context_allowed(html: &str, start: usize) -> bool {
    let mut context_start = start.saturating_sub(220);
    while context_start < start && !html.is_char_boundary(context_start) {
        context_start += 1;
    }
    let context = html[context_start..start].to_lowercase();

    if context.contains("profile_pic") || context.contains("avatar") {
        return false;
    }

    [
        "display_url",
        "thumbnail_src",
        "thumbnail_url",
        "image_versions",
        "carousel_media",
        "edge_sidecar",
        "og:image",
        "poster",
        "embeddedmediaimage",
        "src=",
        "srcset",
        "data-src",
        "\"url\"",
        "\\\"url\\\"",
    ]
    .iter()
    .any(|needle| context.contains(needle))
}

fn collect_instagram_embedded_image_urls(html: &str, seen: &mut HashSet<String>) -> Vec<String> {
    let mut urls = Vec::new();

    for needle in ["https://", "https:\\/\\/"] {
        let mut offset = 0;
        while let Some(relative_start) = html[offset..].find(needle) {
            let start = offset + relative_start;
            let mut end = start;

            for (relative_index, ch) in html[start..].char_indices() {
                if ch == '"' || ch == '\'' || ch == '<' || ch == '>' || ch.is_whitespace() {
                    break;
                }
                end = start + relative_index + ch.len_utf8();
            }

            if end > start {
                let url = decode_embedded_url(&html[start..end]);
                if instagram_embedded_url_context_allowed(html, start)
                    && instagram_likely_post_media_url(&url)
                    && seen.insert(url.clone())
                {
                    urls.push(url);
                }
            }

            offset = end.max(start + needle.len());
        }
    }

    urls
}

fn collect_instagram_likely_post_image_urls(html: &str, seen: &mut HashSet<String>) -> Vec<String> {
    let mut urls = Vec::new();

    for url in collect_instagram_raw_cdn_url_candidates(html) {
        if instagram_likely_post_media_url(&url) && seen.insert(url.clone()) {
            urls.push(url);
        }
    }

    urls
}

fn instagram_public_items_from_html(html: &str, fallback_title: &str) -> Vec<MediaItem> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    for key in ["og:image", "twitter:image"] {
        if let Some(url) = find_meta_content(html, key).map(|value| decode_embedded_url(&value)) {
            if instagram_likely_post_media_url(&url) && seen.insert(url.clone()) {
                urls.push(url);
            }
        }
    }

    urls.extend(collect_instagram_embedded_image_urls(html, &mut seen));
    if urls.is_empty() {
        urls.extend(collect_instagram_likely_post_image_urls(html, &mut seen));
    }

    let raw_title = find_meta_content(html, "og:title")
        .or_else(|| find_meta_content(html, "twitter:title"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_title.to_string());
    let (title, author_name, text) = instagram_public_title_metadata(&raw_title, fallback_title);
    // Public HTML exposes unrelated profile images without a reliable owner
    // identity. Never guess an avatar from that unscoped document.
    let avatar_url: Option<String> = None;
    let width = find_meta_content(html, "og:image:width").and_then(|value| value.parse().ok());
    let height = find_meta_content(html, "og:image:height").and_then(|value| value.parse().ok());
    let total = urls.len();

    urls.into_iter()
        .enumerate()
        .map(|(index, url)| MediaItem {
            id: format!("instagram-public-{}", index),
            item_type: "photo".to_string(),
            source_index: index,
            extension: sanitize_media_extension(
                extension_from_url(&url).as_deref().unwrap_or("jpg"),
            ),
            preview_url: url,
            audio_url: None,
            width: if index == 0 { width } else { None },
            height: if index == 0 { height } else { None },
            is_story: false,
            taken_at_ms: None,
            duration_ms: None,
            has_audio: false,
            preview_ref: Some(format!("item:instagram-public-{}", index)),
            poster_ref: None,
            title: if total > 1 {
                format!("{} - Fotograf {}", title, index + 1)
            } else {
                title.clone()
            },
            author_id: None,
            author_name: author_name.clone(),
            author_handle: None,
            avatar_url: avatar_url.clone(),
            avatar_data_url: None,
            canonical_instagram_identity: None,
            text: text.clone(),
            display_date: None,
            reply_count: None,
            retweet_count: None,
            like_count: None,
            view_count: None,
        })
        .collect()
}

struct InstagramPublicFetchResult {
    text: String,
    final_url: String,
    content_type: String,
    bytes: usize,
}

fn collect_instagram_raw_cdn_url_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for needle in ["https://", "https:\\/\\/"] {
        let mut offset = 0;
        while let Some(relative_start) = text[offset..].find(needle) {
            let start = offset + relative_start;
            let mut end = start;

            for (relative_index, ch) in text[start..].char_indices() {
                if ch == '"' || ch == '\'' || ch == '<' || ch == '>' || ch.is_whitespace() {
                    break;
                }
                end = start + relative_index + ch.len_utf8();
            }

            if end > start {
                let url = decode_embedded_url(&text[start..end]);
                if let Ok(parsed) = reqwest::Url::parse(&url) {
                    let host = parsed.host_str().unwrap_or("").to_lowercase();
                    if (host.contains("cdninstagram.com") || host.contains("fbcdn.net"))
                        && seen.insert(url.clone())
                    {
                        candidates.push(url);
                    }
                }
            }

            offset = end.max(start + needle.len());
        }
    }

    candidates
}

fn report_bool(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn instagram_public_fetch_diagnostic(
    label: &str,
    requested_url: &str,
    fetched: &InstagramPublicFetchResult,
) -> String {
    let lower = fetched.text.to_lowercase();
    let candidates = collect_instagram_raw_cdn_url_candidates(&fetched.text);
    let allowed_candidates = candidates
        .iter()
        .filter(|url| instagram_cdn_image_url_allowed(url))
        .count();
    let contextual_candidates = {
        let mut seen = HashSet::new();
        collect_instagram_embedded_image_urls(&fetched.text, &mut seen).len()
    };
    let likely_post_candidates = candidates
        .iter()
        .filter(|url| instagram_likely_post_media_url(url))
        .count();
    let samples = candidates
        .iter()
        .take(5)
        .map(|url| sanitize_report_text(url))
        .collect::<Vec<_>>()
        .join(" | ");

    format!(
        "{} diagnostic:\n  requested_url={}\n  final_url={}\n  content_type={}\n  bytes={}\n  contains_accounts_login={}\n  contains_login_text={}\n  contains_og_image={}\n  contains_twitter_image={}\n  contains_thumbnail_url={}\n  contains_scontent={}\n  meta_tag_count={}\n  script_tag_count={}\n  raw_cdn_candidate_count={}\n  allowed_cdn_candidate_count={}\n  contextual_candidate_count={}\n  likely_post_candidate_count={}\n  candidate_samples={}",
        label,
        sanitize_report_text(requested_url),
        sanitize_report_text(&fetched.final_url),
        fetched.content_type,
        fetched.bytes,
        report_bool(lower.contains("/accounts/login") || lower.contains("login_required")),
        report_bool(lower.contains("login") || lower.contains("log in")),
        report_bool(lower.contains("og:image")),
        report_bool(lower.contains("twitter:image")),
        report_bool(lower.contains("thumbnail_url") || lower.contains("thumbnailurl")),
        report_bool(lower.contains("scontent") || lower.contains("cdninstagram")),
        lower.matches("<meta").count(),
        lower.matches("<script").count(),
        candidates.len(),
        allowed_candidates,
        contextual_candidates,
        likely_post_candidates,
        if samples.is_empty() { "none".to_string() } else { samples }
    )
}

fn fetch_instagram_public_text(
    url: &str,
    accept: &'static str,
    label: &'static str,
) -> Result<InstagramPublicFetchResult, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{} linki gecersiz.", label))?;
    let client = media_client(Duration::from_secs(35))?;
    let response = client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .header(reqwest::header::ACCEPT, accept)
        .header(reqwest::header::ACCEPT_LANGUAGE, "tr-TR,tr;q=0.9,en-US;q=0.8,en;q=0.7")
        .send()
        .map_err(|err| format!("{} okunamadi: {}", label, err))?;
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("Bilinmiyor")
        .to_string();

    if !response.status().is_success() {
        return Err(format!(
            "{} HTTP {} dondu. final_url={} content_type={}",
            label,
            response.status(),
            sanitize_report_text(&final_url),
            content_type
        ));
    }

    let bytes = read_response_body_with_limit(response, MAX_INSTAGRAM_PUBLIC_HTML_BYTES)?;
    let byte_len = bytes.len();

    Ok(InstagramPublicFetchResult {
        text: String::from_utf8_lossy(&bytes).to_string(),
        final_url,
        content_type,
        bytes: byte_len,
    })
}

fn instagram_public_html_collect_media_at(
    clean_url: &str,
    page_url: &str,
    label: &'static str,
) -> Result<MediaAnalysis, String> {
    let html = fetch_instagram_public_text(
        page_url,
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        label,
    )?;
    let fallback_title = gallery_media_title("instagram", clean_url);
    let items = instagram_public_items_from_html(&html.text, &fallback_title);

    if items.is_empty() {
        return Err(format!(
            "{} icinde fotograf onizlemesi bulunamadi.\n{}",
            label,
            instagram_public_fetch_diagnostic(label, page_url, &html)
        ));
    }

    let title = items
        .first()
        .map(|item| item.title.clone())
        .unwrap_or(fallback_title);
    let uploader = items
        .iter()
        .find_map(|item| item.author_name.clone())
        .unwrap_or_else(|| platform_label_for_backend("instagram").to_string());

    Ok(MediaAnalysis {
        analysis_id: String::new(),
        expires_at_ms: 0,
        platform: "instagram".to_string(),
        content_kind: media_content_kind(&items),
        title,
        uploader,
        author: AuthorIdentity::default(),
        items,
        initial_index: 0,
        requested_item_id: None,
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: None,
        video_info: None,
    })
}

fn instagram_public_html_collect_media(clean_url: &str) -> Result<MediaAnalysis, String> {
    let Some(public_url) = instagram_public_post_url(clean_url) else {
        return Err("Instagram public post linki taninamadi.".to_string());
    };

    instagram_public_html_collect_media_at(clean_url, &public_url, "Instagram public sayfasi")
}

fn instagram_public_json_collect_media(clean_url: &str) -> Result<MediaAnalysis, String> {
    let Some(public_url) = instagram_public_json_url(clean_url) else {
        return Err("Instagram public JSON linki taninamadi.".to_string());
    };

    let text = fetch_instagram_public_text(
        &public_url,
        "application/json,text/plain,*/*",
        "Instagram public JSON",
    )?;
    let clean = text.text.trim_start();

    if !(clean.starts_with('{') || clean.starts_with('[')) {
        return Err(format!(
            "Instagram public JSON yerine HTML/login sayfasi dondu.\n{}",
            instagram_public_fetch_diagnostic("Instagram public JSON", &public_url, &text)
        ));
    }

    let value = serde_json::from_str::<serde_json::Value>(clean).map_err(|err| {
        format!(
            "Instagram public JSON okunamadi: {}\n{}",
            err,
            instagram_public_fetch_diagnostic("Instagram public JSON", &public_url, &text)
        )
    })?;
    let fallback_title = gallery_media_title("instagram", clean_url);
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    collect_gallery_items_from_value(&value, "instagram", &fallback_title, &mut items, &mut seen);

    if items.is_empty() {
        return Err(format!(
            "Instagram public JSON icinde fotograf bulunamadi.\n{}",
            instagram_public_fetch_diagnostic("Instagram public JSON", &public_url, &text)
        ));
    }

    let title = items
        .first()
        .map(|item| item.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(fallback_title);
    let uploader = items
        .iter()
        .find_map(|item| item.author_name.clone())
        .unwrap_or_else(|| platform_label_for_backend("instagram").to_string());

    Ok(MediaAnalysis {
        analysis_id: String::new(),
        expires_at_ms: 0,
        platform: "instagram".to_string(),
        content_kind: media_content_kind(&items),
        title,
        uploader,
        author: AuthorIdentity::default(),
        items,
        initial_index: 0,
        requested_item_id: None,
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: None,
        video_info: None,
    })
}

fn instagram_public_oembed_collect_media(clean_url: &str) -> Result<MediaAnalysis, String> {
    let Some(public_url) = instagram_public_oembed_url(clean_url) else {
        return Err("Instagram oEmbed linki taninamadi.".to_string());
    };

    let text = fetch_instagram_public_text(
        &public_url,
        "application/json,text/plain,*/*",
        "Instagram oEmbed",
    )?;
    let value = serde_json::from_str::<serde_json::Value>(text.text.trim()).map_err(|err| {
        format!(
            "Instagram oEmbed JSON okunamadi: {}\n{}",
            err,
            instagram_public_fetch_diagnostic("Instagram oEmbed", &public_url, &text)
        )
    })?;
    let url = json_text(&value, &["thumbnail_url", "thumbnailUrl"]);

    if !instagram_likely_post_media_url(&url) {
        return Err(format!(
            "Instagram oEmbed icinde kullanilabilir fotograf bulunamadi. thumbnail_url={}\n{}",
            sanitize_report_text(&url),
            instagram_public_fetch_diagnostic("Instagram oEmbed", &public_url, &text)
        ));
    }

    let author_name = non_empty_string(json_text(&value, &["author_name", "authorName"]));
    let title = json_text(&value, &["title", "author_name"])
        .trim()
        .to_string();
    let title = if title.is_empty() {
        gallery_media_title("instagram", clean_url)
    } else {
        title
    };
    let text = non_empty_string(title.clone());
    let item = MediaItem {
        id: "instagram-oembed-0".to_string(),
        item_type: "photo".to_string(),
        source_index: 0,
        extension: sanitize_media_extension(extension_from_url(&url).as_deref().unwrap_or("jpg")),
        preview_url: url,
        audio_url: None,
        width: json_u32(&value, &["thumbnail_width", "thumbnailWidth"]),
        height: json_u32(&value, &["thumbnail_height", "thumbnailHeight"]),
        is_story: false,
        taken_at_ms: None,
        duration_ms: None,
        has_audio: false,
        preview_ref: Some("item:instagram-oembed-0".to_string()),
        poster_ref: None,
        title: title.clone(),
        author_id: None,
        author_name: author_name.clone(),
        author_handle: None,
        avatar_url: None,
        avatar_data_url: None,
        canonical_instagram_identity: None,
        text,
        display_date: None,
        reply_count: None,
        retweet_count: None,
        like_count: None,
        view_count: None,
    };

    Ok(MediaAnalysis {
        analysis_id: String::new(),
        expires_at_ms: 0,
        platform: "instagram".to_string(),
        content_kind: "photo".to_string(),
        title,
        uploader: author_name
            .unwrap_or_else(|| platform_label_for_backend("instagram").to_string()),
        author: AuthorIdentity::default(),
        items: vec![item],
        initial_index: 0,
        requested_item_id: None,
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: None,
        video_info: None,
    })
}

fn try_instagram_public_html_fallback(
    clean_url: &str,
    platform: &str,
    errors: &mut Vec<String>,
) -> Option<MediaAnalysis> {
    if platform != "instagram" || instagram_public_post_path(clean_url).is_none() {
        return None;
    }

    match instagram_public_json_collect_media(clean_url) {
        Ok(analysis) => return Some(analysis),
        Err(err) => {
            errors.push(format!(
                "instagram public json: {}",
                sanitize_report_text(&err)
            ));
        }
    }

    match instagram_public_html_collect_media(clean_url) {
        Ok(analysis) => return Some(analysis),
        Err(err) => {
            errors.push(format!(
                "instagram public html: {}",
                sanitize_report_text(&err)
            ));
        }
    }

    for (label, embed_url) in instagram_public_embed_urls(clean_url) {
        match instagram_public_html_collect_media_at(clean_url, &embed_url, label) {
            Ok(analysis) => return Some(analysis),
            Err(err) => {
                errors.push(format!("{}: {}", label, sanitize_report_text(&err)));
            }
        }
    }

    match instagram_public_oembed_collect_media(clean_url) {
        Ok(analysis) => Some(analysis),
        Err(err) => {
            errors.push(format!("instagram oembed: {}", sanitize_report_text(&err)));
            None
        }
    }
}

#[derive(Clone, Debug)]
enum InstagramHelperAuth {
    Public,
    Prepared { token: String },
    Saved,
    Browser { browser_id: String, save: bool },
}

fn instagram_helper_auth_mode(auth_mode: Option<&str>) -> InstagramHelperAuth {
    let clean = auth_mode.unwrap_or("public").trim();

    if clean == "saved:instagram" {
        return InstagramHelperAuth::Saved;
    }

    if let Some(token) = clean.strip_prefix("prepared:instagram:") {
        let token = token.trim();
        if !token.is_empty() {
            return InstagramHelperAuth::Prepared {
                token: token.to_string(),
            };
        }
    }

    if let Some(rest) = clean.strip_prefix("browser:") {
        let (browser_id, save) = rest
            .strip_suffix(":save")
            .map(|value| (value, true))
            .unwrap_or((rest, false));
        if !browser_auth_definitions(browser_id).is_empty() {
            return InstagramHelperAuth::Browser {
                browser_id: browser_id.to_string(),
                save,
            };
        }
    }

    InstagramHelperAuth::Public
}

fn browser_label_for_id(browser_id: &str) -> &'static str {
    browser_auth_definitions(browser_id)
        .first()
        .map(|definition| definition.browser_label)
        .unwrap_or("Tarayici")
}

fn parse_instaloader_helper_output(
    stdout: &str,
) -> Result<(MediaAnalysis, serde_json::Value), String> {
    let value = serde_json::from_str::<serde_json::Value>(stdout.trim())
        .map_err(|err| format!("Instaloader helper JSON sonucu okunamadi: {}", err))?;

    if value.get("ok").and_then(|item| item.as_bool()) == Some(false) {
        let message = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("Instaloader helper medya okuyamadi.");
        return Err(message.to_string());
    }

    let analysis = serde_json::from_value::<MediaAnalysis>(value.clone())
        .map_err(|err| format!("Instaloader helper medya modeli okunamadi: {}", err))?;

    Ok((analysis, value))
}

fn instaloader_collect_media(
    app: &tauri::AppHandle,
    clean_url: &str,
    auth_mode: Option<&str>,
) -> Result<MediaAnalysis, String> {
    if !is_instagram_url(clean_url) {
        return Err("Instaloader helper yalnizca Instagram icin kullanilir.".to_string());
    }

    let helper = ensure_runtime_tool(app, "instaloader-helper")?;
    let auth = instagram_helper_auth_mode(auth_mode);
    let mut cleanup_files = Vec::new();
    let mut session_out: Option<PathBuf> = None;
    let mut save_browser_id = String::new();
    let mut save_browser_label = String::new();
    let mut save_cookie_jar: Option<BrowserCookieJar> = None;

    let mut command = hidden_command(helper);
    command.arg("analyze").arg("--url").arg(clean_url);

    match auth {
        InstagramHelperAuth::Public => {}
        InstagramHelperAuth::Prepared { token } => {
            let cookie_path = materialize_prepared_instagram_cookie_file(&token)?;
            command.arg("--cookie-jar").arg(cookie_path.path());
            cleanup_files.push(cookie_path);
        }
        InstagramHelperAuth::Saved => match materialize_saved_instagram_session_file() {
            Ok((session_path, username)) => {
                command.arg("--session").arg(session_path.path());
                if !username.trim().is_empty() {
                    command.arg("--session-username").arg(username);
                }
                cleanup_files.push(session_path);
            }
            Err(session_err) => match materialize_saved_instagram_cookie_file() {
                Ok(cookie_path) => {
                    command.arg("--cookie-jar").arg(cookie_path.path());
                    cleanup_files.push(cookie_path);
                }
                Err(cookie_err) => {
                    return Err(format!(
                        "Kayitli Instagram oturumu bulunamadi. Session: {}; Cookie: {}",
                        session_err, cookie_err
                    ));
                }
            },
        },
        InstagramHelperAuth::Browser { browser_id, save } => {
            let browser_label = browser_label_for_id(&browser_id);
            let jar = best_instagram_cookie_jar(&browser_id, browser_label)?;
            let cookie_path = materialize_instagram_cookie_jar(&jar)?;
            command.arg("--cookie-jar").arg(cookie_path.path());
            cleanup_files.push(cookie_path);

            if save {
                save_cookie_jar = Some(jar);
                let out = std::env::temp_dir().join(format!(
                    "mediadrop-instaloader-session-{}-{}.bin",
                    browser_id,
                    unique_stamp()
                ));
                command.arg("--session-out").arg(&out);
                session_out = Some(out);
                save_browser_id = browser_id;
                save_browser_label = browser_label.to_string();
            }
        }
    }

    let output = match capture_command_with_timeout(command, Duration::from_secs(95)) {
        Ok(output) => output,
        Err(err) => {
            drop(cleanup_files);
            if let Some(path) = session_out {
                let _ = fs::remove_file(path);
            }
            return Err(format!("Instaloader helper tamamlanamadi: {}", err));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let parse_result = if !stdout.is_empty() {
        parse_instaloader_helper_output(&stdout)
    } else {
        Err(if stderr_text.is_empty() {
            "Instaloader helper bos sonuc dondurdu.".to_string()
        } else {
            stderr_text
        })
    };

    let result = parse_result.and_then(|(analysis, raw)| {
        if let Some(path) = session_out.as_ref() {
            let username = raw
                .get("sessionUsername")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let helper_version = raw
                .get("instaloaderVersion")
                .and_then(|value| value.as_str());
            save_instagram_session_file(
                path,
                &save_browser_id,
                &save_browser_label,
                username,
                helper_version,
            )?;
        }
        if let Some(jar) = save_cookie_jar.as_ref() {
            save_instagram_cookie_jar(jar)?;
        }
        Ok(analysis)
    });

    drop(cleanup_files);
    if let Some(path) = session_out {
        let _ = fs::remove_file(path);
    }

    result
}

fn collect_instagram_story_attempt(
    gallery_dl: &Path,
    clean_url: &str,
    request: &InstagramStoryRequest,
    auth: GalleryAuthAttempt,
    fallback_title: &str,
) -> Result<(Vec<MediaItem>, Option<String>), String> {
    match request {
        InstagramStoryRequest::Direct {
            username,
            requested_media_id,
        } => {
            let profile_url = instagram_story_profile_url(username)?;
            let stdout = gallerydl_dump_with_fallback(gallery_dl, &profile_url, auth, true)?;
            let items = gallery_stdout_to_items(&stdout, "instagram", fallback_title)?;
            Ok((
                canonical_owner_story_items(items, username)?,
                requested_media_id.clone(),
            ))
        }
        InstagramStoryRequest::Share {
            token,
            query_media_id,
        } => {
            if token.trim().is_empty() {
                return Err("Instagram Story schema: share token bos.".to_string());
            }
            let share_stdout =
                gallerydl_dump_with_fallback(gallery_dl, clean_url, auth.clone(), true)?;
            let share_items = gallery_stdout_to_items(&share_stdout, "instagram", fallback_title)?;
            let target =
                resolve_instagram_share_story_target(&share_items, query_media_id.as_deref())?;
            let profile_url = instagram_story_profile_url(&target.username)?;
            let profile_stdout =
                gallerydl_dump_with_fallback(gallery_dl, &profile_url, auth, true)?;
            let profile_items =
                gallery_stdout_to_items(&profile_stdout, "instagram", fallback_title)?;
            Ok((
                canonical_owner_story_items(profile_items, &target.username)?,
                Some(target.media_id),
            ))
        }
    }
}

fn finalize_media_analysis(mut analysis: MediaAnalysis, clean_url: &str) -> MediaAnalysis {
    analysis.analysis_id = Uuid::new_v4().to_string();
    analysis.expires_at_ms = now_ms().saturating_add(MEDIA_ANALYSIS_TTL_MS);
    apply_story_policy(&mut analysis, clean_url);

    let author_item = analysis.items.iter().find(|item| {
        item.author_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || item
                .author_handle
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    });
    let canonical_instagram_identity = (analysis.platform == "instagram")
        .then(|| {
            analysis
                .items
                .iter()
                .find_map(|item| item.canonical_instagram_identity.clone())
        })
        .flatten();
    let twitter_outer = analysis
        .twitter_quote
        .as_ref()
        .map(|quote| &quote.outer);
    let author_id = canonical_instagram_identity
        .as_ref()
        .and_then(|identity| identity.id.clone())
        .or_else(|| {
            (analysis.platform != "instagram")
                .then(|| author_item.and_then(|item| item.author_id.clone()))
                .flatten()
        });
    let author_name = canonical_instagram_identity
        .as_ref()
        .and_then(|identity| identity.name.clone())
        .or_else(|| twitter_outer.map(|post| post.author_name.clone()))
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            (analysis.platform != "instagram")
                .then(|| author_item.and_then(|item| item.author_name.clone()))
                .flatten()
        })
        .unwrap_or_else(|| {
            if analysis.platform == "instagram" {
                String::new()
            } else {
                analysis.uploader.clone()
            }
        });
    let author_handle = canonical_instagram_identity
        .as_ref()
        .and_then(|identity| identity.handle.clone())
        .or_else(|| twitter_outer.map(|post| post.author_handle.clone()))
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            (analysis.platform != "instagram")
                .then(|| author_item.and_then(|item| item.author_handle.clone()))
                .flatten()
        })
        .unwrap_or_default();
    let avatar_url = canonical_instagram_identity
        .as_ref()
        .and_then(|identity| identity.avatar_url.clone())
        .or_else(|| twitter_outer.and_then(|post| post.avatar_url.clone()))
        .or_else(|| {
            (analysis.platform != "instagram")
                .then(|| author_item.and_then(|item| item.avatar_url.clone()))
                .flatten()
        });
    let identity_matched = canonical_instagram_identity.is_some();
    let mut avatar_data_url = None;
    let mut instagram_diagnostics = None;
    if analysis.platform == "instagram" {
        let author_source = if author_id.is_some() {
            "root_owner_id"
        } else if !author_handle.trim().is_empty() {
            "root_owner_username"
        } else {
            "none"
        };
        let mut host_class = avatar_url
            .as_deref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .map(|url| instagram_avatar_host_class(&url))
            .unwrap_or_else(|| "none".to_string());
        let mut http_status = None;
        if let Some(url) = avatar_url.as_deref() {
            match fetch_instagram_avatar_data_url(url, author_id.as_deref(), &author_handle) {
                Ok(result) => {
                    host_class = result.host_class;
                    http_status = result.http_status;
                    avatar_data_url = Some(result.data_url);
                }
                Err(failure) => {
                    host_class = failure.host_class;
                    http_status = failure.http_status;
                }
            }
        }
        instagram_diagnostics = Some(InstagramAuthorDiagnostics {
            author_source: author_source.to_string(),
            identity_matched,
            avatar_present: avatar_data_url.is_some(),
            host_class,
            http_status,
        });
    }
    analysis.author = AuthorIdentity {
        id: author_id.clone(),
        name: author_name.clone(),
        handle: author_handle.clone(),
        avatar_data_url: avatar_data_url.clone(),
    };
    for item in &mut analysis.items {
        if analysis.platform == "instagram" {
            // Instagram kimliği yalnız analiz seviyesinde kanonik tutulur. Item
            // üzerinde avatar/author mirror etmek carousel medyasıyla profil
            // görselinin karışmasına ve aynı avatarın birden çok kez çizilmesine
            // neden oluyordu.
            item.author_id = None;
            item.author_name = None;
            item.author_handle = None;
            item.avatar_url = None;
            item.avatar_data_url = None;
        } else {
            item.author_id = item.author_id.clone().or_else(|| author_id.clone());
            item.author_name = item
                .author_name
                .clone()
                .or_else(|| Some(author_name.clone()));
            item.author_handle = item
                .author_handle
                .clone()
                .or_else(|| non_empty_string(author_handle.clone()));
            item.avatar_data_url = avatar_data_url.clone();
        }
        item.preview_ref = Some(format!("{}:{}", analysis.analysis_id, item.id));
    }
    analysis.instagram_diagnostics = instagram_diagnostics;
    analysis
}

fn collect_media(
    app: &tauri::AppHandle,
    clean_url: &str,
    auth_mode: Option<&str>,
) -> Result<MediaAnalysis, String> {
    if let Some(error) = instagram_highlight_unsupported_error(clean_url) {
        return Err(error);
    }
    let platform = media_platform_from_url(clean_url);
    if platform != "instagram" {
        return gallerydl_collect_media(app, clean_url, auth_mode)
            .map(|analysis| finalize_media_analysis(analysis, clean_url));
    }

    let mut errors = Vec::new();
    // gallery-dl is the primary Instagram inventory engine. It consumes the
    // same saved Netscape jar that downloads use and currently exposes both
    // post media and the authoritative `owner` object. Instaloader remains a
    // fallback for extractor-specific regressions.
    let gallery_error = match gallerydl_collect_media(app, clean_url, auth_mode) {
        Ok(mut analysis) => {
            propagate_media_item_metadata(&mut analysis.items);
            return Ok(finalize_media_analysis(analysis, clean_url));
        }
        Err(err) => err,
    };
    errors.push(format!(
        "gallery-dl: {}",
        sanitize_report_text(&gallery_error)
    ));

    // Story inventory is authoritative only when gallery-dl uses the saved
    // authenticated session. Never downgrade a Story failure to Instaloader
    // or a public post scraper: doing so can return one stale/wrong item and
    // can also turn not-found/schema failures into repeated auth prompts.
    if instagram_story_request(clean_url).is_some() {
        return Err(gallery_error);
    }

    if instagram_helper_fallback_allowed(clean_url, &gallery_error) {
        match instaloader_collect_media(app, clean_url, auth_mode) {
            Ok(mut analysis) => {
                propagate_media_item_metadata(&mut analysis.items);
                return Ok(finalize_media_analysis(analysis, clean_url));
            }
            Err(err) => errors.push(format!(
                "instaloader-helper: {}",
                sanitize_report_text(&err)
            )),
        }
    }

    if instagram_auth_mode_uses_credentials(auth_mode) {
        if let Some(mut analysis) =
            try_instagram_public_html_fallback(clean_url, &platform, &mut errors)
        {
            add_analysis_warning(
                &mut analysis,
                INSTAGRAM_AUTHENTICATED_PUBLIC_FALLBACK_WARNING,
            );
            return Ok(finalize_media_analysis(analysis, clean_url));
        }

        if gallery_error.starts_with(STRUCTURED_ERROR_PREFIX) {
            return Err(gallery_error);
        }

        return Err(format!(
            "Instagram fotograflari okunamadi.\n{}",
            errors.join("\n")
        ));
    }

    Err(format!(
        "Instagram fotograflari okunamadi.\n{}",
        errors.join("\n")
    ))
}

const INSTAGRAM_AUTHENTICATED_PUBLIC_FALLBACK_WARNING: &str =
    "instagramAuthenticatedPublicFallback";

fn add_analysis_warning(analysis: &mut MediaAnalysis, warning: &str) {
    if !analysis.warnings.iter().any(|item| item == warning) {
        analysis.warnings.push(warning.to_string());
    }
}

fn instagram_auth_mode_uses_credentials(auth_mode: Option<&str>) -> bool {
    let clean = auth_mode.unwrap_or_default().trim();
    clean == "saved:instagram"
        || clean
            .strip_prefix("prepared:instagram:")
            .is_some_and(|token| !token.trim().is_empty())
        || clean.strip_prefix("browser:").is_some_and(|selection| {
            let browser_id = selection.strip_suffix(":save").unwrap_or(selection).trim();
            !browser_auth_definitions(browser_id).is_empty()
        })
}

fn structured_backend_error_code(error: &str) -> Option<String> {
    let payload = error.strip_prefix(STRUCTURED_ERROR_PREFIX)?.trim();
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()?
        .get("code")?
        .as_str()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
}

fn instagram_helper_fallback_allowed(clean_url: &str, error: &str) -> bool {
    if instagram_story_request(clean_url).is_some() {
        return false;
    }

    if let Some(code) = structured_backend_error_code(error) {
        return matches!(
            code.as_str(),
            "instagram_schema_error" | "instagram_extractor_mismatch" | "instagram_media_empty"
        );
    }

    if gallery_error_indicates_auth_failure(error) {
        return false;
    }

    let lower = error.to_ascii_lowercase();
    if [
        "429",
        "rate limit",
        "too many requests",
        "not found",
        "404",
        "expired",
        "deleted",
        "private profile",
        "access denied",
        "permission denied",
        "forbidden",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return false;
    }

    [
        "gallery-dl json ciktisi",
        "gallery-dl json sonucu",
        "json sonucu okunamadi",
        "schema",
        "ayristir",
        "parse error",
        "bos analiz sonucu",
        "indirilebilir medya url",
        "indirilebilir medya bulunamadi",
        "indirilebilir gorsel bulunamadi",
        "indirilebilir fotograf bulunamadi",
        "extractor mismatch",
        "unsupported extractor",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn gallery_error_indicates_auth_failure(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();

    [
        "authenticationerror",
        "login required",
        "login_required",
        "redirect to login page",
        "not logged in",
        "please login",
        "valid sessionid",
        "sessionid cookie",
        "cookies are required",
        "401 unauthorized",
        "403 forbidden",
        "oturumu bulunamadi",
        "oturumu bulunamadı",
        "oturum dogrulanamadi",
        "oturum doğrulanamadı",
        "cookie verisi gecersiz",
        "cookie verisi geçersiz",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn instagram_story_error_code(error: &str) -> Option<&'static str> {
    let lower = error.to_ascii_lowercase();
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests")
    {
        return Some("instagram_rate_limited");
    }
    if lower.contains("private")
        || lower.contains("access denied")
        || lower.contains("permission denied")
        || lower.contains("403 forbidden")
    {
        return Some("instagram_story_access_denied");
    }
    if lower.contains("json ciktisi")
        || lower.contains("json sonucu")
        || lower.contains("schema")
        || lower.contains("ayristir")
        || lower.contains("parse")
    {
        return Some("instagram_schema_error");
    }
    if lower.trim().is_empty()
        || lower.contains("not found")
        || lower.contains("404")
        || lower.contains("expired")
        || lower.contains("no stories")
        || lower.contains("hikaye bulunamadi")
        || lower.contains("indirilebilir medya")
        || lower.contains("indirilebilir gorsel")
    {
        return Some("instagram_story_not_found");
    }
    None
}

fn gallerydl_collect_media(
    app: &tauri::AppHandle,
    clean_url: &str,
    auth_mode: Option<&str>,
) -> Result<MediaAnalysis, String> {
    if !is_supported_media_url(clean_url) {
        return Err(unsupported_media_link_message().to_string());
    }

    let platform = media_platform_from_url(clean_url);
    if let Some(error) = instagram_highlight_unsupported_error(clean_url) {
        return Err(error);
    }
    let gallery_dl = ensure_runtime_tool(app, "gallery-dl")?;
    let fallback_title = gallery_media_title(&platform, clean_url);
    let story_request = if platform == "instagram" {
        instagram_story_request(clean_url)
    } else {
        None
    };
    let extraction_url = clean_url.to_string();
    let mut errors = Vec::new();
    let mut tried_instagram_public_fallback = false;
    let mut saw_auth_failure = false;
    let public_only = auth_mode.unwrap_or("browserAuto").trim() == "public";

    let auth_attempts = if story_request.is_some() {
        let requested_mode = auth_mode.unwrap_or("browserAuto").trim();
        if matches!(requested_mode, "" | "browserAuto" | "public") {
            if read_instagram_cookie_state_blocking().has_saved_cookies {
                vec![GalleryAuthAttempt::SavedInstagramCookies]
            } else {
                return Err(structured_backend_error(
                    "instagram_auth_required",
                    "Instagram hikayeleri icin gecerli kayitli oturum gerekiyor.",
                ));
            }
        } else {
            gallery_auth_attempts(auth_mode)
        }
    } else {
        gallery_auth_attempts(auth_mode)
    };

    for auth in auth_attempts {
        if let Some(request) = story_request.as_ref() {
            match collect_instagram_story_attempt(
                &gallery_dl,
                clean_url,
                request,
                auth.clone(),
                &fallback_title,
            ) {
                Ok((mut items, requested_item_id)) => {
                    propagate_media_item_metadata(&mut items);
                    let title = items
                        .first()
                        .map(|item| item.title.clone())
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or_else(|| fallback_title.clone());
                    let uploader = items
                        .iter()
                        .find_map(|item| item.author_name.clone())
                        .unwrap_or_else(|| platform_label_for_backend(&platform).to_string());
                    return Ok(MediaAnalysis {
                        analysis_id: String::new(),
                        expires_at_ms: 0,
                        platform: platform.clone(),
                        content_kind: "story".to_string(),
                        title,
                        uploader,
                        author: AuthorIdentity::default(),
                        items,
                        initial_index: 0,
                        requested_item_id,
                        warnings: Vec::new(),
                        instagram_diagnostics: None,
                        twitter_quote: None,
                        twitter_post: None,
                        video_info: None,
                    });
                }
                Err(err) => {
                    if gallery_error_indicates_auth_failure(&err) {
                        saw_auth_failure = true;
                    }
                    errors.push(format!("{}: {}", auth.label(), sanitize_report_text(&err)));
                    continue;
                }
            }
        }

        match gallerydl_dump_with_fallback(
            &gallery_dl,
            &extraction_url,
            auth.clone(),
            story_request.is_some(),
        ) {
            Ok(stdout) => match gallery_stdout_to_inventory(&stdout, &platform, &fallback_title) {
                Ok(mut inventory)
                    if !inventory.items.is_empty()
                        || inventory.twitter_quote.is_some()
                        || inventory.twitter_post.is_some() =>
                {
                    let mut items = std::mem::take(&mut inventory.items);
                    propagate_media_item_metadata(&mut items);
                    let quote_outer = inventory
                        .twitter_quote
                        .as_ref()
                        .map(|quote| &quote.outer);
                    let primary_post = quote_outer.or(inventory.twitter_post.as_ref());
                    let title = primary_post
                        .and_then(|post| post.text.clone())
                        .or_else(|| {
                            items
                                .first()
                                .map(|item| item.title.clone())
                                .filter(|title| !title.trim().is_empty())
                        })
                        .unwrap_or_else(|| fallback_title.clone());
                    let uploader = primary_post
                        .map(|post| post.author_name.clone())
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| items.iter().find_map(|item| item.author_name.clone()))
                        .unwrap_or_else(|| platform_label_for_backend(&platform).to_string());
                    let content_kind = if items.is_empty() {
                        "text".to_string()
                    } else {
                        media_content_kind(&items)
                    };
                    return Ok(MediaAnalysis {
                        analysis_id: String::new(),
                        expires_at_ms: 0,
                        platform,
                        content_kind,
                        title,
                        uploader,
                        author: primary_post
                            .map(|post| AuthorIdentity {
                                id: None,
                                name: post.author_name.clone(),
                                handle: post.author_handle.clone(),
                                avatar_data_url: None,
                            })
                            .unwrap_or_default(),
                        items,
                        initial_index: 0,
                        requested_item_id: None,
                        warnings: Vec::new(),
                        instagram_diagnostics: None,
                        twitter_quote: inventory.twitter_quote,
                        twitter_post: inventory.twitter_post,
                        video_info: None,
                    });
                }
                Ok(_) => {
                    if matches!(auth, GalleryAuthAttempt::None) && !tried_instagram_public_fallback
                    {
                        tried_instagram_public_fallback = true;
                        if let Some(analysis) =
                            try_instagram_public_html_fallback(clean_url, &platform, &mut errors)
                        {
                            return Ok(analysis);
                        }
                    }

                    errors.push(format!(
                        "{}: medyada indirilebilir gorsel bulunamadi",
                        auth.label()
                    ));
                }
                Err(err) => {
                    if matches!(auth, GalleryAuthAttempt::None) && !tried_instagram_public_fallback
                    {
                        tried_instagram_public_fallback = true;
                        if let Some(analysis) =
                            try_instagram_public_html_fallback(clean_url, &platform, &mut errors)
                        {
                            return Ok(analysis);
                        }
                    }

                    errors.push(format!("{}: {}", auth.label(), sanitize_report_text(&err)));
                }
            },
            Err(err) => {
                if platform == "instagram" && gallery_error_indicates_auth_failure(&err) {
                    saw_auth_failure = true;
                }

                if matches!(auth, GalleryAuthAttempt::None) && !tried_instagram_public_fallback {
                    tried_instagram_public_fallback = true;
                    if let Some(analysis) =
                        try_instagram_public_html_fallback(clean_url, &platform, &mut errors)
                    {
                        return Ok(analysis);
                    }
                }

                errors.push(format!("{}: {}", auth.label(), sanitize_report_text(&err)));
            }
        }
    }

    let combined = errors.join("\n");

    if story_request.is_some() {
        if let Some(code) = instagram_story_error_code(&combined) {
            return Err(structured_backend_error(code, &combined));
        }
    }

    if platform == "instagram" && public_only && tried_instagram_public_fallback {
        let public_details = errors.join("\n");

        return Err(if public_details.trim().is_empty() {
            "Instagram public gonderisinde girissiz okunabilir fotograf bulunamadi.".to_string()
        } else {
            format!(
                "Instagram public gonderisinde girissiz okunabilir fotograf bulunamadi.\n{}",
                public_details
            )
        });
    }

    if platform == "instagram" && saw_auth_failure {
        return Err(structured_backend_error(
            "instagram_auth_required",
            &format!(
                "Instagram oturumu gecersiz veya suresi dolmus.\n{}",
                combined
            ),
        ));
    }

    Err(if combined.trim().is_empty() {
        format!(
            "{} linkinde indirilebilir fotograf veya hikaye bulunamadi.",
            platform_label_for_backend(&platform)
        )
    } else {
        combined
    })
}

fn platform_label_for_backend(platform: &str) -> &'static str {
    match platform {
        "instagram" => "Instagram",
        "twitter" => "X/Twitter",
        "tiktok" => "TikTok",
        "youtube" => "YouTube",
        _ => "Medya",
    }
}

fn media_url_host_allowed(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    let clean = host
        .trim()
        .trim_matches('.')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase();
    if clean == "localhost" || clean.ends_with(".localhost") {
        return false;
    }

    if let Ok(ip) = clean.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                !(v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.octets()[0] == 0)
            }
            IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_unspecified() || v6.is_unique_local()),
        };
    }

    true
}

fn media_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_MEDIA_PREVIEW_REDIRECTS {
            return attempt.error("Gorsel redirect limiti asildi.");
        }

        if !media_url_host_allowed(attempt.url()) {
            return attempt.error("Gorsel redirect hedefi guvenli degil.");
        }

        attempt.follow()
    })
}

fn sanitize_media_extension(ext: &str) -> String {
    let clean = ext.trim().trim_start_matches('.').to_lowercase();
    if supported_image_extension(&clean) || supported_video_extension(&clean) {
        clean
    } else {
        "jpg".to_string()
    }
}

fn media_output_base(
    platform: &str,
    title: &str,
    source_index: usize,
    total: usize,
    item_type: &str,
) -> String {
    let title = pretty_media_title(platform, Some(title));
    let kind = if item_type == "video" {
        "Video"
    } else {
        "Fotograf"
    };
    let label = if total > 1 {
        format!("{} {}", kind, source_index + 1)
    } else {
        kind.to_string()
    };

    truncate_filename_chars(&format!("{} {}", title, label), 96)
}

fn media_batch_dir_name(platform: &str, title: &str) -> String {
    let prefix = match platform {
        "instagram" => "Instagram Fotograflari",
        "twitter" => "X Fotograflari",
        "tiktok" => "TikTok Fotograflari",
        _ => "MediaDrop Fotograflari",
    };
    let title = pretty_media_title(platform, Some(title));
    truncate_filename_chars(&format!("{} - {}", prefix, title), 96)
}

fn materialize_cached_media_file(source: &Path, target: &Path) -> Result<u64, String> {
    let size = fs::metadata(source)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("Onbellekteki medya okunamadi: {error}"))?;
    if fs::hard_link(source, target).is_ok() {
        return Ok(size);
    }
    fs::copy(source, target)
        .map_err(|error| format!("Onbellekteki medya kopyalanamadi: {error}"))
}

fn media_progress_update_due(elapsed: Duration, finished: bool) -> bool {
    finished || elapsed >= MEDIA_PROGRESS_EMIT_INTERVAL
}

fn download_media_url_to_file(
    app: &tauri::AppHandle,
    url: &str,
    referer: &str,
    expected_type: &str,
    output_path: &Path,
    progress_start: f64,
    progress_end: f64,
) -> Result<u64, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Gorsel linki gecersiz.".to_string())?;

    if !media_url_host_allowed(&parsed) {
        return Err("Gorsel linki guvenli degil.".to_string());
    }

    let client = media_client(Duration::from_secs(90))?;
    let mut request = client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .header(reqwest::header::ACCEPT, "video/mp4,video/webm,image/avif,image/webp,image/png,image/jpeg,*/*;q=0.8");

    if !referer.trim().is_empty() {
        request = request.header(reqwest::header::REFERER, referer);
    }

    let mut response = request
        .send()
        .map_err(|err| format!("Gorsel indirilemedi: {}", err))?;

    if !response.status().is_success() {
        return Err(format!("Gorsel indirme HTTP {} dondu.", response.status()));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if expected_type == "audio" {
        validate_declared_audio_type(&content_type)?;
    } else {
        validate_declared_media_type(expected_type, &content_type)?;
    }

    let total_bytes = response.content_length().unwrap_or(0);
    if total_bytes > MAX_MEDIA_DOWNLOAD_BYTES as u64 {
        return Err("Gorsel dosyasi beklenenden cok buyuk.".to_string());
    }

    let output_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = fs::File::create(output_path)
        .map_err(|err| format!("Gorsel dosyasi olusturulamadi: {}", err))?;
    let mut written: u64 = 0;
    let mut buffer = [0u8; 64 * 1024];
    let mut magic_prefix = Vec::with_capacity(MEDIA_MAGIC_PREFIX_BYTES);
    let mut magic_validated = false;
    let mut last_progress_at = Instant::now();
    let emit_download_progress = |written: u64| {
        let ratio = if total_bytes > 0 {
            (written as f64 / total_bytes as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let percent = if total_bytes > 0 {
            Some(progress_start + (progress_end - progress_start) * ratio)
        } else {
            None
        };
        let total_mb = if total_bytes > 0 {
            Some(total_bytes as f64 / 1024.0 / 1024.0)
        } else {
            None
        };

        emit_progress(
            app,
            percent,
            Some(written as f64 / 1024.0 / 1024.0),
            total_mb,
            None,
            "Fotograf indiriliyor...",
            "",
        );
    };

    loop {
        check_active_range_stop(output_dir, output_path)?;
        let read = response
            .read(&mut buffer)
            .map_err(|err| format!("Gorsel verisi okunamadi: {}", err))?;

        if read == 0 {
            break;
        }

        if magic_prefix.len() < MEDIA_MAGIC_PREFIX_BYTES {
            let copy_len = (MEDIA_MAGIC_PREFIX_BYTES - magic_prefix.len()).min(read);
            magic_prefix.extend_from_slice(&buffer[..copy_len]);
        }
        if !magic_validated && magic_prefix.len() >= 12 {
            let validation = if expected_type == "audio" {
                validate_audio_preview_magic(&content_type, &magic_prefix).map(|_| ())
            } else {
                validate_preview_magic(expected_type, &content_type, &magic_prefix).map(|_| ())
            };
            if let Err(error) = validation {
                let _ = fs::remove_file(output_path);
                return Err(error);
            }
            magic_validated = true;
        }

        written = written.saturating_add(read as u64);
        if written > MAX_MEDIA_DOWNLOAD_BYTES as u64 {
            let _ = fs::remove_file(output_path);
            return Err("Gorsel dosyasi beklenenden cok buyuk.".to_string());
        }

        file.write_all(&buffer[..read])
            .map_err(|err| format!("Gorsel dosyaya yazilamadi: {}", err))?;

        if media_progress_update_due(last_progress_at.elapsed(), false) {
            emit_download_progress(written);
            last_progress_at = Instant::now();
        }
    }

    if media_progress_update_due(last_progress_at.elapsed(), true) {
        emit_download_progress(written);
    }

    if !magic_validated {
        let validation = if expected_type == "audio" {
            validate_audio_preview_magic(&content_type, &magic_prefix).map(|_| ())
        } else {
            validate_preview_magic(expected_type, &content_type, &magic_prefix).map(|_| ())
        };
        if let Err(error) = validation {
            let _ = fs::remove_file(output_path);
            return Err(error);
        }
    }

    file.flush()
        .map_err(|err| format!("Gorsel dosyasi flush edilemedi: {}", err))?;

    if written < 64 {
        let _ = fs::remove_file(output_path);
        return Err("Gorsel dosyasi bos veya gecersiz.".to_string());
    }

    Ok(written)
}

#[allow(clippy::too_many_arguments)]
fn completed_media_file(
    app: &tauri::AppHandle,
    download_dir: &Path,
    source_url: &str,
    referer: &str,
    title: &str,
    platform: &str,
    source_index: usize,
    total: usize,
    extension: &str,
    item_type: &str,
    audio_url: Option<&str>,
    expect_audio: bool,
    progress_start: f64,
    progress_end: f64,
    cached_source: Option<&Path>,
) -> Result<MediaDownloadFile, String> {
    let cached_extension = cached_source
        .and_then(Path::extension)
        .and_then(OsStr::to_str)
        .unwrap_or(extension);
    let ext = sanitize_media_extension(cached_extension);
    let base = media_output_base(platform, title, source_index, total, item_type);
    let temp_name = format!(
        "{}.{}",
        temp_output_stem(
            if item_type == "video" {
                "media-video"
            } else {
                "media-photo"
            },
            source_url,
            &source_index.to_string(),
            &ext,
            None
        ),
        ext
    );
    let temp_path = download_dir.join(temp_name);
    let clean_audio_url = if cached_source.is_some() {
        None
    } else {
        audio_url
            .map(str::trim)
            .filter(|url| !url.is_empty() && *url != source_url && item_type == "video")
    };
    let audio_temp_path = download_dir.join(format!(
        "{}.m4a",
        temp_output_stem(
            "media-audio",
            source_url,
            &source_index.to_string(),
            "m4a",
            None
        )
    ));
    let mux_temp_path = download_dir.join(format!(
        "{}.mp4",
        temp_output_stem(
            "media-mux",
            source_url,
            &source_index.to_string(),
            "mp4",
            None
        )
    ));

    let result = (|| {
        let video_end = if clean_audio_url.is_some() {
            progress_start + (progress_end - progress_start) * 0.68
        } else {
            progress_end
        };
        let written = if let Some(cached) = cached_source {
            check_active_range_stop(download_dir, &temp_path)?;
            emit_simple_progress(app, Some(progress_start), "Medya onbellekten hazirlaniyor...");
            match materialize_cached_media_file(cached, &temp_path) {
                Ok(size) => {
                    check_active_range_stop(download_dir, &temp_path)?;
                    emit_simple_progress(app, Some(video_end), "Medya onbellekten hazirlandi.");
                    size
                }
                Err(_) => download_media_url_to_file(
                    app,
                    source_url,
                    referer,
                    item_type,
                    &temp_path,
                    progress_start,
                    video_end,
                )?,
            }
        } else {
            download_media_url_to_file(
                app,
                source_url,
                referer,
                item_type,
                &temp_path,
                progress_start,
                video_end,
            )?
        };
        let mut completed_temp = temp_path.clone();

        if let Some(audio_url) = clean_audio_url {
            download_media_url_to_file(
                app,
                audio_url,
                referer,
                "audio",
                &audio_temp_path,
                video_end,
                progress_start + (progress_end - progress_start) * 0.84,
            )?;
            emit_simple_progress(app, None, "Story video sesi birleştiriliyor...");
            let ffmpeg = ensure_runtime_tool(app, "ffmpeg")?;
            let ffprobe = ensure_runtime_tool(app, "ffprobe")?;
            mux_separate_video_audio(
                &ffmpeg,
                &ffprobe,
                &temp_path,
                &audio_temp_path,
                &mux_temp_path,
            )
            .map_err(|err| format!("Story video ve ses birleştirilemedi: {}", err))?;
            completed_temp = mux_temp_path.clone();
        } else if item_type == "video" && expect_audio {
            emit_simple_progress(app, None, "Story video sesi doğrulanıyor...");
            let ffprobe = ensure_runtime_tool(app, "ffprobe")?;
            let has_audio = probe_media_has_audio_stream(&ffprobe, &temp_path)
                .map_err(|err| format!("Story video sesi doğrulanamadı: {}", err))?;
            if !has_audio {
                return Err("Story videosunda beklenen ses akışı bulunamadı.".to_string());
            }
        }

        let final_path = finalize_pretty_output_file(&completed_temp, download_dir, &base);
        let size = file_size(&final_path).unwrap_or(written);
        Ok(MediaDownloadFile {
            file_path: final_path.to_string_lossy().to_string(),
            file_size: size,
            title: title.to_string(),
            source_index,
        })
    })();

    let _ = fs::remove_file(&temp_path);
    let _ = fs::remove_file(&audio_temp_path);
    let _ = fs::remove_file(&mux_temp_path);
    result
}

fn selected_media_items(items: &[MediaItem], scope: &str) -> Vec<MediaItem> {
    items
        .iter()
        .filter(|item| match scope {
            "photos" => item.item_type == "photo",
            "all-stories" => item.is_story && matches!(item.item_type.as_str(), "photo" | "video"),
            _ => matches!(item.item_type.as_str(), "photo" | "video"),
        })
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn completed_analysis_media_item(
    app: &tauri::AppHandle,
    download_dir: &Path,
    analysis: &MediaAnalysis,
    source_url: &str,
    item_id: &str,
    scope: &str,
    total: usize,
    progress_start: f64,
    progress_end: f64,
) -> Result<MediaDownloadFile, String> {
    let item = selected_media_items(&analysis.items, scope)
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "Secili medya analiz kaydinda bulunamadi.".to_string())?;
    let cached_source = cached_media_preview_path(app, &analysis.analysis_id, item_id, &item);
    let display_title = companion::protocol::media_display_title(analysis, &item);
    completed_media_file(
        app,
        download_dir,
        &item.preview_url,
        source_url,
        &display_title,
        &analysis.platform,
        item.source_index,
        total.max(1),
        &item.extension,
        &item.item_type,
        item.audio_url.as_deref(),
        item.has_audio,
        progress_start,
        progress_end,
        cached_source.as_deref(),
    )
}

fn twitter_post_registry_source_item<'a>(
    platform: &str,
    items: &'a [MediaItem],
    item_id: &str,
) -> Result<&'a MediaItem, String> {
    if platform != "twitter" {
        return Err("Gönderi video kaydı X/Twitter analizine ait değil.".to_string());
    }
    let item = items
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "Seçili gönderi videosu analiz kaydında bulunamadı.".to_string())?;
    if item.item_type != "video" {
        return Err("Seçili gönderi medyası video değil.".to_string());
    }
    Ok(item)
}

fn materialize_twitter_post_registry_video(
    app: &tauri::AppHandle,
    temp_dir: &Path,
    source_url: &str,
    analysis_id: Option<&str>,
    item_id: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let analysis_id = analysis_id.map(str::trim).filter(|value| !value.is_empty());
    let item_id = item_id.map(str::trim).filter(|value| !value.is_empty());
    let (analysis_id, item_id) = match (analysis_id, item_id) {
        (None, None) => return Ok(None),
        (Some(analysis_id), Some(item_id)) => {
            require_media_registry_identity(analysis_id, item_id)?
        }
        _ => {
            return Err(
                "Gönderi videosu için analysisId ve itemId birlikte gönderilmelidir."
                    .to_string(),
            )
        }
    };

    let mut registered = registered_media_analysis(&analysis_id)?;
    let mut refresh_attempts = 0;
    loop {
        if registered.source_url.trim() != source_url.trim() {
            return Err("Gönderi videosu analiz edilen X/Twitter bağlantısıyla eşleşmiyor.".to_string());
        }
        twitter_post_registry_source_item(
            &registered.analysis.platform,
            &registered.analysis.items,
            &item_id,
        )?;
        let total = selected_media_items(&registered.analysis.items, "all").len();
        match completed_analysis_media_item(
            app,
            temp_dir,
            &registered.analysis,
            &registered.source_url,
            &item_id,
            "all",
            total,
            0.0,
            80.0,
        ) {
            Ok(file) => return Ok(Some(PathBuf::from(file.file_path))),
            Err(error) if media_error_allows_registry_refresh(&error, refresh_attempts) => {
                refresh_attempts += 1;
                emit_simple_progress(app, None, "Gönderi video bağlantısı yenileniyor...");
                registered = refresh_registered_media_analysis(app, &analysis_id)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn unique_output_dir(parent: &Path, base_name: &str) -> PathBuf {
    let clean = pretty_output_filename_part(base_name, "MediaDrop Fotograflari");
    let first = parent.join(&clean);

    if !first.exists() {
        return first;
    }

    for index in 2..1000 {
        let candidate = parent.join(format!("{} ({})", clean, index));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{} {}", clean, unique_stamp()))
}

#[tauri::command]
async fn analyze_media(
    app: tauri::AppHandle,
    url: String,
    auth_mode: Option<String>,
) -> ApiResult<MediaAnalysis> {
    let result: Result<MediaAnalysis, String> = tauri::async_runtime::spawn_blocking(move || {
        let clean_url = url.trim().to_string();

        if clean_url.is_empty() {
            return Err("Bos link gonderildi.".to_string());
        }

        match collect_media(&app, &clean_url, auth_mode.as_deref()) {
            Ok(analysis) => {
                register_media_analysis(&analysis, &clean_url, auth_mode.as_deref())?;
                Ok(analysis)
            }
            Err(err) => {
                let platform = media_platform_from_url(&clean_url);
                let ctx = ErrorReportContext::new("media-analysis", &clean_url, &err)
                    .platform(platform_label_for_backend(&platform))
                    .kind("photo-story-analysis")
                    .output_mode("media-inventory");

                Err(with_error_report(&app, err, ctx))
            }
        }
    })
    .await
    .map_err(|err| {
        ApiError::new(
            "thread_error",
            format!("Medya analiz thread hatasi: {}", err),
        )
    })?;
    result.map_err(ApiError::from)
}

fn download_media_item_blocking_with_job(
    app: tauri::AppHandle,
    analysis_id: String,
    item_id: String,
    output_dir: Option<String>,
    _job: DownloadJobGuard,
) -> Result<MediaDownloadResult, String> {
    let (analysis_id, item_id) = require_media_registry_identity(&analysis_id, &item_id)?;

    let started_at_ms = now_ms();
    let mut registered = registered_media_analysis(&analysis_id)?;
    let clean_url = registered.source_url.clone();
    let download_dir = resolve_download_dir(&app, output_dir.as_deref())?;

    mark_active_range(&download_dir, started_at_ms);
    emit_simple_progress(&app, Some(0.0), "Medya indiriliyor...");

    let result = (|| {
        let mut refresh_attempts = 0;
        let file = loop {
            let total = selected_media_items(&registered.analysis.items, "all").len();
            match completed_analysis_media_item(
                &app,
                &download_dir,
                &registered.analysis,
                &clean_url,
                &item_id,
                "all",
                total,
                0.0,
                100.0,
            ) {
                Ok(file) => break file,
                Err(error) if media_error_allows_registry_refresh(&error, refresh_attempts) => {
                    refresh_attempts += 1;
                    emit_simple_progress(&app, None, "Medya baglantisi yenileniyor...");
                    registered = refresh_registered_media_analysis(&app, &analysis_id)?;
                }
                Err(error) => return Err(error),
            }
        };

        emit_simple_progress(&app, Some(100.0), "Medya indirildi.");

        Ok(MediaDownloadResult {
            message: "Medya indirildi.".to_string(),
            files: vec![file],
            output_dir: download_dir.to_string_lossy().to_string(),
            downloaded_count: 1,
            failed_count: 0,
            failures: Vec::new(),
            mode: "media_item".to_string(),
        })
    })();

    finish_active_range();
    result
}

#[tauri::command]
async fn download_media_item(
    app: tauri::AppHandle,
    analysis_id: String,
    item_id: String,
    output_dir: Option<String>,
) -> ApiResult<MediaDownloadResult> {
    let result: Result<MediaDownloadResult, String> =
        tauri::async_runtime::spawn_blocking(move || {
            let job = begin_download_job()?;
            download_media_item_blocking_with_job(app, analysis_id, item_id, output_dir, job)
        })
        .await
        .map_err(|err| {
            ApiError::new(
                "thread_error",
                format!("Medya indirme thread hatasi: {}", err),
            )
        })?;
    result.map_err(ApiError::from)
}

fn download_media_batch_blocking(
    app: tauri::AppHandle,
    analysis_id: String,
    scope: String,
    output_dir: Option<String>,
) -> Result<MediaDownloadResult, String> {
    let _job = begin_download_job()?;
    download_media_batch_blocking_with_job(app, analysis_id, scope, output_dir, _job)
}

fn download_media_batch_blocking_with_job(
    app: tauri::AppHandle,
    analysis_id: String,
    scope: String,
    output_dir: Option<String>,
    _job: DownloadJobGuard,
) -> Result<MediaDownloadResult, String> {
            let analysis_id = analysis_id.trim();
            if analysis_id.is_empty() {
                return Err("Toplu medya indirme icin analysisId zorunludur.".to_string());
            }
            let scope = scope.trim();
            if !matches!(scope, "photos" | "all" | "all-stories") {
                return Err("Toplu indirme kapsami gecersiz.".to_string());
            }

            let started_at_ms = now_ms();
            let mut registered = registered_media_analysis(analysis_id)?;
            let clean_url = registered.source_url.clone();
            let media_items = selected_media_items(&registered.analysis.items, scope);
            if media_items.is_empty() {
                return Err("Bu linkte indirilebilir medya bulunamadi.".to_string());
            }
            let item_ids = media_items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();

            let parent_dir = resolve_download_dir(&app, output_dir.as_deref())?;
            let batch_name =
                media_batch_dir_name(&registered.analysis.platform, &registered.analysis.title);
            let batch_dir = unique_output_dir(&parent_dir, &batch_name);
            fs::create_dir_all(&batch_dir)
                .map_err(|err| format!("Fotograf klasoru olusturulamadi: {}", err))?;

            mark_active_range(&batch_dir, started_at_ms);
            emit_simple_progress(&app, Some(0.0), "Fotograflar indiriliyor...");

            let result = (|| {
                let total = item_ids.len();
                let mut files = Vec::new();
                let mut failures = Vec::new();
                let mut refresh_attempts = 0;

                for (index, item_id) in item_ids.iter().enumerate() {
                    let start = (index as f64 / total as f64) * 100.0;
                    let end = ((index + 1) as f64 / total as f64) * 100.0;
                    emit_simple_progress(
                        &app,
                        Some(start),
                        format!("Medya indiriliyor... ({}/{})", index + 1, total),
                    );

                    let initial_item = selected_media_items(&registered.analysis.items, scope)
                        .into_iter()
                        .find(|item| item.id == *item_id);
                    let source_index = initial_item
                        .as_ref()
                        .map(|item| item.source_index)
                        .unwrap_or(index);
                    let mut attempt = completed_analysis_media_item(
                        &app,
                        &batch_dir,
                        &registered.analysis,
                        &clean_url,
                        item_id,
                        scope,
                        total,
                        start,
                        end,
                    );
                    if attempt.as_ref().is_err_and(|error| {
                        media_error_allows_registry_refresh(error, refresh_attempts)
                    }) {
                        refresh_attempts += 1;
                        emit_simple_progress(&app, None, "Medya baglantilari yenileniyor...");
                        attempt = refresh_registered_media_analysis(&app, analysis_id).and_then(
                            |refreshed| {
                                registered = refreshed;
                                completed_analysis_media_item(
                                    &app,
                                    &batch_dir,
                                    &registered.analysis,
                                    &clean_url,
                                    item_id,
                                    scope,
                                    total,
                                    start,
                                    end,
                                )
                            },
                        );
                    }

                    match attempt {
                        Ok(file) => files.push(file),
                        Err(message) => failures.push(MediaDownloadFailure {
                            item_id: item_id.clone(),
                            source_index,
                            message,
                        }),
                    }
                }

                if files.is_empty() {
                    return Err(failures
                        .first()
                        .map(|failure| failure.message.clone())
                        .unwrap_or_else(|| "Medya indirilemedi.".to_string()));
                }
                emit_simple_progress(&app, Some(100.0), "Medya indirme tamamlandi.");

                Ok(MediaDownloadResult {
                    message: if failures.is_empty() {
                        format!("{} medya indirildi.", files.len())
                    } else {
                        format!(
                            "{} medya indirildi, {} medya indirilemedi.",
                            files.len(),
                            failures.len()
                        )
                    },
                    downloaded_count: files.len(),
                    failed_count: failures.len(),
                    failures,
                    files,
                    output_dir: batch_dir.to_string_lossy().to_string(),
                    mode: "media_batch".to_string(),
                })
            })();

            finish_active_range();
            result
}

#[tauri::command]
async fn download_media_batch(
    app: tauri::AppHandle,
    analysis_id: String,
    scope: String,
    output_dir: Option<String>,
) -> ApiResult<MediaDownloadResult> {
    let result: Result<MediaDownloadResult, String> =
        tauri::async_runtime::spawn_blocking(move || {
            download_media_batch_blocking(app, analysis_id, scope, output_dir)
        })
        .await
        .map_err(|err| {
            ApiError::new(
                "thread_error",
                format!("Toplu fotograf indirme thread hatasi: {}", err),
            )
        })?;
    result.map_err(ApiError::from)
}

fn download_media_post_card_blocking_with_job(
    app: tauri::AppHandle,
    url: String,
    image_data_url: String,
    title: Option<String>,
    output_dir: Option<String>,
    _job: DownloadJobGuard,
) -> Result<MediaDownloadResult, String> {
            let clean_url = url.trim().to_string();
            if clean_url.is_empty() {
                return Err("Bos link gonderildi.".to_string());
            }

            if !is_twitter_url(&clean_url) {
                return Err(
                    "Gonderi karti simdilik yalnizca X/Twitter icin destekleniyor.".to_string(),
                );
            }
            let bytes = decode_twitter_post_card_png(&image_data_url)?;
            let download_dir = resolve_download_dir(&app, output_dir.as_deref())?;
            let safe_title = title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("X/Twitter gonderisi");
            let temp_path = download_dir.join(format!(
                "{}.png",
                temp_output_stem("media-post-card", &clean_url, "twitter", "png", None)
            ));

            fs::write(&temp_path, &bytes)
                .map_err(|err| format!("Gonderi karti dosyaya yazilamadi: {}", err))?;

            let final_base = twitter_post_output_base(Some(safe_title), safe_title);
            let final_path = finalize_pretty_output_file(&temp_path, &download_dir, &final_base);
            let size = file_size(&final_path).unwrap_or(bytes.len() as u64);

            Ok(MediaDownloadResult {
                message: "Gonderi karti indirildi.".to_string(),
                files: vec![MediaDownloadFile {
                    file_path: final_path.to_string_lossy().to_string(),
                    file_size: size,
                    title: safe_title.to_string(),
                    source_index: 0,
                }],
                output_dir: download_dir.to_string_lossy().to_string(),
                downloaded_count: 1,
                failed_count: 0,
                failures: Vec::new(),
                mode: "media_post_card".to_string(),
            })
}

#[tauri::command]
async fn download_media_post_card(
    app: tauri::AppHandle,
    url: String,
    image_data_url: String,
    title: Option<String>,
    output_dir: Option<String>,
) -> ApiResult<MediaDownloadResult> {
    let result: Result<MediaDownloadResult, String> =
        tauri::async_runtime::spawn_blocking(move || {
            let job = begin_download_job()?;
            download_media_post_card_blocking_with_job(
                app,
                url,
                image_data_url,
                title,
                output_dir,
                job,
            )
        })
        .await
        .map_err(|err| {
            ApiError::new(
                "thread_error",
                format!("Gonderi karti indirme thread hatasi: {}", err),
            )
        })?;
    result.map_err(ApiError::from)
}

#[tauri::command]
async fn cache_twitter_avatar(url: String) -> ApiResult<String> {
    let result = tauri::async_runtime::spawn_blocking(move || cache_twitter_avatar_blocking(&url))
        .await
        .map_err(|err| ApiError::new("thread_error", format!("Avatar thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

fn cache_twitter_avatar_blocking(url: &str) -> Result<String, String> {
    let clean_url = url.trim();

    if clean_url.is_empty() {
        return Err("Avatar linki boş.".to_string());
    }

    let parsed =
        reqwest::Url::parse(clean_url).map_err(|_| "Avatar linki geçersiz.".to_string())?;

    if parsed.scheme() != "https" {
        return Err("Avatar yalnızca güvenli HTTPS üzerinden alınabilir.".to_string());
    }

    if !parsed
        .host_str()
        .map(twitter_avatar_host_allowed)
        .unwrap_or(false)
    {
        return Err("Avatar host'u X/Twitter kaynaklı değil.".to_string());
    }

    let client = twitter_avatar_client()?;

    let response = client
        .get(parsed.clone())
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .send()
        .map_err(|err| format!("Avatar alınamadı: {}", err))?;

    if !response.status().is_success() {
        return Err(format!("Avatar alınamadı: HTTP {}", response.status()));
    }

    let final_url = response.url().clone();

    if !twitter_avatar_url_allowed(&final_url) {
        return Err("Avatar yönlendirmesi X/Twitter dışına çıktı.".to_string());
    }

    if response
        .content_length()
        .map(|value| value as usize > MAX_TWITTER_AVATAR_BYTES)
        .unwrap_or(false)
    {
        return Err("Avatar dosyası çok büyük.".to_string());
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(avatar_mime_from_text);
    let bytes = read_response_body_with_limit(response, MAX_TWITTER_AVATAR_BYTES)?;

    if bytes.is_empty() {
        return Err("Avatar verisi boş.".to_string());
    }

    if bytes.len() > MAX_TWITTER_AVATAR_BYTES {
        return Err("Avatar dosyası çok büyük.".to_string());
    }

    let mime = content_type
        .or_else(|| avatar_mime_from_url(&final_url))
        .ok_or_else(|| "Avatar görsel tipi desteklenmiyor.".to_string())?;

    if !avatar_bytes_match_mime(&bytes, mime) {
        return Err("Avatar görsel verisi geçersiz.".to_string());
    }

    Ok(image_bytes_to_data_url(&bytes, mime))
}

fn decode_twitter_profile_html_text(value: &str) -> String {
    value
        .replace("\\u002F", "/")
        .replace("\\/", "/")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("\\\"", "\"")
}

fn twitter_profile_avatar_path_allowed(url: &reqwest::Url) -> bool {
    let path = url.path().to_lowercase();
    let host = url.host_str().unwrap_or("").to_lowercase();

    path.contains("/profile_images/")
        || (host == "abs.twimg.com" && path.contains("/default_profile_images/"))
}

fn trim_twitter_avatar_candidate(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| {
            !matches!(
                ch,
                '"' | '\'' | '<' | '>' | ')' | '(' | ' ' | '\n' | '\r' | '\t'
            )
        })
        .collect::<String>()
}

fn extract_twitter_profile_avatar_url(html: &str) -> Option<String> {
    let decoded = decode_twitter_profile_html_text(html);
    let markers = [
        "https://pbs.twimg.com/profile_images/",
        "https://abs.twimg.com/sticky/default_profile_images/",
    ];

    for marker in markers {
        let mut search_start = 0;

        while let Some(relative_start) = decoded[search_start..].find(marker) {
            let start = search_start + relative_start;
            let candidate = trim_twitter_avatar_candidate(&decoded[start..]);
            search_start = start.saturating_add(marker.len());

            let Ok(parsed) = reqwest::Url::parse(&candidate) else {
                continue;
            };

            if twitter_avatar_url_allowed(&parsed) && twitter_profile_avatar_path_allowed(&parsed) {
                return Some(candidate);
            }
        }
    }

    None
}

#[tauri::command]
async fn resolve_twitter_avatar_by_handle(handle: String) -> ApiResult<String> {
    let result = tauri::async_runtime::spawn_blocking(move || resolve_twitter_avatar_by_handle_blocking(&handle))
        .await
        .map_err(|err| ApiError::new("thread_error", format!("Avatar profil thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

fn resolve_twitter_avatar_by_handle_blocking(handle: &str) -> Result<String, String> {
    let clean_handle = clean_twitter_handle(handle)?;
    let profile_url = format!("https://x.com/{}", clean_handle);
    let parsed =
        reqwest::Url::parse(&profile_url).map_err(|_| "Profil linki geçersiz.".to_string())?;

    if !twitter_profile_url_allowed(&parsed) {
        return Err("Profil linki X/Twitter kaynaklı değil.".to_string());
    }

    let client = twitter_profile_client()?;

    let response = client
        .get(parsed)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .send()
        .map_err(|err| format!("Profil sayfası alınamadı: {}", err))?;

    if !response.status().is_success() {
        return Err(format!(
            "Profil sayfası alınamadı: HTTP {}",
            response.status()
        ));
    }

    if !twitter_profile_url_allowed(response.url()) {
        return Err("Profil yönlendirmesi X/Twitter dışına çıktı.".to_string());
    }

    if response
        .content_length()
        .map(|value| value as usize > MAX_TWITTER_PROFILE_HTML_BYTES)
        .unwrap_or(false)
    {
        return Err("Profil sayfası çok büyük.".to_string());
    }

    let bytes = read_response_body_with_limit(response, MAX_TWITTER_PROFILE_HTML_BYTES)?;
    let html = String::from_utf8_lossy(&bytes);
    let avatar_url = extract_twitter_profile_avatar_url(&html)
        .ok_or_else(|| "Profil sayfasında avatar linki bulunamadı.".to_string())?;

    cache_twitter_avatar_blocking(&avatar_url)
}

#[tauri::command]
async fn cache_thumbnail(app: tauri::AppHandle, url: String) -> ApiResult<String> {
    let result = tauri::async_runtime::spawn_blocking(move || cache_thumbnail_blocking(app, url))
        .await
        .map_err(|err| ApiError::new("thread_error", format!("Thumbnail thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

fn cache_thumbnail_blocking(app: tauri::AppHandle, url: String) -> Result<String, String> {
    let clean_url = url.trim();

    if clean_url.is_empty() {
        return Err("Boş link gönderildi.".to_string());
    }

    let tools = ensure_runtime_tools(&app)?;
    let thumb_root = mediadrop_thumbnail_dir()?;
    let work_dir = thumb_root.join(format!("{}{}", THUMBNAIL_TEMP_PREFIX, unique_stamp()));

    fs::create_dir_all(&work_dir)
        .map_err(|err| format!("Thumbnail geçici klasörü oluşturulamadı: {}", err))?;

    let mut command = ytdlp_command(&tools.yt_dlp);
    prepend_path(&mut command, &tools.ffmpeg_dir);

    command
        .arg("--skip-download")
        .arg("--write-thumbnail")
        .arg("--convert-thumbnails")
        .arg("jpg")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--windows-filenames")
        .arg("--restrict-filenames")
        .arg("--ffmpeg-location")
        .arg(&tools.ffmpeg_dir)
        .arg("-P")
        .arg(&work_dir)
        .arg("-o")
        .arg("thumb.%(ext)s");
    let _cookie_file = add_registered_ytdlp_cookies(&mut command, clean_url)?;
    command.arg(clean_url);

    let output = match capture_command_with_timeout(command, Duration::from_secs(25)) {
        Ok(output) => output,
        Err(err) => {
            remove_owned_temp_dir(&work_dir, &thumb_root, THUMBNAIL_TEMP_PREFIX);
            return Err(format!("Thumbnail alma komutu tamamlanamadı: {}", err));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if stderr.is_empty() {
            remove_owned_temp_dir(&work_dir, &thumb_root, THUMBNAIL_TEMP_PREFIX);
            return Err("Thumbnail alınamadı.".to_string());
        }

        remove_owned_temp_dir(&work_dir, &thumb_root, THUMBNAIL_TEMP_PREFIX);
        return Err(stderr);
    }

    let Some(thumb) = find_first_thumbnail_file(&work_dir) else {
        remove_owned_temp_dir(&work_dir, &thumb_root, THUMBNAIL_TEMP_PREFIX);
        return Err("Thumbnail dosyası oluşturulamadı.".to_string());
    };

    let result = file_to_data_url(&thumb);
    remove_owned_temp_dir(&work_dir, &thumb_root, THUMBNAIL_TEMP_PREFIX);

    result
}

fn run_ytdlp_json_analysis(
    app: &tauri::AppHandle,
    clean_url: &str,
    report_errors: bool,
    cookie_browser: Option<&str>,
    restart_browser: bool,
    force_close: bool,
) -> Result<String, String> {
    let clean_url = clean_url.trim();

    if clean_url.is_empty() {
        return Err("Boş link gönderildi.".to_string());
    }

    if !is_supported_media_url(clean_url) {
        return Err(unsupported_media_link_message().to_string());
    }

    let platform = platform_from_kind_or_url("", clean_url).to_string();
    if is_youtube_url(clean_url) {
        invalidate_youtube_analysis(clean_url);
    }

    let yt_dlp_path = match ensure_runtime_tool(app, "yt-dlp") {
        Ok(path) => path,
        Err(err) => {
            if report_errors {
                let ctx = ErrorReportContext::new("analysis", clean_url, &err)
                    .platform(platform)
                    .kind("analysis");
                return Err(with_error_report(app, err.clone(), ctx));
            }

            return Err(err);
        }
    };

    let _ = ensure_runtime_tool(app, "deno");

    let cookie_browser = cookie_browser
        .map(str::trim)
        .filter(|browser_id| !browser_id.is_empty());
    let browser_was_running = cookie_browser
        .map(|browser_id| !list_cookie_browser_processes(browser_id).is_empty())
        .unwrap_or(false);
    let browser_restart_available = browser_was_running && !restart_browser;
    let cookie_browser_spec = cookie_browser.map(ytdlp_cookie_browser_spec).transpose()?;
    let cookie_export = if cookie_browser_spec.is_some() {
        Some(TempArtifact::write(
            &std::env::temp_dir(),
            "mediadrop-ytdlp-cookie-export-",
            ".txt",
            b"# Netscape HTTP Cookie File\n",
        )?)
    } else {
        None
    };
    let auth_failure_code = if is_twitter_url(clean_url) {
        "twitter_auth_failed"
    } else {
        "youtube_auth_failed"
    };
    let relaunch_path = if restart_browser {
        cookie_browser
            .map(|browser_id| {
                close_cookie_browser_for_cookie_read(browser_id, force_close, auth_failure_code)
            })
            .transpose()?
            .flatten()
    } else {
        None
    };

    let run_analysis = || {
        let mut command = ytdlp_command(&yt_dlp_path);
        if let Some(runtime_dir) = yt_dlp_path.parent() {
            prepend_path(&mut command, runtime_dir);
        }

        command.arg("-J");

        if is_youtube_url(clean_url) {
            command.arg("--no-playlist");
        } else {
            command.arg("--ignore-no-formats-error");
            command.args(["--playlist-end", "20"]);
        }

        if let (Some(browser_spec), Some(export)) =
            (cookie_browser_spec.as_deref(), cookie_export.as_ref())
        {
            command
                .arg("--cookies-from-browser")
                .arg(browser_spec)
                .arg("--cookies")
                .arg(export.path());
        }

        command.args(["--no-warnings", "--socket-timeout", "20", clean_url]);
        capture_command_with_timeout(command, Duration::from_secs(55))
    };

    let mut output_result = run_analysis();
    let mut completed_attempts = 1;
    loop {
        let should_retry = match &output_result {
            Ok(output) if !output.status.success() => should_retry_tiktok_rehydration_error(
                clean_url,
                &String::from_utf8_lossy(&output.stderr),
                completed_attempts,
            ),
            _ => false,
        };
        if !should_retry {
            break;
        }
        output_result = run_analysis();
        completed_attempts += 1;
    }
    if let Some(path) = relaunch_path.as_deref() {
        let _ = relaunch_cookie_browser(path);
    }
    let output = match output_result {
        Ok(output) => output,
        Err(_err) => {
            return Err(format!(
                "{} analizi zamanında tamamlanamadı. Linki veya internet bağlantısını kontrol edip tekrar dene.",
                platform
            ));
        }
    };

    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr).to_string();

        let raw = if error_message.trim().is_empty() {
            "yt-dlp analiz sırasında hata verdi.".to_string()
        } else {
            error_message
        };
        if is_youtube_url(clean_url) {
            if let Some(error) = youtube_cookie_analysis_error(
                &raw,
                cookie_browser,
                browser_restart_available,
            ) {
                return Err(error);
            }
        } else if is_twitter_url(clean_url) {
            if let Some(error) = twitter_cookie_analysis_error(
                &raw,
                cookie_browser,
                browser_restart_available,
            ) {
                return Err(error);
            }
        }
        if let Some(friendly) = friendly_media_access_error(&platform, &raw) {
            return Err(friendly);
        }

        if report_errors {
            let ctx = ErrorReportContext::new("analysis", clean_url, &raw)
                .platform(platform)
                .kind("analysis");

            return Err(with_error_report(app, raw.clone(), ctx));
        }

        return Err(raw);
    }

    let stdout_json = String::from_utf8_lossy(&output.stdout).to_string();

    if let (Some(browser_id), Some(export)) = (cookie_browser, cookie_export.as_ref()) {
        let text = fs::read_to_string(export.path())
            .map_err(|err| format!("Tarayıcı cookie çıktısı okunamadı: {}", err))?;
        register_ytdlp_cookie_jar(clean_url, browser_id, &text)?;
    }

    if is_twitter_url(clean_url) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout_json) {
            if twitter_ytdlp_analysis_is_placeholder(&value) {
                let (code, message) = if cookie_browser.is_some() {
                    (
                        "twitter_auth_failed",
                        "Seçili tarayıcı oturumuyla bu X/Twitter gönderisinin medyası okunamadı. X'e giriş yaptığın başka bir tarayıcı seç.",
                    )
                } else {
                    (
                        "twitter_auth_required",
                        "Bu X/Twitter gönderisindeki medya oturumsuz isteklerde gösterilmiyor. X'e giriş yaptığın tarayıcı oturumuna izin vermelisin.",
                    )
                };
                return Err(structured_backend_error(code, message));
            }
        }
    }

    if is_youtube_url(clean_url) {
        let _ = cache_youtube_analysis(clean_url, &stdout_json);
    }

    Ok(stdout_json)
}

fn should_retry_tiktok_rehydration_error(
    url: &str,
    error: &str,
    completed_attempts: usize,
) -> bool {
    completed_attempts < 6
        && is_tiktok_url(url)
        && error
            .to_ascii_lowercase()
            .contains("unable to extract universal data for rehydration")
}

fn ytdlp_format_has_downloadable_video(format: &serde_json::Value) -> bool {
    let vcodec = format
        .get("vcodec")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if vcodec.is_empty() || vcodec == "none" {
        return false;
    }

    let ext = format
        .get("ext")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let has_protocol = format
        .get("protocol")
        .and_then(|value| value.as_str())
        .is_some();
    let has_url = format
        .get("url")
        .and_then(|value| value.as_str())
        .map(|value| value.starts_with("http://") || value.starts_with("https://"))
        .unwrap_or(false);

    ext == "mp4" || has_protocol || has_url
}

fn ytdlp_json_has_downloadable_video(value: &serde_json::Value) -> bool {
    if value
        .get("formats")
        .and_then(|item| item.as_array())
        .map(|formats| formats.iter().any(ytdlp_format_has_downloadable_video))
        .unwrap_or(false)
    {
        return true;
    }

    if value
        .get("requested_formats")
        .and_then(|item| item.as_array())
        .map(|formats| formats.iter().any(ytdlp_format_has_downloadable_video))
        .unwrap_or(false)
    {
        return true;
    }

    if ytdlp_format_has_downloadable_video(value) {
        return true;
    }

    value
        .get("entries")
        .and_then(|item| item.as_array())
        .map(|entries| entries.iter().any(ytdlp_json_has_downloadable_video))
        .unwrap_or(false)
}

fn twitter_ytdlp_analysis_is_placeholder(value: &serde_json::Value) -> bool {
    if ytdlp_json_has_downloadable_video(value) {
        return false;
    }

    let title = value
        .get("title")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let synthetic_title = title
        .strip_prefix("twitter video #")
        .is_some_and(|id| !id.is_empty() && id.chars().all(|character| character.is_ascii_digit()));
    if !synthetic_title {
        return false;
    }

    let has_real_metadata = [
        "description",
        "uploader",
        "uploader_id",
        "channel",
        "channel_id",
        "creator",
        "thumbnail",
    ]
    .into_iter()
    .any(|key| {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .is_some_and(|item| !item.trim().is_empty())
    }) || value
        .get("thumbnails")
        .and_then(|item| item.as_array())
        .is_some_and(|items| !items.is_empty());

    !has_real_metadata
}

#[tauri::command]
fn probe_instagram_video(app: tauri::AppHandle, url: &str) -> ApiResult<Option<String>> {
    let clean_url = url.trim();
    if clean_url.is_empty() || !is_instagram_url(clean_url) {
        return Ok(None);
    }

    let stdout = match run_ytdlp_json_analysis(&app, clean_url, false, None, false, false) {
        Ok(stdout) => stdout,
        Err(_) => return Ok(None),
    };

    let value = match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    if ytdlp_json_has_downloadable_video(&value) {
        Ok(Some(stdout))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn analyze_video(
    app: tauri::AppHandle,
    url: &str,
    cookie_browser: Option<String>,
    restart_browser: Option<bool>,
    force_close: Option<bool>,
) -> ApiResult<String> {
    run_ytdlp_json_analysis(
        &app,
        url,
        true,
        cookie_browser.as_deref(),
        restart_browser.unwrap_or(false),
        force_close.unwrap_or(false),
    )
    .map_err(ApiError::from)
}

#[tauri::command]
async fn prepare_clip_preview_stream(
    app: tauri::AppHandle,
    url: String,
    quality: Option<String>,
) -> ApiResult<PreviewStreamResult> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        let clean_url = url.trim().to_string();

        if clean_url.is_empty() {
            return Err("Boş YouTube linki gönderildi.".to_string());
        }

        let yt_dlp = ensure_runtime_tool(&app, "yt-dlp")?;
        let selected_quality = quality
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("720p")
            .to_string();

        let selector = youtube_preview_selector(&selected_quality);
        let preview_attempts: Vec<(&str, Option<&str>, bool, bool)> = vec![
            ("web / IPv4", Some("youtube:player_client=web"), true, false),
            (
                "mweb / IPv4",
                Some("youtube:player_client=mweb"),
                true,
                false,
            ),
            (
                "web embedded / IPv4",
                Some("youtube:player_client=web_embedded"),
                true,
                false,
            ),
            ("iOS / IPv4", Some("youtube:player_client=ios"), true, false),
            ("default / IPv4", None, true, false),
            (
                "web / normal DNS",
                Some("youtube:player_client=web"),
                false,
                false,
            ),
        ];

        let mut urls: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let started = Instant::now();

        for (label, extractor_args, force_ipv4, force_ipv6) in preview_attempts {
            let Some(timeout) = youtube_preview_attempt_timeout(started.elapsed()) else {
                errors.push("Ön izleme çözümleme toplam süre sınırına ulaştı.".to_string());
                break;
            };

            match ytdlp_stream_urls(
                &yt_dlp,
                &clean_url,
                &selector,
                extractor_args,
                force_ipv4,
                force_ipv6,
                timeout,
            ) {
                Ok(found) => {
                    urls = found.into_iter().take(2).collect();
                    if !urls.is_empty() {
                        break;
                    }
                }
                Err(err) => errors.push(format!("{}: {}", label, sanitize_report_text(&err))),
            }
        }

        if urls.is_empty() {
            return Err(format!(
                "Ön izleme stream URL bulunamadı. Denemeler:
{}",
                errors.join(
                    "
---
"
                )
            ));
        }

        let video_url = urls[0].clone();
        let audio_url = urls.get(1).cloned();
        let mode = if audio_url.is_some() {
            "native-separate-streams"
        } else {
            "native-progressive-stream"
        };

        Ok(PreviewStreamResult {
            url: video_url.clone(),
            urls: vec![video_url],
            audio_url,
            mode: mode.to_string(),
            format: selector,
        })
    })
    .await
    .map_err(|err| ApiError::new("thread_error", format!("Ön izleme thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn download_video(
    app: tauri::AppHandle,
    url: String,
    format_id: String,
    kind: String,
    quality: String,
    output_dir: Option<String>,
    fast_mode: bool,
    title: Option<String>,
    clip_start_seconds: Option<f64>,
    clip_end_seconds: Option<f64>,
) -> ApiResult<DownloadResult> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_video_blocking(
            app,
            url,
            format_id,
            kind,
            quality,
            output_dir,
            fast_mode,
            title,
            clip_start_seconds,
            clip_end_seconds,
        )
    })
    .await
    .map_err(|err| ApiError::new("thread_error", format!("İndirme thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

#[allow(clippy::too_many_arguments)]
fn download_video_blocking(
    app: tauri::AppHandle,
    url: String,
    format_id: String,
    kind: String,
    quality: String,
    output_dir: Option<String>,
    fast_mode: bool,
    title: Option<String>,
    clip_start_seconds: Option<f64>,
    clip_end_seconds: Option<f64>,
) -> Result<DownloadResult, String> {
    let job = begin_download_job()?;
    download_video_blocking_with_job(
        app,
        url,
        format_id,
        kind,
        quality,
        output_dir,
        fast_mode,
        title,
        clip_start_seconds,
        clip_end_seconds,
        true,
        job,
    )
}

#[allow(clippy::too_many_arguments)]
fn download_video_blocking_with_job(
    app: tauri::AppHandle,
    url: String,
    format_id: String,
    kind: String,
    quality: String,
    output_dir: Option<String>,
    fast_mode: bool,
    title: Option<String>,
    clip_start_seconds: Option<f64>,
    clip_end_seconds: Option<f64>,
    report_errors: bool,
    _job: DownloadJobGuard,
) -> Result<DownloadResult, String> {
    let clean_url = url.trim();
    let clean_format_id = format_id.trim();
    let clean_kind = kind.trim();
    if clean_url.is_empty() {
        return Err("Boş link gönderildi.".to_string());
    }

    if !is_supported_media_url(clean_url) {
        return Err(unsupported_media_link_message().to_string());
    }

    if clean_format_id.is_empty() {
        return Err("Format seçilmedi.".to_string());
    }

    let is_twitter = clean_kind == "twitter" || is_twitter_url(clean_url);
    let is_instagram = clean_kind == "instagram" || is_instagram_url(clean_url);
    let is_tiktok = clean_kind == "tiktok" || is_tiktok_url(clean_url);
    let is_youtube = is_youtube_url(clean_url);

    let clip_range =
        normalize_clip_range(clip_start_seconds, clip_end_seconds, is_youtube, clean_kind)?;

    let platform = platform_from_kind_or_url(clean_kind, clean_url).to_string();

    let tools = match ensure_runtime_tools(&app) {
        Ok(tools) => tools,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind(clean_kind)
                .format_id(clean_format_id)
                .quality(&quality)
                .fast_mode(fast_mode)
                .clip_range(clip_start_seconds, clip_end_seconds);
            return Err(with_optional_error_report(
                &app,
                report_errors,
                err.clone(),
                ctx,
            ));
        }
    };

    let download_dir = match resolve_download_dir(&app, output_dir.as_deref()) {
        Ok(dir) => dir,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind(clean_kind)
                .format_id(clean_format_id)
                .quality(&quality)
                .fast_mode(fast_mode)
                .clip_range(clip_start_seconds, clip_end_seconds);
            return Err(with_optional_error_report(
                &app,
                report_errors,
                err.clone(),
                ctx,
            ));
        }
    };

    let file_kind = if is_twitter {
        "twitter"
    } else if is_instagram {
        "instagram"
    } else if is_tiktok {
        "tiktok"
    } else {
        clean_kind
    };

    let final_output_base = pretty_output_base(file_kind, title.as_deref(), &quality, clip_range);
    let output_template = format!(
        "{}.%(ext)s",
        temp_output_stem(clean_kind, clean_url, clean_format_id, &quality, clip_range)
    );

    emit_simple_progress(&app, Some(0.0), "İndirme hazırlanıyor...");

    let started_at_ms = now_ms();

    if let Some(range) = clip_range {
        if true_quality_clip_requested(&quality) {
            match run_true_quality_clip_pipeline(
                &app,
                &tools,
                &download_dir,
                clean_url,
                clean_format_id,
                &quality,
                title.as_deref(),
                range,
                started_at_ms,
            ) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    if error == PAUSED_SIGNAL || error == CANCELLED_SIGNAL {
                        return Err(error);
                    }

                    let report_details = format!(
                        "--- True Quality DASH byte-range clip ---\n{}\n\nNot: 2K/4K seçildiği için otomatik 1080p HLS fallback çalıştırılmadı. 1080p hızlı klip isteniyorsa kaliteyi 1080p seçerek tekrar indir.",
                        sanitize_report_text(&error)
                    );

                    let ctx = ErrorReportContext::new("download", clean_url, &report_details)
                        .platform(platform)
                        .kind(clean_kind)
                        .format_id(clean_format_id)
                        .quality(&quality)
                        .fast_mode(fast_mode)
                        .clip_range(clip_start_seconds, clip_end_seconds)
                        .output_dir(download_dir);

                    let reported_error = with_optional_error_report(
                        &app,
                        report_errors,
                        "4K/2K True Quality klip indirilemedi. 1080p hızlı moda otomatik düşülmedi; hata raporu oluşturuldu.".to_string(),
                        ctx,
                    );

                    return Err(hls_fallback_error("true_quality_failed", reported_error));
                }
            }
        }

        let hls_quality_label = youtube_hls_clip_quality_label(&quality);
        let clip_output_template = format!(
            "{}-hls.%(ext)s",
            temp_output_stem(
                clean_kind,
                clean_url,
                clean_format_id,
                &hls_quality_label,
                Some(range)
            )
        );
        let clip_final_output_base =
            pretty_output_base(file_kind, title.as_deref(), &hls_quality_label, Some(range));

        match run_ytdlp_clip_fallback(
            &app,
            &tools,
            &download_dir,
            &clip_output_template,
            &clip_final_output_base,
            clean_kind,
            clean_url,
            clean_format_id,
            &quality,
            fast_mode,
            range,
            started_at_ms,
        ) {
            Ok(result) => return Ok(result),
            Err(error) => {
                if error == PAUSED_SIGNAL || error == CANCELLED_SIGNAL {
                    return Err(error);
                }

                let report_details = format!(
                    "--- HLS segment clip ---\n{}\n\nNot: Tam video indirip local kesme otomatik fallback olarak çalıştırılmadı; klip modu yalnızca seçilen zaman aralığına denk gelen HLS segmentlerini indirmeyi dener.",
                    sanitize_report_text(&error)
                );

                let ctx = ErrorReportContext::new("download", clean_url, &report_details)
                    .platform(platform)
                    .kind(clean_kind)
                    .format_id(clean_format_id)
                    .quality(&quality)
                    .fast_mode(fast_mode)
                    .clip_range(clip_start_seconds, clip_end_seconds)
                    .output_dir(download_dir);

                return Err(with_optional_error_report(
                    &app,
                    report_errors,
                    "Klip indirilemedi. Hata raporu oluşturuldu.".to_string(),
                    ctx,
                ));
            }
        }
    }

    let attempts = if is_youtube {
        make_youtube_attempts(
            clean_kind,
            clean_format_id,
            &quality,
            fast_mode,
            clip_range.is_some(),
        )
    } else {
        make_generic_attempts(
            clean_kind,
            clean_format_id,
            is_twitter,
            is_instagram,
            is_tiktok,
            fast_mode,
        )
    };

    let mut errors: Vec<(String, String)> = Vec::new();

    for attempt in attempts {
        if attempt.only_when_ssl_error {
            let combined_errors = errors
                .iter()
                .map(|(label, error)| format!("--- {} ---\n{}", label, error.trim()))
                .collect::<Vec<_>>()
                .join("\n\n");

            if !is_ssl_or_network_error(&combined_errors) {
                continue;
            }

            emit_simple_progress(&app, Some(0.0), "YouTube ağ kurtarma modu deneniyor...");
        }

        match run_download_attempt(
            &app,
            &tools,
            &download_dir,
            &output_template,
            clean_kind,
            clean_url,
            &attempt,
            started_at_ms,
            clip_range,
        ) {
            Ok(()) => {
                let mode_note = match attempt.external_downloader {
                    ExternalDownloader::Aria2c if !attempt.only_when_ssl_error => "Hızlı mod",
                    ExternalDownloader::Aria2c | ExternalDownloader::Curl => "Ağ kurtarma modu",
                    ExternalDownloader::Native if attempt.only_when_ssl_error => "Ağ kurtarma modu",
                    ExternalDownloader::Native => "Stabil mod",
                };

                let Some(completed_file) = take_ytdlp_final_output(&download_dir) else {
                    errors.push((
                        attempt.label.clone(),
                        "yt-dlp final dosya yolunu bildirmedi.".to_string(),
                    ));
                    continue;
                };

                if is_youtube && clean_kind != "audio" {
                    emit_simple_progress(&app, None, "İndirilen video doğrulanıyor...");
                    if let Err(error) = validate_video_output(
                        &tools,
                        &completed_file,
                        quality_height_limit(&quality),
                        None,
                    ) {
                        let _ = fs::remove_file(&completed_file);
                        errors.push((
                            format!("{} / çıktı doğrulama", attempt.label),
                            error,
                        ));
                        emit_simple_progress(
                            &app,
                            Some(0.0),
                            "İndirilen video doğrulanamadı. Sonraki mod deneniyor...",
                        );
                        continue;
                    }
                }

                let completed_file =
                    finalize_pretty_output_file(&completed_file, &download_dir, &final_output_base);
                let file_size = file_size(&completed_file).unwrap_or(0);
                let file_path = completed_file.to_string_lossy().to_string();

                let message = if file_path.is_empty() {
                    format!(
                        "İndirme tamamlandı. {} kullanıldı. Klasör: {}",
                        mode_note,
                        download_dir.to_string_lossy()
                    )
                } else {
                    format!(
                        "İndirme tamamlandı. {} kullanıldı. Dosya: {}",
                        mode_note, file_path
                    )
                };

                emit_simple_progress(&app, Some(100.0), "İndirme tamamlandı.");

                return Ok(DownloadResult {
                    message,
                    file_path,
                    output_dir: download_dir.to_string_lossy().to_string(),
                    mode: mode_note.to_string(),
                    file_size,
                });
            }
            Err(error) => {
                if error == PAUSED_SIGNAL || error == CANCELLED_SIGNAL {
                    return Err(error);
                }

                errors.push((attempt.label.clone(), error));

                emit_simple_progress(
                    &app,
                    Some(0.0),
                    "Bu mod başarısız oldu. Sonraki mod deneniyor...",
                );
            }
        }
    }

    let friendly_error = user_friendly_download_error(is_youtube, &errors);
    let report_details = errors
        .iter()
        .map(|(label, error)| format!("--- {} ---\n{}", label, error.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");

    if let Some(friendly) = friendly_media_access_error(&platform, &report_details) {
        return Err(friendly);
    }

    let ctx = ErrorReportContext::new("download", clean_url, &report_details)
        .platform(platform)
        .kind(clean_kind)
        .format_id(clean_format_id)
        .quality(&quality)
        .fast_mode(fast_mode)
        .clip_range(clip_start_seconds, clip_end_seconds)
        .output_dir(download_dir);

    Err(with_optional_error_report(
        &app,
        report_errors,
        friendly_error,
        ctx,
    ))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn download_twitter_post(
    app: tauri::AppHandle,
    url: String,
    format_id: String,
    quality: String,
    output_dir: Option<String>,
    title: Option<String>,
    post_text: Option<String>,
    author_name: Option<String>,
    author_handle: Option<String>,
    display_date: Option<String>,
    webpage_url: Option<String>,
    card_png_base64: String,
    card_overlay_png_base64: Option<String>,
    card_layout: TwitterPostCardLayout,
    analysis_id: Option<String>,
    item_id: Option<String>,
) -> ApiResult<DownloadResult> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_twitter_post_blocking(
            app,
            url,
            format_id,
            quality,
            output_dir,
            title,
            post_text,
            author_name,
            author_handle,
            display_date,
            webpage_url,
            card_png_base64,
            card_overlay_png_base64,
            card_layout,
            analysis_id,
            item_id,
            None,
        )
    })
    .await
    .map_err(|err| ApiError::new("thread_error", format!("Gönderi indirme thread hatası: {err}")))?;
    result.map_err(ApiError::from)
}

#[allow(clippy::too_many_arguments)]
fn download_twitter_post_blocking(
    app: tauri::AppHandle,
    url: String,
    format_id: String,
    quality: String,
    output_dir: Option<String>,
    title: Option<String>,
    post_text: Option<String>,
    author_name: Option<String>,
    author_handle: Option<String>,
    display_date: Option<String>,
    webpage_url: Option<String>,
    card_png_base64: String,
    card_overlay_png_base64: Option<String>,
    card_layout: TwitterPostCardLayout,
    analysis_id: Option<String>,
    item_id: Option<String>,
    job: Option<DownloadJobGuard>,
) -> Result<DownloadResult, String> {
    let clean_url = url.trim();
    let clean_format_id = format_id.trim();
    let clean_quality = quality.trim();
    let _job = match job {
        Some(job) => job,
        None => begin_download_job()?,
    };

    if clean_url.is_empty() {
        return Err("Boş link gönderildi.".to_string());
    }

    if !is_twitter_url(clean_url) {
        return Err(
            "Gönderi indirme modu sadece X/Twitter linkleri için kullanılabilir.".to_string(),
        );
    }

    if clean_format_id.is_empty() {
        return Err("Format seçilmedi.".to_string());
    }

    let stop_generation = current_download_stop_generation();
    let platform = platform_from_kind_or_url("twitter", clean_url).to_string();
    let card_layout = match validate_twitter_post_card_layout(card_layout) {
        Ok(layout) => layout,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4");
            return Err(with_error_report(&app, err, ctx));
        }
    };
    let post_text = clean_optional_text(post_text);
    let _author_name = clean_optional_text(author_name);
    let _author_handle = clean_optional_text(author_handle);
    let _display_date = clean_optional_text(display_date);
    let title_text = clean_optional_text(title);
    let _ = clean_optional_text(webpage_url);
    let post_text_for_output = if post_text.is_empty() {
        "Gönderi metni alınamadı.".to_string()
    } else {
        post_text
    };
    let title_for_output = if title_text.is_empty() {
        post_text_for_output.clone()
    } else {
        title_text
    };

    let tools = match ensure_runtime_tools(&app) {
        Ok(tools) => tools,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4");
            return Err(with_error_report(&app, err.clone(), ctx));
        }
    };

    let download_dir = match resolve_download_dir(&app, output_dir.as_deref()) {
        Ok(dir) => dir,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4");
            return Err(with_error_report(&app, err.clone(), ctx));
        }
    };

    check_download_stop_since(stop_generation)?;

    let (temp_root, temp_dir) = match unique_twitter_post_temp_dir() {
        Ok(value) => value,
        Err(err) => {
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4")
                .output_dir(download_dir);
            return Err(with_error_report(&app, err.clone(), ctx));
        }
    };

    let card_path = match write_twitter_post_card_png(&temp_dir, &card_png_base64) {
        Ok(path) => path,
        Err(err) => {
            remove_twitter_post_temp_dir(&temp_dir, &temp_root);
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4")
                .output_dir(download_dir);
            return Err(with_error_report(&app, err, ctx));
        }
    };

    let overlay_path =
        match write_twitter_post_overlay_png(&temp_dir, card_overlay_png_base64.as_deref()) {
            Ok(path) => path,
            Err(err) => {
                remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                let ctx = ErrorReportContext::new("download", clean_url, &err)
                    .platform(platform.clone())
                    .kind("twitter")
                    .format_id(clean_format_id)
                    .quality(clean_quality)
                    .fast_mode(false)
                    .output_mode("twitter_post_mp4")
                    .output_dir(download_dir);
                return Err(with_error_report(&app, err, ctx));
            }
        };

    check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;

    emit_simple_progress(&app, Some(0.0), "Gönderi videosu indiriliyor...");

    let mut registered_video_path = match materialize_twitter_post_registry_video(
        &app,
        &temp_dir,
        clean_url,
        analysis_id.as_deref(),
        item_id.as_deref(),
    ) {
        Ok(path) => path,
        Err(err) => {
            remove_twitter_post_temp_dir(&temp_dir, &temp_root);
            let ctx = ErrorReportContext::new("download", clean_url, &err)
                .platform(platform.clone())
                .kind("twitter")
                .format_id(clean_format_id)
                .quality(clean_quality)
                .fast_mode(false)
                .output_mode("twitter_post_mp4")
                .output_dir(download_dir);
            return Err(with_error_report(&app, err, ctx));
        }
    };

    let started_at_ms = now_ms();
    let output_template = format!(
        "{}.%(ext)s",
        temp_output_stem(
            "twitter-post",
            clean_url,
            clean_format_id,
            clean_quality,
            None
        )
    );
    let mut attempts = make_generic_attempts("twitter", clean_format_id, true, false, false, false);
    if registered_video_path.is_some() {
        attempts.truncate(1);
    }
    let mut errors: Vec<(String, String)> = Vec::new();

    for attempt in attempts {
        check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;

        let completed_file = if let Some(path) = registered_video_path.take() {
            Ok(path)
        } else {
            run_download_attempt_with_progress_mode(
                &app,
                &tools,
                &temp_dir,
                &output_template,
                "twitter",
                clean_url,
                &attempt,
                started_at_ms,
                None,
                DownloadProgressMode::TwitterPostMp4Download,
                Some(stop_generation),
            )
            .and_then(|_| {
                take_ytdlp_final_output(&temp_dir).ok_or_else(|| {
                    "Gönderi videosu indirildi ancak final video dosyası bulunamadı.".to_string()
                })
            })
        };

        match completed_file {
            Ok(completed_file) => {
                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;
                emit_simple_progress(&app, Some(80.0), "Gönderi videosu indiriliyor...");

                let video_path = finalize_twitter_post_source_video(&completed_file, &temp_dir);

                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;
                emit_simple_progress(&app, Some(81.0), "Gönderi kartı hazırlanıyor...");

                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;
                let duration = match media_duration_seconds(&tools, &video_path) {
                    Ok(value) => value,
                    Err(err) => {
                        remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                        let ctx = ErrorReportContext::new("download", clean_url, &err)
                            .platform(platform.clone())
                            .kind("twitter")
                            .format_id(clean_format_id)
                            .quality(clean_quality)
                            .fast_mode(false)
                            .output_mode("twitter_post_mp4")
                            .output_dir(download_dir);
                        return Err(with_error_report(&app, err, ctx));
                    }
                };

                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, None)?;
                emit_simple_progress(&app, Some(84.0), "Gönderi kartı hazırlanıyor...");

                let temp_output = temp_dir.join("twitter-post-output.mp4");
                let compose_command = build_ffmpeg_twitter_post_compose_command(
                    &tools,
                    &card_path,
                    &video_path,
                    overlay_path.as_deref(),
                    &temp_output,
                    card_layout,
                    duration,
                );

                check_twitter_post_stop(
                    stop_generation,
                    &temp_dir,
                    &temp_root,
                    Some(&temp_output),
                )?;
                emit_simple_progress(&app, Some(85.0), "MP4 oluşturuluyor...");

                if let Err(err) = run_ffmpeg_twitter_post_compose_process(
                    &app,
                    compose_command,
                    &temp_dir,
                    &temp_output,
                    now_ms(),
                    duration,
                    stop_generation,
                ) {
                    remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                    if err == PAUSED_SIGNAL || err == CANCELLED_SIGNAL {
                        return Err(err);
                    }

                    let report_error = format!(
                        "backend_compose_failed: Gönderi MP4 çıktısı oluşturulamadı.\n\n{}",
                        err
                    );
                    let ctx = ErrorReportContext::new("download", clean_url, &report_error)
                        .platform(platform.clone())
                        .kind("twitter")
                        .format_id(clean_format_id)
                        .quality(clean_quality)
                        .fast_mode(false)
                        .output_mode("twitter_post_mp4")
                        .output_dir(download_dir);
                    return Err(with_error_report(
                        &app,
                        "backend_compose_failed: Gönderi MP4 çıktısı oluşturulamadı.".to_string(),
                        ctx,
                    ));
                }

                check_twitter_post_stop(
                    stop_generation,
                    &temp_dir,
                    &temp_root,
                    Some(&temp_output),
                )?;

                let output_base =
                    twitter_post_output_base(Some(&title_for_output), &post_text_for_output);
                let final_path = unique_pretty_output_path(&download_dir, &output_base, "mp4");

                check_twitter_post_stop(
                    stop_generation,
                    &temp_dir,
                    &temp_root,
                    Some(&temp_output),
                )?;

                if let Err(err) = fs::copy(&temp_output, &final_path) {
                    let _ = fs::remove_file(&final_path);
                    remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                    let message = format!("Gönderi MP4 dosyası final konuma yazılamadı: {}", err);
                    let ctx = ErrorReportContext::new("download", clean_url, &message)
                        .platform(platform.clone())
                        .kind("twitter")
                        .format_id(clean_format_id)
                        .quality(clean_quality)
                        .fast_mode(false)
                        .output_mode("twitter_post_mp4")
                        .output_dir(download_dir);
                    return Err(with_error_report(&app, message, ctx));
                }

                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, Some(&final_path))?;

                remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, Some(&final_path))?;

                emit_simple_progress(&app, Some(100.0), "Tamamlandı.");

                let file_size = file_size(&final_path).unwrap_or(0);
                let message = format!(
                    "X/Twitter gönderi videosu hazırlandı. Dosya: {}",
                    final_path.to_string_lossy()
                );

                check_twitter_post_stop(stop_generation, &temp_dir, &temp_root, Some(&final_path))?;

                return Ok(DownloadResult {
                    message,
                    file_path: final_path.to_string_lossy().to_string(),
                    output_dir: download_dir.to_string_lossy().to_string(),
                    mode: "twitter_post_mp4".to_string(),
                    file_size,
                });
            }
            Err(error) => {
                if error == PAUSED_SIGNAL || error == CANCELLED_SIGNAL {
                    remove_twitter_post_temp_dir(&temp_dir, &temp_root);
                    return Err(error);
                }

                errors.push((attempt.label.clone(), error));

                emit_simple_progress(&app, None, "Gönderi videosu indiriliyor...");
            }
        }
    }

    let report_details = errors
        .iter()
        .map(|(label, error)| format!("--- {} ---\n{}", label, error.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");

    remove_twitter_post_temp_dir(&temp_dir, &temp_root);

    if let Some(friendly) = friendly_media_access_error(&platform, &report_details) {
        return Err(friendly);
    }

    let friendly_error = user_friendly_download_error(false, &errors);
    let ctx = ErrorReportContext::new("download", clean_url, &report_details)
        .platform(platform)
        .kind("twitter")
        .format_id(clean_format_id)
        .quality(clean_quality)
        .fast_mode(false)
        .output_mode("twitter_post_mp4")
        .output_dir(download_dir);

    Err(with_error_report(&app, friendly_error, ctx))
}

#[cfg(test)]
mod tests;
