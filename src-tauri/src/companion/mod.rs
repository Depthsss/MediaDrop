pub(crate) mod protocol;
pub(crate) mod state;
#[cfg(target_os = "windows")]
pub(crate) mod windows_pipe;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Emitter;

use crate::core::error::{ApiError, STRUCTURED_ERROR_PREFIX};
use crate::core::media::{AuthorIdentity, MediaAnalysis, TwitterPostMetadata};
use crate::media_cache::preview::prepare_companion_media_preview_blocking;
use crate::media_cache::registry::MEDIA_ANALYSIS_TTL_MS;
use crate::util::url::{is_instagram_url, is_tiktok_url, is_twitter_url};
use crate::{
    begin_download_job_owned, collect_media, current_download_job_id, current_download_job_stop,
    download_job_result_target_for, download_job_snapshot_for,
    download_media_batch_blocking_with_job, download_media_item_blocking_with_job,
    download_media_post_card_blocking_with_job, download_twitter_post_blocking,
    download_video_blocking_with_job, ensure_download_job_history_owner, ensure_download_job_owner,
    record_download_job_operation, record_download_job_result, record_download_job_terminal,
    queue_extension_setup_request, register_media_analysis, run_ytdlp_json_analysis,
    show_main_window,
    update_download_job_progress, DownloadJobStop, DownloadResultKind, TwitterPostCardLayout,
};
use protocol::{
    hello_requires_extension_refresh, hello_response, media_display_title, normalize_page_url,
    parse_request, project_media_analysis, project_ytdlp_analysis, validate_source_payload, Command,
    RequestEnvelope, ResponseEnvelope, SourcePayload, MAX_RESPONSE_BYTES, PROTOCOL_VERSION,
};
use state::{
    AnalysisSnapshot, AnalysisStatus, CompanionStore, DownloadPlan, InsertOutcome, StoredAnalysis,
};

static COMPANION_STORE: OnceLock<Mutex<CompanionStore>> = OnceLock::new();
static PENDING_HANDOFF: OnceLock<Mutex<Option<Value>>> = OnceLock::new();
static RENDERER_READY: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();
static RENDER_TASKS: OnceLock<Mutex<HashMap<String, mpsc::SyncSender<Value>>>> = OnceLock::new();
static EXTENSION_CONNECTED: AtomicBool = AtomicBool::new(false);

const RENDERER_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RENDER_RESULT_BYTES: usize = 12 * 1024 * 1024;

pub(crate) fn companion_extension_connected() -> bool {
    EXTENSION_CONNECTED.load(Ordering::Acquire)
}

fn store() -> &'static Mutex<CompanionStore> {
    COMPANION_STORE.get_or_init(|| Mutex::new(CompanionStore::default()))
}

fn pending_handoff() -> &'static Mutex<Option<Value>> {
    PENDING_HANDOFF.get_or_init(|| Mutex::new(None))
}

fn renderer_ready() -> &'static (Mutex<bool>, Condvar) {
    RENDERER_READY.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

fn render_tasks() -> &'static Mutex<HashMap<String, mpsc::SyncSender<Value>>> {
    RENDER_TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
pub(crate) fn companion_renderer_ready() -> Result<(), ApiError> {
    let (ready, signal) = renderer_ready();
    *ready
        .lock()
        .map_err(|_| ApiError::new("renderer_unavailable", "Renderer durumu kullanılamıyor."))? =
        true;
    signal.notify_all();
    Ok(())
}

#[tauri::command]
pub(crate) fn complete_companion_render(task_id: String, result: Value) -> Result<(), ApiError> {
    if uuid::Uuid::parse_str(task_id.trim()).is_err()
        || serde_json::to_vec(&result)
            .map(|bytes| bytes.len() > MAX_RENDER_RESULT_BYTES)
            .unwrap_or(true)
    {
        return Err(ApiError::new(
            "renderer_result_invalid",
            "Renderer sonucu geçersiz veya çok büyük.",
        ));
    }
    let sender = render_tasks()
        .lock()
        .map_err(|_| ApiError::new("renderer_unavailable", "Renderer durumu kullanılamıyor."))?
        .remove(task_id.trim())
        .ok_or_else(|| {
            ApiError::new(
                "renderer_result_invalid",
                "Renderer görevi artık etkin değil.",
            )
        })?;
    sender
        .send(result)
        .map_err(|_| ApiError::new("renderer_result_invalid", "Renderer sonucu işlenemedi."))
}

fn request_companion_render(
    app: &tauri::AppHandle,
    kind: &str,
    payload: Value,
) -> Result<Value, ApiError> {
    let (ready, signal) = renderer_ready();
    let ready = ready
        .lock()
        .map_err(|_| ApiError::new("renderer_unavailable", "Renderer durumu kullanılamıyor."))?;
    let (ready, _) = signal
        .wait_timeout_while(ready, RENDERER_TIMEOUT, |ready| !*ready)
        .map_err(|_| ApiError::new("renderer_unavailable", "Renderer durumu kullanılamıyor."))?;
    if !*ready {
        return Err(ApiError::new(
            "renderer_timeout",
            "Gönderi renderer'ı zamanında hazır olmadı.",
        ));
    }
    drop(ready);

    let task_id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = mpsc::sync_channel(1);
    {
        let mut tasks = render_tasks().lock().map_err(|_| {
            ApiError::new("renderer_unavailable", "Renderer durumu kullanılamıyor.")
        })?;
        if tasks.len() >= 4 {
            return Err(ApiError::new(
                "renderer_unavailable",
                "Renderer şu anda kullanılamıyor.",
            ));
        }
        tasks.insert(task_id.clone(), sender);
    }
    if app
        .emit(
            "companion-render-request",
            json!({"taskId": task_id, "kind": kind, "payload": payload}),
        )
        .is_err()
    {
        render_tasks()
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(&task_id));
        return Err(ApiError::new(
            "renderer_unavailable",
            "Gönderi renderer'ına ulaşılamadı.",
        ));
    }
    match receiver.recv_timeout(RENDERER_TIMEOUT) {
        Ok(result) => Ok(result),
        Err(_) => {
            render_tasks()
                .lock()
                .ok()
                .and_then(|mut tasks| tasks.remove(&task_id));
            Err(ApiError::new(
                "renderer_timeout",
                "Gönderi renderer'ı zaman aşımına uğradı.",
            ))
        }
    }
}

fn set_pending_handoff(payload: Value) -> Result<(), ApiError> {
    *pending_handoff()
        .lock()
        .map_err(|_| ApiError::new("state_unavailable", "Companion handoff kullanılamıyor."))? =
        Some(payload);
    Ok(())
}

#[tauri::command]
pub(crate) fn take_companion_handoff() -> Result<Option<Value>, ApiError> {
    Ok(pending_handoff()
        .lock()
        .map_err(|_| ApiError::new("state_unavailable", "Companion handoff kullanılamıyor."))?
        .take())
}

fn time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn analysis_error(site: &str, raw: String) -> (AnalysisStatus, ApiError) {
    if let Some(encoded) = raw.strip_prefix(STRUCTURED_ERROR_PREFIX) {
        if let Ok(mut error) = serde_json::from_str::<ApiError>(encoded) {
            if site == "instagram"
                && matches!(
                    error.code.as_str(),
                    "instagram_auth_required"
                        | "instagram_auth_expired"
                        | "instagram_cookie_invalid"
                        | "instagram_browser_locked"
                )
            {
                error.message = "Instagram oturumunu yenilemek için masaüstü uygulamasında tarayıcı iznini tamamla.".to_string();
            }
            let needs_user = error.code.contains("auth")
                || error.code.contains("cookie")
                || error.code.contains("browser")
                || error.action.is_some();
            return (
                if needs_user {
                    AnalysisStatus::NeedsUser
                } else {
                    AnalysisStatus::Error
                },
                error,
            );
        }
    }
    if matches!(site, "instagram" | "twitter") && crate::gallery_error_indicates_auth_failure(&raw)
    {
        let (code, platform) = if site == "twitter" {
            ("twitter_auth_required", "X/Twitter")
        } else {
            ("instagram_auth_required", "Instagram")
        };
        return (
            AnalysisStatus::NeedsUser,
            ApiError::new(
                code,
                format!("{platform} oturumu gerekiyor. MediaDrop'u açıp tarayıcı iznini tamamla."),
            )
            .with_retryable(true)
            .with_action("open_advanced"),
        );
    }
    (
        AnalysisStatus::Error,
        ApiError::new(
            "analysis_failed",
            "Medya analizi tamamlanamadı. Ayrıntılar için masaüstü uygulamasını aç.",
        )
        .with_retryable(true)
        .with_action("open_app"),
    )
}

fn ytdlp_media<'a>(value: &'a Value, media_id: &str) -> Option<&'a Value> {
    value
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(media_id))
        })
        .or_else(|| (value.get("id").and_then(Value::as_str) == Some(media_id)).then_some(value))
}

pub(crate) fn build_download_plan(
    stored: &StoredAnalysis,
    media_id: &str,
    choice_id: &str,
) -> Result<DownloadPlan, ApiError> {
    let media_id = media_id.trim();
    let choice_id = choice_id.trim();
    if media_id.is_empty() || choice_id.is_empty() || choice_id.len() > 160 {
        return Err(ApiError::new(
            "invalid_request",
            "Medya veya kalite seçimi geçersiz.",
        ));
    }
    match stored {
        StoredAnalysis::Video { source_url, value } => {
            let media = ytdlp_media(value, media_id).ok_or_else(|| {
                ApiError::new("analysis_expired", "Seçili medya analizde bulunamadı.")
            })?;
            let formats = media
                .get("formats")
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::new("unsupported", "Seçilebilir format bulunamadı."))?;
            let (kind, requested) = choice_id
                .split_once(':')
                .ok_or_else(|| ApiError::new("invalid_request", "Kalite seçimi geçersiz."))?;
            let social_kind = if is_twitter_url(source_url) {
                Some("twitter")
            } else if is_instagram_url(source_url) {
                Some("instagram")
            } else if is_tiktok_url(source_url) {
                Some("tiktok")
            } else {
                None
            };
            let selected = formats
                .iter()
                .find(|format| format.get("format_id").and_then(Value::as_str) == Some(requested));
            let (format_id, quality) = match kind {
                "video" => {
                    let format = selected
                        .filter(|format| {
                            format.get("vcodec").and_then(Value::as_str) != Some("none")
                        })
                        .ok_or_else(|| {
                            ApiError::new("invalid_request", "Video formatı analizde bulunamadı.")
                        })?;
                    let height = format.get("height").and_then(Value::as_u64).unwrap_or(0);
                    (
                        requested.to_string(),
                        if height > 0 {
                            format!("{height}p")
                        } else {
                            "Best".to_string()
                        },
                    )
                }
                "audio" if requested == "best" => {
                    if !formats.iter().any(|format| {
                        format
                            .get("acodec")
                            .and_then(Value::as_str)
                            .is_some_and(|codec| codec != "none")
                    }) {
                        return Err(ApiError::new(
                            "unsupported",
                            "Bu medyada ses formatı bulunamadı.",
                        ));
                    }
                    ("bestaudio/best".to_string(), "MP3".to_string())
                }
                "audio" => {
                    selected
                        .filter(|format| {
                            format.get("vcodec").and_then(Value::as_str) == Some("none")
                                && format
                                    .get("acodec")
                                    .and_then(Value::as_str)
                                    .is_some_and(|codec| codec != "none")
                        })
                        .ok_or_else(|| {
                            ApiError::new("invalid_request", "Ses formatı analizde bulunamadı.")
                        })?;
                    (requested.to_string(), "MP3".to_string())
                }
                "social" if requested == "auto" && social_kind.is_some() => {
                    let has_video = formats.iter().any(|format| {
                        format
                            .get("vcodec")
                            .and_then(Value::as_str)
                            .is_some_and(|codec| codec != "none")
                    });
                    if !has_video {
                        return Err(ApiError::new(
                            "unsupported",
                            "Bu sosyal medya içeriğinde video formatı bulunamadı.",
                        ));
                    }
                    let height = formats
                        .iter()
                        .filter(|format| {
                            format
                                .get("vcodec")
                                .and_then(Value::as_str)
                                .is_some_and(|codec| codec != "none")
                        })
                        .filter_map(|format| format.get("height").and_then(Value::as_u64))
                        .max()
                        .unwrap_or(0);
                    (
                        "best[ext=mp4]/bestvideo+bestaudio/best".to_string(),
                        if height > 0 {
                            format!("{height}p")
                        } else {
                            "Best".to_string()
                        },
                    )
                }
                _ => return Err(ApiError::new("invalid_request", "Kalite seçimi geçersiz.")),
            };
            let plan_kind = if kind == "audio" {
                "audio"
            } else if kind == "social" {
                social_kind.unwrap_or("video")
            } else {
                "video"
            };
            Ok(DownloadPlan {
                operation_kind: if kind == "audio" { "audio" } else { "video" }.to_string(),
                source_url: source_url.clone(),
                format_id,
                kind: plan_kind.to_string(),
                quality,
                title: media
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|value| value.chars().take(512).collect()),
                clip_start_seconds: None,
                clip_end_seconds: None,
                registry_item: None,
                registry_batch: None,
            })
        }
        StoredAnalysis::Media { source_url, value } => {
            let item = value
                .items
                .iter()
                .find(|item| item.id == media_id)
                .ok_or_else(|| {
                    ApiError::new("analysis_expired", "Seçili medya analizde bulunamadı.")
                })?;
            let (format_id, kind, quality, registry_item) = match choice_id {
                "social:auto" => (
                    "best[ext=mp4]/bestvideo+bestaudio/best".to_string(),
                    value.platform.clone(),
                    item.height
                        .map(|height| format!("{height}p"))
                        .unwrap_or_else(|| "Otomatik".to_string()),
                    Some((value.analysis_id.clone(), item.id.clone())),
                ),
                "audio:best" if item.has_audio && value.items.len() == 1 => (
                    "bestaudio/best".to_string(),
                    "audio".to_string(),
                    "MP3".to_string(),
                    None,
                ),
                _ => {
                    return Err(ApiError::new(
                        "invalid_request",
                        "Sosyal medya kalite seçimi geçersiz.",
                    ))
                }
            };
            Ok(DownloadPlan {
                operation_kind: if registry_item.is_some() {
                    "media_item"
                } else {
                    "audio"
                }
                .to_string(),
                source_url: source_url.clone(),
                format_id,
                kind,
                quality,
                title: Some(media_display_title(value, item)),
                clip_start_seconds: None,
                clip_end_seconds: None,
                registry_item,
                registry_batch: None,
            })
        }
    }
}

fn validate_companion_clip(
    stored: &StoredAnalysis,
    media_id: &str,
    plan: &DownloadPlan,
    clip: Option<ClipPayload>,
) -> Result<Option<(f64, f64)>, ApiError> {
    let Some(clip) = clip else {
        return Ok(None);
    };
    let StoredAnalysis::Video { source_url, value } = stored else {
        return Err(ApiError::new(
            "clip_range_invalid",
            "Hızlı klip yalnız YouTube videolarında kullanılabilir.",
        ));
    };
    if !crate::is_youtube_url(source_url) || plan.kind != "video" {
        return Err(ApiError::new(
            "clip_range_invalid",
            "Hızlı klip yalnız YouTube video kalitesinde kullanılabilir.",
        ));
    }
    if !clip.start_seconds.is_finite()
        || !clip.end_seconds.is_finite()
        || clip.start_seconds < 0.0
        || clip.end_seconds - clip.start_seconds < 1.0
    {
        return Err(ApiError::new(
            "clip_range_invalid",
            "Klip başlangıç ve bitiş aralığı geçersiz.",
        ));
    }
    let duration = ytdlp_media(value, media_id)
        .and_then(|media| media.get("duration"))
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    if duration.is_some_and(|duration| clip.end_seconds > duration) {
        return Err(ApiError::new(
            "clip_range_invalid",
            "Klip bitişi video süresini aşamaz.",
        ));
    }
    Ok(Some((clip.start_seconds, clip.end_seconds)))
}

fn command_name(command: Command) -> &'static str {
    match command {
        Command::Hello => "hello",
        Command::AnalyzeSource => "analyze_source",
        Command::GetState => "get_state",
        Command::GetMediaPreview => "get_media_preview",
        Command::OpenApp => "open_app",
        Command::OpenAdvanced => "open_advanced",
        Command::OpenDownloads => "open_downloads",
        Command::RevealResult => "reveal_result",
        Command::RevealErrorReport => "reveal_error_report",
        Command::StartDownload => "start_download",
        Command::StartMediaBatch => "start_media_batch",
        Command::StartPostExport => "start_post_export",
        Command::PauseDownload => "pause_download",
        Command::ResumeDownload => "resume_download",
        Command::CancelDownload => "cancel_download",
    }
}

fn response_bytes(response: &ResponseEnvelope) -> Vec<u8> {
    let bytes = serde_json::to_vec(response).unwrap_or_else(|_| {
        br#"{"messageType":"response","protocolVersion":1,"requestId":"00000000-0000-4000-8000-000000000000","command":"unknown","status":"error","stateRevision":0,"payload":{},"capabilities":{},"error":{"code":"serialization_error","message":"Companion response could not be serialized.","retryable":false,"action":null,"reportId":null}}"#.to_vec()
    });
    if bytes.len() <= MAX_RESPONSE_BYTES {
        return bytes;
    }
    serde_json::to_vec(&json!({
        "messageType":"response",
        "protocolVersion":PROTOCOL_VERSION,
        "requestId":response.request_id,
        "command":response.command,
        "status":"error",
        "stateRevision":0,
        "payload":{},
        "capabilities":{},
        "error":{
            "code":"message_too_large",
            "message":"Companion response is too large.",
            "retryable":false,
            "action":null,
            "reportId":null
        }
    }))
    .unwrap_or_else(|_| br#"{"messageType":"response","protocolVersion":1,"requestId":"00000000-0000-4000-8000-000000000000","command":"unknown","status":"error","stateRevision":0,"payload":{},"capabilities":{},"error":{"code":"serialization_error","message":"Companion response could not be serialized.","retryable":false,"action":null,"reportId":null}}"#.to_vec())
}

fn unbound_error_response(bytes: &[u8], error: ApiError) -> Vec<u8> {
    let value = serde_json::from_slice::<Value>(bytes).unwrap_or_else(|_| json!({}));
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .unwrap_or("00000000-0000-4000-8000-000000000000");
    let command = value
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    serde_json::to_vec(&json!({
        "messageType": "response",
        "protocolVersion": PROTOCOL_VERSION,
        "requestId": request_id,
        "command": command,
        "status": if error.code == "version_mismatch" { "version_mismatch" } else { "error" },
        "stateRevision": 0,
        "payload": {},
        "capabilities": protocol::Capabilities::current(),
        "error": error
    }))
    .unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetStatePayload {
    #[serde(default)]
    analysis_request_id: Option<String>,
    #[serde(default)]
    page_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetMediaPreviewPayload {
    analysis_request_id: String,
    media_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaPreviewRenderResult {
    ok: bool,
    #[serde(default)]
    data_url: String,
    #[serde(default)]
    duration_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum AdvancedIntent {
    DownloadTwitterPost,
}

impl AdvancedIntent {
    fn as_str(self) -> &'static str {
        match self {
            Self::DownloadTwitterPost => "download_twitter_post",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdvancedPayload {
    #[serde(default)]
    analysis_request_id: Option<String>,
    #[serde(default)]
    intent: Option<AdvancedIntent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartDownloadPayload {
    analysis_request_id: String,
    media_id: String,
    choice_id: String,
    #[serde(default)]
    clip: Option<ClipPayload>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClipPayload {
    start_seconds: f64,
    end_seconds: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartMediaBatchPayload {
    analysis_request_id: String,
    #[serde(default = "default_batch_scope")]
    scope: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartPostExportPayload {
    analysis_request_id: String,
    media_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PostExportRenderResult {
    ok: bool,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    card_png_base64: String,
    #[serde(default)]
    card_overlay_png_base64: String,
    #[serde(default)]
    card_layout: Option<TwitterPostCardLayout>,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    stage: String,
}

fn default_batch_scope() -> String {
    "all".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlPayload {
    analysis_request_id: String,
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    previous_job_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevealResultPayload {
    analysis_request_id: String,
    job_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevealErrorReportPayload {
    report_id: String,
}

fn status_name(status: AnalysisStatus) -> &'static str {
    match status {
        AnalysisStatus::Analyzing => "analyzing",
        AnalysisStatus::Ready => "ready",
        AnalysisStatus::NeedsUser => "needs_user",
        AnalysisStatus::Error => "error",
    }
}

fn snapshot_status(snapshot: &AnalysisSnapshot) -> &'static str {
    match snapshot.error.as_ref().map(|error| error.code.as_str()) {
        Some("download_busy" | "analysis_busy") => "busy",
        Some("media_not_found" | "unsupported_source" | "unsupported") => "unsupported",
        Some(code) if code.contains("auth") || code.contains("cookie") => "needs_user",
        _ => status_name(snapshot.status),
    }
}

fn snapshot_response(request: &RequestEnvelope, snapshot: AnalysisSnapshot) -> ResponseEnvelope {
    let base_status = snapshot_status(&snapshot);
    let mut payload = snapshot
        .projection
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "analysisRequestId".to_string(),
            Value::String(snapshot.analysis_request_id.clone()),
        );
        let job = download_job_snapshot_for(
            Some(&request.client_origin),
            Some(&snapshot.analysis_request_id),
        );
        object.insert(
            "activeJob".to_string(),
            job.as_ref()
                .and_then(|value| serde_json::to_value(value).ok())
                .unwrap_or(Value::Null),
        );
    }
    let job = download_job_snapshot_for(
        Some(&request.client_origin),
        Some(&snapshot.analysis_request_id),
    );
    let response_status = job.as_ref().map_or_else(
        || base_status,
        |job| {
            if job.owned_by_request {
                match job.status.as_str() {
                    "preparing" | "downloading" => "downloading",
                    "postprocessing" => "postprocessing",
                    "validating" => "validating",
                    "paused" => "paused",
                    "completed" => "completed",
                    "cancelled" => "cancelled",
                    "error" => "error",
                    _ => base_status,
                }
            } else if current_download_job_id().is_some() {
                "busy"
            } else {
                base_status
            }
        },
    );
    let mut response =
        ResponseEnvelope::success(request, response_status, snapshot.state_revision, payload);
    response.error = snapshot.error;
    response
}

fn insert_mutating_request(request: &RequestEnvelope) -> Result<InsertOutcome, ApiError> {
    store()
        .lock()
        .map(|mut state| {
            state.insert_command(
                &request.client_origin,
                &request.request_id,
                command_name(request.command),
                &request.payload,
                time_ms(),
            )
        })
        .map_err(|_| ApiError::new("state_unavailable", "Companion durumu kullanılamıyor."))
}

fn record_media_job_result(job_id: &str, outcome: &crate::MediaDownloadResult) {
    let single_file = outcome.files.len() == 1 && outcome.failed_count == 0;
    let target = if single_file {
        PathBuf::from(&outcome.files[0].file_path)
    } else {
        PathBuf::from(&outcome.output_dir)
    };
    let display_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("MediaDrop")
        .to_string();
    let size = outcome.files.iter().map(|file| file.file_size).sum::<u64>();
    record_download_job_result(
        job_id,
        target,
        if single_file {
            DownloadResultKind::File
        } else {
            DownloadResultKind::Directory
        },
        display_name,
        outcome.downloaded_count,
        outcome.failed_count,
        Some(size),
    );
}

fn run_download_plan(
    app: &tauri::AppHandle,
    request: &RequestEnvelope,
    analysis_request_id: &str,
    plan: DownloadPlan,
) -> ResponseEnvelope {
    let job = match begin_download_job_owned(&request.client_origin, analysis_request_id) {
        Ok(job) => job,
        Err(error) => return ResponseEnvelope::failure(request, ApiError::from_legacy(error)),
    };
    let job_id = job.id().to_string();
    record_download_job_operation(&job_id, &plan.operation_kind);
    if let Ok(mut state) = store().lock() {
        state.store_download_plan(&request.client_origin, analysis_request_id, plan.clone());
        state.set_download_error(&request.client_origin, analysis_request_id, None);
    }
    let app_handle = app.clone();
    let download_origin = request.client_origin.clone();
    let download_request_id = analysis_request_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let result = if let Some((analysis_id, scope)) = plan.registry_batch {
            download_media_batch_blocking_with_job(app_handle, analysis_id, scope, None, job)
                .map(|outcome| record_media_job_result(&job_id, &outcome))
        } else if let Some((analysis_id, item_id)) = plan.registry_item {
            download_media_item_blocking_with_job(app_handle, analysis_id, item_id, None, job)
                .map(|outcome| record_media_job_result(&job_id, &outcome))
        } else {
            download_video_blocking_with_job(
                app_handle,
                plan.source_url,
                plan.format_id,
                plan.kind,
                plan.quality,
                None,
                false,
                plan.title,
                plan.clip_start_seconds,
                plan.clip_end_seconds,
                false,
                job,
            )
            .map(|outcome| {
                let target = PathBuf::from(&outcome.file_path);
                let display_name = target
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("MediaDrop")
                    .to_string();
                record_download_job_result(
                    &job_id,
                    target,
                    DownloadResultKind::File,
                    display_name,
                    1,
                    0,
                    Some(outcome.file_size),
                );
            })
        };
        let terminal = match result.as_ref().err().map(String::as_str) {
            None => "completed",
            Some(crate::PAUSED_SIGNAL) => "paused",
            Some(crate::CANCELLED_SIGNAL) => "cancelled",
            Some(_) => "error",
        };
        let structured_error = result
            .as_ref()
            .err()
            .map(|error| ApiError::from_legacy(error.clone()))
            .filter(|error| error.code != "internal_error");
        record_download_job_terminal(&job_id, terminal);
        if terminal == "error" {
            if let Ok(mut state) = store().lock() {
                state.set_download_error(
                    &download_origin,
                    &download_request_id,
                    Some(structured_error.unwrap_or_else(|| {
                        ApiError::new("download_failed", "İndirme tamamlanamadı.")
                            .with_retryable(true)
                            .with_action("open_app")
                    })),
                );
            }
        }
    });
    response_for_analysis(request, analysis_request_id)
}

fn start_download(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload: StartDownloadPayload = match serde_json::from_value(request.payload.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "İndirme payload geçersiz."),
            )
        }
    };
    match insert_mutating_request(request) {
        Ok(InsertOutcome::Duplicate) => {
            return response_for_analysis(request, &payload.analysis_request_id)
        }
        Ok(InsertOutcome::Conflict) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "request_conflict",
                    "Aynı requestId farklı bir istekle kullanıldı.",
                ),
            )
        }
        Err(error) => return ResponseEnvelope::failure(request, error),
        Ok(InsertOutcome::Inserted) => {}
    }
    let stored = store().lock().ok().and_then(|state| {
        state.stored_analysis(&request.client_origin, &payload.analysis_request_id)
    });
    let Some(stored) = stored else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "analysis_expired",
                "İndirme analizi bulunamadı veya süresi doldu.",
            ),
        );
    };
    let mut plan = match build_download_plan(&stored, &payload.media_id, &payload.choice_id) {
        Ok(plan) => plan,
        Err(error) => return ResponseEnvelope::failure(request, error),
    };
    match validate_companion_clip(&stored, &payload.media_id, &plan, payload.clip) {
        Ok(Some((start, end))) => {
            plan.operation_kind = "youtube_clip".to_string();
            plan.clip_start_seconds = Some(start);
            plan.clip_end_seconds = Some(end);
        }
        Ok(None) => {}
        Err(error) => return ResponseEnvelope::failure(request, error),
    }
    run_download_plan(app, request, &payload.analysis_request_id, plan)
}

fn start_media_batch(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload: StartMediaBatchPayload =
        match serde_json::from_value::<StartMediaBatchPayload>(request.payload.clone()) {
            Ok(payload) if matches!(payload.scope.as_str(), "photos" | "all" | "all-stories") => {
                payload
            }
            _ => {
                return ResponseEnvelope::failure(
                    request,
                    ApiError::new("invalid_request", "Toplu indirme payload geçersiz."),
                )
            }
        };
    match insert_mutating_request(request) {
        Ok(InsertOutcome::Duplicate) => {
            return response_for_analysis(request, &payload.analysis_request_id)
        }
        Ok(InsertOutcome::Conflict) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "request_conflict",
                    "Aynı requestId farklı bir istekle kullanıldı.",
                ),
            )
        }
        Err(error) => return ResponseEnvelope::failure(request, error),
        Ok(InsertOutcome::Inserted) => {}
    }
    let stored = store().lock().ok().and_then(|state| {
        state.stored_analysis(&request.client_origin, &payload.analysis_request_id)
    });
    let Some(StoredAnalysis::Media { source_url, value }) = stored else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "analysis_expired",
                "Toplu indirilecek sosyal medya analizi bulunamadı.",
            ),
        );
    };
    if value.items.is_empty() || value.analysis_id.trim().is_empty() {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("unsupported", "Bu analizde toplu indirilebilir medya yok."),
        );
    }
    let plan = DownloadPlan {
        operation_kind: "media_batch".to_string(),
        source_url,
        format_id: "registry".to_string(),
        kind: value.platform,
        quality: payload.scope.clone(),
        title: Some(value.title),
        clip_start_seconds: None,
        clip_end_seconds: None,
        registry_item: None,
        registry_batch: Some((value.analysis_id, payload.scope)),
    };
    run_download_plan(app, request, &payload.analysis_request_id, plan)
}

fn renderer_report_details(result: &PostExportRenderResult) -> String {
    let code = result
        .error_code
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .take(64)
        .collect::<String>();
    let stage = result
        .stage
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .take(64)
        .collect::<String>();
    let layout = result.card_layout.as_ref().map_or_else(
        || "none".to_string(),
        |layout| format!("{}x{}", layout.output_width, layout.output_height),
    );
    format!(
        "Renderer code: {}\nRenderer stage: {}\nRenderer version: 1\nLayout: {}",
        if code.is_empty() { "unknown" } else { &code },
        if stage.is_empty() { "unknown" } else { &stage },
        layout,
    )
}

fn renderer_failure(app: &tauri::AppHandle, result: &PostExportRenderResult) -> ApiError {
    let code = match result.error_code.as_str() {
        "preview_failed" => "preview_failed",
        "renderer_timeout" => "renderer_timeout",
        "card_layout_invalid"
        | "card_png_render_failed"
        | "placeholder_render_failed"
        | "template_load_failed"
        | "template_contains_external_resource"
        | "template_missing_data_video_slot"
        | "tainted_canvas_detected" => "renderer_result_invalid",
        _ => "renderer_result_invalid",
    };
    let error = ApiError::new(code, "X/Twitter gönderi kartı oluşturulamadı.")
        .with_retryable(true)
        .with_action("retry");
    let context = crate::ErrorReportContext::new(
        "companion-renderer",
        "[redacted]",
        renderer_report_details(result),
    )
    .platform("twitter")
    .kind("post-card");
    crate::write_error_report(app, &context)
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .map_or(error.clone(), |report_id| error.with_report_id(report_id))
}

fn valid_report_id(value: &str) -> bool {
    value.len() <= 128
        && value.starts_with("mediadrop-companion-renderer-")
        && value.ends_with(".txt")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
}

fn renderer_transport_failure(app: &tauri::AppHandle, error: ApiError) -> ApiError {
    renderer_failure(
        app,
        &PostExportRenderResult {
            ok: false,
            mode: String::new(),
            card_png_base64: String::new(),
            card_overlay_png_base64: String::new(),
            card_layout: None,
            error_code: error.code,
            stage: "bridge".to_string(),
        },
    )
}

fn record_video_job_result(job_id: &str, outcome: &crate::DownloadResult) {
    let target = PathBuf::from(&outcome.file_path);
    let display_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("MediaDrop")
        .to_string();
    record_download_job_result(
        job_id,
        target,
        DownloadResultKind::File,
        display_name,
        1,
        0,
        Some(outcome.file_size),
    );
}

fn twitter_quote_secondary_index(
    item_count: usize,
    quoted_media_indexes: &[usize],
    active_index: usize,
) -> Option<usize> {
    let active_is_quoted = quoted_media_indexes.contains(&active_index);
    (0..item_count)
        .find(|index| quoted_media_indexes.contains(index) != active_is_quoted)
}

fn start_post_export(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<StartPostExportPayload>(request.payload.clone()) {
        Ok(payload)
            if uuid::Uuid::parse_str(&payload.analysis_request_id).is_ok()
                && !payload.media_id.trim().is_empty()
                && payload.media_id.len() <= 128 =>
        {
            payload
        }
        _ => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Gönderi kartı payload geçersiz."),
            )
        }
    };
    match insert_mutating_request(request) {
        Ok(InsertOutcome::Duplicate) => {
            return response_for_analysis(request, &payload.analysis_request_id)
        }
        Ok(InsertOutcome::Conflict) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "request_conflict",
                    "Aynı requestId farklı bir istekle kullanıldı.",
                ),
            )
        }
        Err(error) => return ResponseEnvelope::failure(request, error),
        Ok(InsertOutcome::Inserted) => {}
    }
    let stored = store().lock().ok().and_then(|state| {
        state.stored_analysis(&request.client_origin, &payload.analysis_request_id)
    });
    let Some(StoredAnalysis::Media { source_url, value }) = stored else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "post_export_unsupported",
                "Gönderi kartı için hazır X/Twitter analizi bulunamadı.",
            ),
        );
    };
    if value.platform != "twitter" {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "post_export_unsupported",
                "Gönderi kartı yalnız X/Twitter içeriklerinde desteklenir.",
            ),
        );
    }
    let item = value
        .items
        .iter()
        .find(|item| item.id == payload.media_id)
        .cloned();
    if item.is_none() && !(payload.media_id == "post" && value.content_kind == "text") {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("analysis_expired", "Seçili gönderi medyası bulunamadı."),
        );
    }
    let job = match begin_download_job_owned(&request.client_origin, &payload.analysis_request_id) {
        Ok(job) => job,
        Err(error) => return ResponseEnvelope::failure(request, ApiError::from_legacy(error)),
    };
    let job_id = job.id().to_string();
    record_download_job_operation(&job_id, "post_export");
    if let Ok(mut state) = store().lock() {
        state.set_download_error(&request.client_origin, &payload.analysis_request_id, None);
    }

    let app_handle = app.clone();
    let origin = request.client_origin.clone();
    let analysis_request_id = payload.analysis_request_id.clone();
    let media_id = payload.media_id;
    tauri::async_runtime::spawn_blocking(move || {
        update_download_job_progress(None, None, None, None, "Gönderi kartı hazırlanıyor...");
        let expected_mode = item
            .as_ref()
            .map(|item| item.item_type.as_str())
            .unwrap_or("text");
        let preview_path = if expected_mode == "photo" {
            item.as_ref()
                .ok_or_else(|| ApiError::new("preview_failed", "Gönderi fotoğrafı bulunamadı."))
                .and_then(|item| {
                    prepare_companion_media_preview_blocking(
                        &app_handle,
                        &value.analysis_id,
                        &item.id,
                    )
                        .map(|preview| preview.file_path)
                        .map_err(|_| {
                            ApiError::new("preview_failed", "Gönderi fotoğrafı hazırlanamadı.")
                        })
                })
        } else {
            Ok(String::new())
        };
        let secondary_item = item.as_ref().and_then(|active_item| {
            let active_index = value
                .items
                .iter()
                .position(|candidate| candidate.id == active_item.id)?;
            let quote = value.twitter_quote.as_ref()?;
            let secondary_index = twitter_quote_secondary_index(
                value.items.len(),
                &quote.quoted_media_indexes,
                active_index,
            )?;
            value.items.get(secondary_index).cloned()
        });
        let preview_paths = preview_path.and_then(|preview_path| {
            let secondary_preview_path = secondary_item
                .as_ref()
                .map(|secondary| {
                    prepare_companion_media_preview_blocking(
                        &app_handle,
                        &value.analysis_id,
                        &secondary.id,
                    )
                    .map(|preview| preview.file_path)
                    .map_err(|_| {
                        ApiError::new(
                            "preview_failed",
                            "Alıntılanan gönderinin medyası hazırlanamadı.",
                        )
                    })
                })
                .transpose()?
                .unwrap_or_default();
            Ok((preview_path, secondary_preview_path))
        });
        let outcome = preview_paths.and_then(|(preview_path, secondary_preview_path)| {
            let render = request_companion_render(
                &app_handle,
                "twitter_post_export",
                json!({
                    "analysis": value,
                    "sourceUrl": source_url,
                    "mediaId": media_id,
                    "previewPath": preview_path,
                    "secondaryPreviewPath": secondary_preview_path,
                }),
            )
            .map_err(|error| renderer_transport_failure(&app_handle, error))?;
            let rendered: PostExportRenderResult =
                serde_json::from_value(render).map_err(|_| {
                    ApiError::new(
                        "renderer_result_invalid",
                        "Gönderi renderer sonucu geçersiz.",
                    )
                })?;
            if !rendered.ok {
                return Err(renderer_failure(&app_handle, &rendered));
            }
            match current_download_job_stop() {
                Some(DownloadJobStop::Pause) => {
                    return Err(ApiError::new("download_paused", "İndirme duraklatıldı."));
                }
                Some(DownloadJobStop::Cancel) => {
                    return Err(ApiError::new("download_cancelled", "İndirme iptal edildi."));
                }
                None => {}
            }
            if rendered.mode != expected_mode
                || rendered.card_png_base64.is_empty()
                || rendered.card_png_base64.len() > 8 * 1024 * 1024
            {
                return Err(ApiError::new(
                    "renderer_result_invalid",
                    "Gönderi renderer sonucu doğrulanamadı.",
                ));
            }
            let title = item
                .as_ref()
                .map(|item| media_display_title(&value, item))
                .unwrap_or_else(|| {
                    let clean = value.title.trim();
                    if clean.is_empty() {
                        "X gönderisi".to_string()
                    } else {
                        clean.chars().take(512).collect()
                    }
                });
            if expected_mode == "video" {
                let layout = rendered.card_layout.ok_or_else(|| {
                    ApiError::new("renderer_result_invalid", "Gönderi kartı yerleşimi eksik.")
                })?;
                let selected = item.as_ref().expect("validated video item");
                download_twitter_post_blocking(
                    app_handle.clone(),
                    source_url.clone(),
                    "best".to_string(),
                    selected
                        .height
                        .map(|height| format!("{height}p"))
                        .unwrap_or_else(|| "Otomatik".to_string()),
                    None,
                    Some(title),
                    selected.text.clone(),
                    selected.author_name.clone(),
                    selected.author_handle.clone(),
                    selected.display_date.clone(),
                    Some(source_url.clone()),
                    rendered.card_png_base64,
                    (!rendered.card_overlay_png_base64.is_empty())
                        .then_some(rendered.card_overlay_png_base64),
                    layout,
                    Some(value.analysis_id.clone()),
                    Some(selected.id.clone()),
                    Some(job),
                )
                .map(|result| record_video_job_result(&job_id, &result))
                .map_err(ApiError::from_legacy)
            } else {
                download_media_post_card_blocking_with_job(
                    app_handle.clone(),
                    source_url.clone(),
                    format!("data:image/png;base64,{}", rendered.card_png_base64),
                    Some(title),
                    None,
                    job,
                )
                .map(|result| record_media_job_result(&job_id, &result))
                .map_err(ApiError::from_legacy)
            }
        });
        let terminal = match outcome.as_ref().err().map(|error| error.code.as_str()) {
            None => "completed",
            Some("download_paused") => "paused",
            Some("download_cancelled") => "cancelled",
            Some(_) => "error",
        };
        record_download_job_terminal(&job_id, terminal);
        if let Err(error) = outcome {
            if let Ok(mut state) = store().lock() {
                state.set_download_error(&origin, &analysis_request_id, Some(error));
            }
        }
    });
    response_for_analysis(request, &payload.analysis_request_id)
}

fn control_download(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload: ControlPayload = match serde_json::from_value(request.payload.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "İndirme kontrol payload geçersiz."),
            )
        }
    };
    match insert_mutating_request(request) {
        Ok(InsertOutcome::Duplicate) => {
            return response_for_analysis(request, &payload.analysis_request_id)
        }
        Ok(InsertOutcome::Conflict) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "request_conflict",
                    "Aynı requestId farklı bir istekle kullanıldı.",
                ),
            )
        }
        Err(error) => return ResponseEnvelope::failure(request, error),
        Ok(InsertOutcome::Inserted) => {}
    }
    let job_id = payload
        .job_id
        .as_deref()
        .or(payload.previous_job_id.as_deref())
        .unwrap_or("");
    let outcome = match request.command {
        Command::PauseDownload | Command::CancelDownload => {
            ensure_download_job_owner(&request.client_origin, &payload.analysis_request_id, job_id)
                .and_then(|_| {
                    if request.command == Command::PauseDownload {
                        crate::pause_download(Some(job_id.to_string()))
                            .map_err(|error| error.to_string())?;
                    } else {
                        crate::cancel_download(Some(job_id.to_string()))
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(None)
                })
        }
        Command::ResumeDownload => ensure_download_job_history_owner(
            &request.client_origin,
            &payload.analysis_request_id,
            job_id,
        )
        .and_then(|_| {
            store()
                .lock()
                .map_err(|_| "Companion durumu kullanılamıyor.".to_string())?
                .download_plan(&request.client_origin, &payload.analysis_request_id)
                .map(Some)
                .ok_or_else(|| "İndirme devam bilgisi bulunamadı.".to_string())
        }),
        _ => Err("Geçersiz indirme kontrol komutu.".to_string()),
    };
    match outcome {
        Ok(Some(plan)) => run_download_plan(app, request, &payload.analysis_request_id, plan),
        Ok(None) => response_for_analysis(request, &payload.analysis_request_id),
        Err(error) => ResponseEnvelope::failure(request, ApiError::from_legacy(error)),
    }
}

fn response_for_analysis(request: &RequestEnvelope, analysis_request_id: &str) -> ResponseEnvelope {
    let snapshot = store()
        .lock()
        .ok()
        .and_then(|state| state.analysis(&request.client_origin, analysis_request_id));
    snapshot.map_or_else(
        || {
            ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "analysis_expired",
                    "Medya analizi bulunamadı veya süresi doldu.",
                )
                .with_retryable(true),
            )
        },
        |snapshot| snapshot_response(request, snapshot),
    )
}

fn companion_media_auth_mode(
    site: &str,
    has_saved_instagram_cookies: bool,
) -> Option<&'static str> {
    (site == "instagram" && has_saved_instagram_cookies).then_some("saved:instagram")
}

fn site_uses_ytdlp_fallback(site: &str) -> bool {
    matches!(site, "twitter" | "tiktok")
}

fn stored_analysis_supports_twitter_post(stored: &StoredAnalysis) -> bool {
    match stored {
        StoredAnalysis::Media { value, .. } => value.platform == "twitter",
        StoredAnalysis::Video { source_url, .. } => is_twitter_url(source_url),
    }
}

fn json_text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

fn json_count(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|item| {
            item.as_u64().or_else(|| {
                item.as_f64()
                    .filter(|number| number.is_finite() && *number >= 0.0)
                    .map(|number| number.round() as u64)
            })
        })
    })
}

fn twitter_text_analysis_from_ytdlp(value: &Value) -> Option<MediaAnalysis> {
    let has_formats = value
        .get("formats")
        .and_then(Value::as_array)
        .is_some_and(|formats| !formats.is_empty());
    let has_entries = value
        .get("entries")
        .and_then(Value::as_array)
        .is_some_and(|entries| !entries.is_empty());
    if has_formats || has_entries {
        return None;
    }

    let text = json_text(value, &["description", "fulltitle", "title"])?;
    let id = json_text(value, &["id"]).or_else(|| {
        value
            .get("id")
            .and_then(Value::as_u64)
            .map(|id| id.to_string())
    })?;
    let author_name = json_text(value, &["uploader", "channel", "creator"])
        .unwrap_or_else(|| "X/Twitter".to_string());
    let author_handle = json_text(value, &["uploader_id", "channel_id"]).unwrap_or_default();
    let display_date = value
        .get("timestamp")
        .and_then(|timestamp| {
            timestamp
                .as_u64()
                .map(|timestamp| timestamp.to_string())
                .or_else(|| {
                    timestamp
                        .as_f64()
                        .map(|timestamp| timestamp.round().to_string())
                })
        })
        .or_else(|| json_text(value, &["upload_date"]));
    let post = TwitterPostMetadata {
        id,
        author_name: author_name.clone(),
        author_handle: author_handle.clone(),
        avatar_url: None,
        text: Some(text.clone()),
        display_date,
        is_verified: false,
        reply_count: json_count(value, &["comment_count", "reply_count"]),
        retweet_count: json_count(value, &["repost_count", "retweet_count"]),
        like_count: json_count(value, &["like_count"]),
        view_count: json_count(value, &["view_count"]),
    };
    Some(MediaAnalysis {
        analysis_id: uuid::Uuid::new_v4().to_string(),
        expires_at_ms: time_ms().saturating_add(MEDIA_ANALYSIS_TTL_MS),
        platform: "twitter".to_string(),
        content_kind: "text".to_string(),
        title: text,
        uploader: author_name.clone(),
        author: AuthorIdentity {
            id: None,
            name: author_name,
            handle: author_handle,
            avatar_data_url: None,
        },
        items: Vec::new(),
        initial_index: 0,
        requested_item_id: None,
        warnings: Vec::new(),
        instagram_diagnostics: None,
        twitter_quote: None,
        twitter_post: Some(post),
        video_info: None,
    })
}

fn complete_analysis(
    app: tauri::AppHandle,
    origin: String,
    request_id: String,
    source: protocol::ValidatedSource,
) {
    let collect_ytdlp = || {
        run_ytdlp_json_analysis(&app, &source.analysis_url, false, None, false, false).and_then(
            |raw| {
                let value = serde_json::from_str::<Value>(&raw)
                    .map_err(|_| "yt-dlp geçersiz analiz verisi döndürdü.".to_string())?;
                if source.site == "twitter" {
                    if let Some(analysis) = twitter_text_analysis_from_ytdlp(&value) {
                        register_media_analysis(&analysis, &source.analysis_url, None)?;
                        let projection =
                            project_media_analysis(&analysis).map_err(|error| error.to_string())?;
                        return Ok((
                            projection,
                            StoredAnalysis::Media {
                                source_url: source.analysis_url.clone(),
                                value: analysis,
                            },
                        ));
                    }
                }
                let projection = project_ytdlp_analysis(&source.site, &value)
                    .map_err(|error| error.to_string())?;
                Ok((
                    projection,
                    StoredAnalysis::Video {
                        source_url: source.analysis_url.clone(),
                        value,
                    },
                ))
            },
        )
    };
    let result = if source.site == "youtube" {
        collect_ytdlp()
    } else {
        let has_saved_instagram_cookies = source.site == "instagram"
            && crate::read_instagram_cookie_state_blocking().has_saved_cookies;
        let auth_mode = companion_media_auth_mode(&source.site, has_saved_instagram_cookies);
        let media_result =
            collect_media(&app, &source.analysis_url, auth_mode).and_then(|analysis| {
                register_media_analysis(&analysis, &source.analysis_url, auth_mode)?;
                let projection =
                    project_media_analysis(&analysis).map_err(|error| error.to_string())?;
                Ok((
                    projection,
                    StoredAnalysis::Media {
                        source_url: source.analysis_url.clone(),
                        value: analysis,
                    },
                ))
            });
        if site_uses_ytdlp_fallback(&source.site) {
            media_result.or_else(|_| collect_ytdlp())
        } else {
            media_result
        }
    };

    let Ok(mut state) = store().lock() else {
        return;
    };
    match result {
        Ok((projection, stored)) => {
            state.finish_analysis(&origin, &request_id, projection);
            state.store_analysis(&origin, &request_id, stored);
        }
        Err(error) => {
            let (status, error) = analysis_error(&source.site, error);
            state.fail_analysis(&origin, &request_id, status, error);
        }
    }
}

fn analyze_source(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload: SourcePayload = match serde_json::from_value(request.payload.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Analiz payload geçersiz."),
            )
        }
    };
    let source = match validate_source_payload(payload) {
        Ok(source) => source,
        Err(error) => return ResponseEnvelope::failure(request, error),
    };
    let now = time_ms();
    let (outcome, revision) = match store().lock() {
        Ok(mut state) => {
            let outcome = state.insert_request(
                &request.client_origin,
                &request.request_id,
                command_name(request.command),
                &request.payload,
                now,
            );
            if outcome != InsertOutcome::Inserted {
                (outcome, 0)
            } else {
                let revision = state
                    .start_analysis(
                        &request.client_origin,
                        &request.request_id,
                        &source.site,
                        now,
                    )
                    .unwrap_or(0);
                (outcome, revision)
            }
        }
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("state_unavailable", "Companion durumu kullanılamıyor."),
            )
        }
    };
    match outcome {
        InsertOutcome::Duplicate => return response_for_analysis(request, &request.request_id),
        InsertOutcome::Conflict => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new(
                    "request_conflict",
                    "Aynı requestId farklı bir istekle kullanıldı.",
                ),
            )
        }
        InsertOutcome::Inserted => {}
    }

    if current_download_job_id().is_some() {
        let error = ApiError::new("download_busy", "Başka bir indirme işlemi devam ediyor.")
            .with_retryable(true)
            .with_action("show_active_job");
        if let Ok(mut state) = store().lock() {
            state.fail_analysis(
                &request.client_origin,
                &request.request_id,
                AnalysisStatus::Error,
                error.clone(),
            );
        }
        return ResponseEnvelope::failure(request, error);
    }

    let app = app.clone();
    let origin = request.client_origin.clone();
    let request_id = request.request_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        complete_analysis(app, origin, request_id, source)
    });

    ResponseEnvelope::success(
        request,
        "accepted",
        revision,
        json!({"analysisRequestId": request.request_id}),
    )
}

fn get_state(request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<GetStatePayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "State payload geçersiz."),
            )
        }
    };
    let target = if let Some(request_id) = payload.analysis_request_id {
        Some(request_id)
    } else if let Some(page_url) = payload.page_url {
        let page_url = match normalize_page_url(&page_url) {
            Ok(page_url) => page_url,
            Err(error) => return ResponseEnvelope::failure(request, error),
        };
        store().lock().ok().and_then(|mut state| {
            state.latest_request_id_for_page(&request.client_origin, &page_url, time_ms())
        })
    } else {
        store()
            .lock()
            .ok()
            .and_then(|mut state| state.latest_request_id(&request.client_origin, time_ms()))
    };
    match target {
        Some(target) if uuid::Uuid::parse_str(&target).is_ok() => {
            response_for_analysis(request, &target)
        }
        Some(_) => ResponseEnvelope::failure(
            request,
            ApiError::new("invalid_request", "analysisRequestId geçersiz."),
        ),
        None => {
            let job = download_job_snapshot_for(None, None);
            ResponseEnvelope::success(
                request,
                if job.is_some() { "busy" } else { "accepted" },
                0,
                json!({"analysisRequestId": null, "activeJob": job}),
            )
        }
    }
}

fn get_media_preview(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<GetMediaPreviewPayload>(request.payload.clone()) {
        Ok(payload)
            if uuid::Uuid::parse_str(&payload.analysis_request_id).is_ok()
                && !payload.media_id.trim().is_empty()
                && payload.media_id.len() <= 128 =>
        {
            payload
        }
        _ => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Önizleme payload geçersiz."),
            )
        }
    };
    if current_download_job_id().is_some() {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "download_busy",
                "İndirme sürerken yeni önizleme hazırlanamaz.",
            )
            .with_retryable(true),
        );
    }
    let stored = store().lock().ok().and_then(|state| {
        state.stored_analysis(&request.client_origin, &payload.analysis_request_id)
    });
    let Some(StoredAnalysis::Media { value, .. }) = stored else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("preview_failed", "Önizleme kaynağı artık bulunamıyor."),
        );
    };
    let Some(item) = value.items.iter().find(|item| item.id == payload.media_id) else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("preview_failed", "Önizlenecek medya bulunamadı."),
        );
    };
    let prepared = match prepare_companion_media_preview_blocking(app, &value.analysis_id, &item.id)
    {
        Ok(prepared) => prepared,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("preview_failed", "Medya önizlemesi hazırlanamadı."),
            )
        }
    };
    let rendered = match request_companion_render(
        app,
        "media_preview",
        json!({
            "previewPath": prepared.file_path,
            "mediaType": prepared.media_type,
            "width": item.width,
            "height": item.height,
        }),
    ) {
        Ok(rendered) => rendered,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("preview_failed", "Medya önizlemesi oluşturulamadı."),
            )
        }
    };
    let rendered: MediaPreviewRenderResult = match serde_json::from_value(rendered) {
        Ok(rendered) => rendered,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("preview_failed", "Medya önizleme sonucu geçersiz."),
            )
        }
    };
    let valid_data = rendered.ok
        && rendered.data_url.starts_with("data:image/jpeg;base64,")
        && rendered.data_url.len() <= 350_000;
    if !valid_data {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("preview_failed", "Medya önizleme sonucu doğrulanamadı."),
        );
    }
    let duration_seconds = rendered
        .duration_seconds
        .filter(|value| value.is_finite() && *value > 0.0);
    let revision = store()
        .lock()
        .ok()
        .and_then(|state| state.analysis(&request.client_origin, &payload.analysis_request_id))
        .map(|snapshot| snapshot.state_revision)
        .unwrap_or(0);
    ResponseEnvelope::success(
        request,
        "ready",
        revision,
        json!({
            "analysisRequestId": payload.analysis_request_id,
            "mediaId": payload.media_id,
            "dataUrl": rendered.data_url,
            "durationSeconds": duration_seconds,
        }),
    )
}

fn reveal_result(request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<RevealResultPayload>(request.payload.clone()) {
        Ok(payload)
            if uuid::Uuid::parse_str(&payload.analysis_request_id).is_ok()
                && uuid::Uuid::parse_str(&payload.job_id).is_ok() =>
        {
            payload
        }
        _ => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Sonuç gösterme payload geçersiz."),
            )
        }
    };
    let target = match download_job_result_target_for(
        &request.client_origin,
        &payload.analysis_request_id,
        &payload.job_id,
    ) {
        Ok(target) => target,
        Err(error) => {
            return ResponseEnvelope::failure(request, ApiError::from_legacy(error));
        }
    };
    match crate::platform::windows::reveal_download_notification_target(&target) {
        Ok(()) => response_for_analysis(request, &payload.analysis_request_id),
        Err(_) => ResponseEnvelope::failure(
            request,
            ApiError::new(
                "result_missing",
                "İndirilen dosya taşınmış veya silinmiş olabilir.",
            ),
        ),
    }
}

fn reveal_error_report(request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<RevealErrorReportPayload>(request.payload.clone())
    {
        Ok(payload) if valid_report_id(payload.report_id.trim()) => payload,
        _ => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Hata raporu kimliği geçersiz."),
            )
        }
    };
    let target = match crate::mediadrop_error_reports_dir() {
        Ok(dir) => dir.join(payload.report_id.trim()),
        Err(error) => return ResponseEnvelope::failure(request, ApiError::from_legacy(error)),
    };
    match crate::platform::windows::reveal_file_in_explorer(&target) {
        Ok(()) => get_state(request),
        Err(_) => ResponseEnvelope::failure(
            request,
            ApiError::new("result_missing", "Hata raporu bulunamadı."),
        ),
    }
}

fn open_advanced(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    let payload = match serde_json::from_value::<AdvancedPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("invalid_request", "Advanced payload geçersiz."),
            )
        }
    };
    let intent = payload.intent;
    let target = payload.analysis_request_id.or_else(|| {
        store()
            .lock()
            .ok()
            .and_then(|mut state| state.latest_request_id(&request.client_origin, time_ms()))
    });
    let Some(target) = target else {
        return ResponseEnvelope::failure(
            request,
            ApiError::new("analysis_expired", "Açılacak medya analizi bulunamadı."),
        );
    };
    let (stored, snapshot, source_payload) = match store().lock() {
        Ok(state) => (
            state.stored_analysis(&request.client_origin, &target),
            state.analysis(&request.client_origin, &target),
            state.request_payload(&request.client_origin, &target),
        ),
        Err(_) => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("state_unavailable", "Companion durumu kullanılamıyor."),
            )
        }
    };
    if intent.is_some()
        && !stored
            .as_ref()
            .is_some_and(stored_analysis_supports_twitter_post)
    {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "invalid_request",
                "Gönderi kartı yalnızca hazır X/Twitter analizinde kullanılabilir.",
            ),
        );
    }
    let payload = match stored {
        Some(StoredAnalysis::Video { source_url, value }) => {
            let mut payload = json!({"kind":"video", "sourceUrl":source_url, "info":value});
            if let Some(intent) = intent {
                payload["intent"] = Value::String(intent.as_str().to_string());
            }
            payload
        }
        Some(StoredAnalysis::Media { source_url, value }) => {
            let mut payload = json!({"kind":"media", "sourceUrl":source_url, "analysis":value});
            if let Some(intent) = intent {
                payload["intent"] = Value::String(intent.as_str().to_string());
            }
            payload
        }
        None if snapshot.is_some_and(|value| value.status == AnalysisStatus::NeedsUser) => {
            let source = source_payload
                .and_then(|value| serde_json::from_value::<SourcePayload>(value).ok())
                .and_then(|value| validate_source_payload(value).ok());
            let Some(source) = source else {
                return ResponseEnvelope::failure(
                    request,
                    ApiError::new(
                        "analysis_expired",
                        "Devam ettirilecek medya kaynağı bulunamadı.",
                    ),
                );
            };
            json!({"kind":"reanalyze", "sourceUrl":source.analysis_url})
        }
        None => {
            return ResponseEnvelope::failure(
                request,
                ApiError::new("analysis_expired", "Açılacak medya analizi bulunamadı."),
            )
        }
    };
    if let Err(error) = set_pending_handoff(payload.clone()) {
        return ResponseEnvelope::failure(request, error);
    }
    let _ = app.emit("companion-analysis-ready", payload);
    match show_main_window(app) {
        Ok(()) => ResponseEnvelope::success(request, "accepted", 0, json!({})),
        Err(error) => ResponseEnvelope::failure(request, error),
    }
}

fn dispatch(app: &tauri::AppHandle, request: &RequestEnvelope) -> ResponseEnvelope {
    if request.command == Command::Hello {
        return match hello_response(request) {
            Ok(response) => {
                let accepted = response.status == "accepted";
                EXTENSION_CONNECTED.store(accepted, Ordering::Release);
                if accepted {
                    #[cfg(target_os = "windows")]
                    windows_pipe::signal_installer_extension_connection();
                } else if hello_requires_extension_refresh(&response) {
                    queue_extension_setup_request();
                    let _ = app.emit("open-extension-setup", ());
                    let _ = show_main_window(app);
                }
                response
            }
            Err(error) => ResponseEnvelope::failure(request, error),
        };
    }
    if request.protocol_version != PROTOCOL_VERSION {
        return ResponseEnvelope::failure(
            request,
            ApiError::new(
                "version_mismatch",
                "Companion protokol sürümü uyumlu değil.",
            ),
        );
    }
    match request.command {
        Command::Hello => unreachable!(),
        Command::AnalyzeSource => analyze_source(app, request),
        Command::GetState => get_state(request),
        Command::GetMediaPreview => get_media_preview(app, request),
        Command::OpenApp => match show_main_window(app) {
            Ok(()) => ResponseEnvelope::success(request, "accepted", 0, json!({})),
            Err(error) => ResponseEnvelope::failure(request, error),
        },
        Command::OpenAdvanced => open_advanced(app, request),
        Command::OpenDownloads => {
            let result = crate::resolve_download_dir(app, None).and_then(|path| {
                crate::platform::windows::reveal_download_notification_target(&path)
            });
            match result {
                Ok(()) => get_state(request),
                Err(error) => ResponseEnvelope::failure(request, ApiError::from(error)),
            }
        }
        Command::RevealResult => reveal_result(request),
        Command::RevealErrorReport => reveal_error_report(request),
        Command::StartDownload => start_download(app, request),
        Command::StartMediaBatch => start_media_batch(app, request),
        Command::StartPostExport => start_post_export(app, request),
        Command::PauseDownload | Command::ResumeDownload | Command::CancelDownload => {
            control_download(app, request)
        }
    }
}

pub(crate) fn handle_request(app: &tauri::AppHandle, bytes: &[u8]) -> Vec<u8> {
    let request = match parse_request(bytes) {
        Ok(request) => request,
        Err(error) => return unbound_error_response(bytes, error),
    };
    response_bytes(&dispatch(app, &request))
}

#[cfg(test)]
mod tests {
    use crate::core::error::{ApiError, STRUCTURED_ERROR_PREFIX};

    use super::protocol::{parse_request, ResponseEnvelope, MAX_RESPONSE_BYTES};

    use super::build_download_plan;
    use super::state::{AnalysisStatus, StoredAnalysis};
    use super::{
        analysis_error, companion_media_auth_mode, site_uses_ytdlp_fallback,
        stored_analysis_supports_twitter_post, twitter_text_analysis_from_ytdlp, AdvancedIntent,
        AdvancedPayload,
    };
    use super::{get_state, set_pending_handoff, take_companion_handoff};

    #[test]
    fn analysis_errors_preserve_typed_user_actions_but_hide_raw_tool_output() {
        let typed = ApiError::new(
            "youtube_auth_required",
            "Masaüstünde tarayıcı izni gerekli.",
        )
        .with_action("select_cookie_browser");
        let encoded = format!(
            "{}{}",
            STRUCTURED_ERROR_PREFIX,
            serde_json::to_string(&typed).unwrap()
        );
        let (status, error) = analysis_error("youtube", encoded);
        assert_eq!(status, AnalysisStatus::NeedsUser);
        assert_eq!(error.code, "youtube_auth_required");

        let typed = ApiError::new(
            "instagram_auth_required",
            "public: gallery-dl JSON çıktısı: https://example.invalid/?token=SECRET_CANARY",
        )
        .with_retryable(true)
        .with_action("open_advanced")
        .with_report_id("report-1");
        let encoded = format!(
            "{}{}",
            STRUCTURED_ERROR_PREFIX,
            serde_json::to_string(&typed).unwrap()
        );
        let (status, error) = analysis_error("instagram", encoded);
        assert_eq!(status, AnalysisStatus::NeedsUser);
        assert_eq!(error.code, "instagram_auth_required");
        assert_eq!(error.action.as_deref(), Some("open_advanced"));
        assert_eq!(error.report_id.as_deref(), Some("report-1"));
        assert_eq!(
            error.message,
            "Instagram oturumunu yenilemek için masaüstü uygulamasında tarayıcı iznini tamamla."
        );
        assert!(!error.message.contains("gallery-dl"));
        assert!(!error.message.contains("SECRET_CANARY"));

        let (status, error) = analysis_error(
            "youtube",
            "yt-dlp failed for https://example.invalid/watch?token=SECRET_CANARY".to_string(),
        );
        assert_eq!(status, AnalysisStatus::Error);
        assert_eq!(error.code, "analysis_failed");
        assert!(!error.message.contains("SECRET_CANARY"));
        assert!(!error.message.contains("http"));
    }

    #[test]
    fn companion_instagram_analysis_reuses_only_an_existing_saved_cookie_jar() {
        assert_eq!(
            companion_media_auth_mode("instagram", true),
            Some("saved:instagram")
        );
        assert_eq!(companion_media_auth_mode("instagram", false), None);
        assert_eq!(companion_media_auth_mode("twitter", true), None);
    }

    #[test]
    fn advanced_payload_accepts_only_the_typed_twitter_post_intent() {
        let payload = serde_json::from_value::<AdvancedPayload>(serde_json::json!({
            "analysisRequestId":"11111111-1111-4111-8111-111111111111",
            "intent":"download_twitter_post"
        }))
        .expect("typed advanced intent");
        assert_eq!(payload.intent, Some(AdvancedIntent::DownloadTwitterPost));
        assert_eq!(payload.intent.unwrap().as_str(), "download_twitter_post");

        assert!(
            serde_json::from_value::<AdvancedPayload>(serde_json::json!({
                "intent":"run_arbitrary_action"
            }))
            .is_err()
        );
    }

    #[test]
    fn twitter_and_tiktok_use_the_existing_ytdlp_fallback() {
        assert!(site_uses_ytdlp_fallback("twitter"));
        assert!(site_uses_ytdlp_fallback("tiktok"));
        assert!(!site_uses_ytdlp_fallback("instagram"));

        let twitter = StoredAnalysis::Video {
            source_url: "https://x.com/mediadrop/status/1".to_string(),
            value: serde_json::json!({"title":"Metin tweet"}),
        };
        let youtube = StoredAnalysis::Video {
            source_url: "https://www.youtube.com/watch?v=1".to_string(),
            value: serde_json::json!({"title":"Video"}),
        };
        assert!(stored_analysis_supports_twitter_post(&twitter));
        assert!(!stored_analysis_supports_twitter_post(&youtube));
    }

    #[test]
    fn twitter_metadata_only_fallback_becomes_a_text_post_analysis() {
        let analysis = twitter_text_analysis_from_ytdlp(&serde_json::json!({
            "id":"2090883745628704991",
            "title":"NASA - fallback title",
            "description":"The word attitude here refers to spacecraft position.",
            "uploader":"NASA",
            "uploader_id":"NASA",
            "timestamp":1787340506,
            "like_count":42,
            "formats":[]
        }))
        .expect("metadata-only Twitter result should become a text analysis");

        assert_eq!(analysis.content_kind, "text");
        assert!(analysis.items.is_empty());
        let post = analysis.twitter_post.expect("post metadata");
        assert_eq!(post.id, "2090883745628704991");
        assert_eq!(post.author_name, "NASA");
        assert_eq!(post.author_handle, "NASA");
        assert_eq!(
            post.text.as_deref(),
            Some("The word attitude here refers to spacecraft position.")
        );
        assert_eq!(post.like_count, Some(42));
    }

    #[test]
    fn twitter_video_fallback_is_not_misclassified_as_text() {
        assert!(twitter_text_analysis_from_ytdlp(&serde_json::json!({
            "id":"video",
            "title":"Video",
            "formats":[{"format_id":"hls"}]
        }))
        .is_none());
    }

    #[test]
    fn instagram_login_redirect_requires_the_desktop_auth_flow() {
        let (status, error) = analysis_error(
            "instagram",
            "gallery-dl: AbortExtraction: HTTP redirect to login page (secret URL omitted)"
                .to_string(),
        );

        assert_eq!(status, AnalysisStatus::NeedsUser);
        assert_eq!(error.code, "instagram_auth_required");
        assert_eq!(error.action.as_deref(), Some("open_advanced"));
        assert!(!error.message.contains("HTTP"));
    }

    #[test]
    fn twitter_login_failure_requires_the_desktop_auth_flow() {
        let (status, error) = analysis_error(
            "twitter",
            "gallery-dl: AuthenticationError: login required; token=SECRET_CANARY".to_string(),
        );

        assert_eq!(status, AnalysisStatus::NeedsUser);
        assert_eq!(error.code, "twitter_auth_required");
        assert_eq!(error.action.as_deref(), Some("open_advanced"));
        assert!(!error.message.contains("SECRET_CANARY"));
    }

    #[test]
    fn advanced_handoff_is_latest_only_and_consumed_once() {
        set_pending_handoff(serde_json::json!({"kind":"video","sourceUrl":"one"})).unwrap();
        set_pending_handoff(serde_json::json!({"kind":"media","sourceUrl":"two"})).unwrap();
        let handoff = take_companion_handoff().unwrap().unwrap();
        assert_eq!(handoff["sourceUrl"], "two");
        assert_eq!(take_companion_handoff().unwrap(), None);
    }

    #[test]
    fn oversized_response_keeps_the_request_correlation_id() {
        let request = parse_request(
            &serde_json::to_vec(&serde_json::json!({
                "messageType":"request",
                "protocolVersion":1,
                "requestId":"77777777-7777-4777-8777-777777777777",
                "command":"get_state",
                "clientOrigin":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                "payload":{}
            }))
            .unwrap(),
        )
        .unwrap();
        let response = ResponseEnvelope::success(
            &request,
            "ready",
            1,
            serde_json::json!({"oversized":"x".repeat(MAX_RESPONSE_BYTES)}),
        );
        let value: serde_json::Value =
            serde_json::from_slice(&super::response_bytes(&response)).unwrap();
        assert_eq!(value["requestId"], request.request_id);
        assert_eq!(value["command"], "get_state");
        assert_eq!(value["error"]["code"], "message_too_large");
    }

    #[test]
    fn malformed_get_state_payload_is_rejected_instead_of_reading_latest_state() {
        let request = parse_request(
            &serde_json::to_vec(&serde_json::json!({
                "messageType":"request",
                "protocolVersion":1,
                "requestId":"88888888-8888-4888-8888-888888888888",
                "command":"get_state",
                "clientOrigin":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                "payload":{"analysisRequestId":42}
            }))
            .unwrap(),
        )
        .unwrap();
        let response = get_state(&request);
        assert_eq!(response.status, "invalid_request");
        assert_eq!(response.error.unwrap().code, "invalid_request");
    }

    #[test]
    fn no_media_error_uses_the_popup_unsupported_state() {
        let request = parse_request(
            &serde_json::to_vec(&serde_json::json!({
                "messageType":"request",
                "protocolVersion":1,
                "requestId":"99999999-9999-4999-8999-999999999999",
                "command":"get_state",
                "clientOrigin":"chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                "payload":{}
            }))
            .unwrap(),
        )
        .unwrap();
        let response = ResponseEnvelope::failure(
            &request,
            ApiError::new("media_not_found", "Medya bulunamadı."),
        );
        assert_eq!(response.status, "unsupported");
    }

    #[test]
    fn youtube_choice_is_revalidated_against_stored_formats() {
        let stored = StoredAnalysis::Video {
            source_url: "https://www.youtube.com/watch?v=one".to_string(),
            value: serde_json::json!({
                "id":"m1",
                "title":"Example",
                "formats":[
                    {"format_id":"137","vcodec":"avc1","acodec":"none","height":1080},
                    {"format_id":"140","vcodec":"none","acodec":"mp4a","abr":128}
                ]
            }),
        };
        let video = build_download_plan(&stored, "m1", "video:137").unwrap();
        assert_eq!(video.format_id, "137");
        assert_eq!(video.kind, "video");
        assert_eq!(video.quality, "1080p");
        let audio = build_download_plan(&stored, "m1", "audio:140").unwrap();
        assert_eq!(audio.kind, "audio");
        assert!(build_download_plan(&stored, "m1", "video:137/bestaudio").is_err());
        assert!(build_download_plan(&stored, "m1", "video:140").is_err());
    }

    #[test]
    fn social_ytdlp_fallback_accepts_the_popup_auto_choice() {
        let stored = StoredAnalysis::Video {
            source_url: "https://www.tiktok.com/@creator/video/1".to_string(),
            value: serde_json::json!({
                "id":"m1",
                "title":"Example",
                "formats":[
                    {"format_id":"video-720","vcodec":"h264","acodec":"aac","height":720},
                    {"format_id":"audio","vcodec":"none","acodec":"aac"}
                ]
            }),
        };

        let plan = build_download_plan(&stored, "m1", "social:auto").unwrap();

        assert_eq!(plan.format_id, "best[ext=mp4]/bestvideo+bestaudio/best");
        assert_eq!(plan.kind, "tiktok");
        assert_eq!(plan.quality, "720p");
    }

    #[test]
    fn quoted_post_export_selects_media_from_the_other_post() {
        assert_eq!(
            super::twitter_quote_secondary_index(2, &[1], 0),
            Some(1)
        );
        assert_eq!(
            super::twitter_quote_secondary_index(2, &[1], 1),
            Some(0)
        );
        assert_eq!(super::twitter_quote_secondary_index(1, &[], 0), None);
    }

    #[test]
    fn companion_clip_range_is_youtube_video_only_and_duration_bounded() {
        let stored = StoredAnalysis::Video {
            source_url: "https://www.youtube.com/watch?v=one".to_string(),
            value: serde_json::json!({
                "id":"m1",
                "title":"Example",
                "duration":120.0,
                "formats":[
                    {"format_id":"137","vcodec":"avc1","acodec":"none","height":1080},
                    {"format_id":"140","vcodec":"none","acodec":"mp4a","abr":128}
                ]
            }),
        };
        let plan = build_download_plan(&stored, "m1", "video:137").unwrap();
        let clip = super::ClipPayload {
            start_seconds: 15.0,
            end_seconds: 42.0,
        };
        assert_eq!(
            super::validate_companion_clip(&stored, "m1", &plan, Some(clip)).unwrap(),
            Some((15.0, 42.0))
        );
        assert!(super::validate_companion_clip(
            &stored,
            "m1",
            &plan,
            Some(super::ClipPayload {
                start_seconds: 42.0,
                end_seconds: 42.5
            })
        )
        .is_err());
        assert!(super::validate_companion_clip(
            &stored,
            "m1",
            &plan,
            Some(super::ClipPayload {
                start_seconds: 15.0,
                end_seconds: 121.0
            })
        )
        .is_err());

        let audio = build_download_plan(&stored, "m1", "audio:best").unwrap();
        assert!(super::validate_companion_clip(&stored, "m1", &audio, Some(clip)).is_err());
    }

    #[test]
    fn renderer_reports_are_redacted_and_report_ids_cannot_escape_the_report_dir() {
        let result = super::PostExportRenderResult {
            ok: false,
            mode: "video".to_string(),
            card_png_base64: "SECRET_CANARY_BASE64".to_string(),
            card_overlay_png_base64: String::new(),
            card_layout: None,
            error_code: "placeholder_render_failed".to_string(),
            stage: "canvas".to_string(),
        };
        let details = super::renderer_report_details(&result);
        assert!(details.contains("placeholder_render_failed"));
        assert!(details.contains("canvas"));
        assert!(!details.contains("SECRET_CANARY_BASE64"));
        assert!(super::valid_report_id(
            "mediadrop-companion-renderer-123.txt"
        ));
        assert!(!super::valid_report_id("..\\secret.txt"));
        assert!(!super::valid_report_id(
            "mediadrop-companion-renderer-123.log"
        ));
    }

    #[test]
    fn social_choice_requires_registered_media_identity() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(serde_json::json!({
            "analysisId":"analysis-1","expiresAtMs":999999,"platform":"instagram",
            "contentKind":"video","title":"Post","uploader":"Creator",
            "items":[{"id":"item-1","type":"video","sourceIndex":0,"width":720,"height":1280,
                "extension":"mp4","isStory":false,"hasAudio":true,"title":"Clip"}],
            "videoInfo":null
        }))
        .unwrap();
        let stored = StoredAnalysis::Media {
            source_url: "https://www.instagram.com/p/example/".to_string(),
            value: analysis,
        };
        let video = build_download_plan(&stored, "item-1", "social:auto").unwrap();
        assert_eq!(video.kind, "instagram");
        assert_eq!(
            video.registry_item,
            Some(("analysis-1".to_string(), "item-1".to_string()))
        );
        let audio = build_download_plan(&stored, "item-1", "audio:best").unwrap();
        assert_eq!(audio.kind, "audio");
        assert_eq!(audio.registry_item, None);
        let mut multi = stored.clone();
        if let StoredAnalysis::Media { value, .. } = &mut multi {
            let mut second = value.items[0].clone();
            second.id = "item-2".to_string();
            value.items.push(second);
        }
        assert!(build_download_plan(&multi, "item-1", "audio:best").is_err());
        assert!(build_download_plan(&stored, "missing", "social:auto").is_err());
    }
}
