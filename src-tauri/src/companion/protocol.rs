use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::core::error::ApiError;
use crate::{
    is_instagram_url, is_supported_media_url, is_tiktok_url, is_twitter_url, is_youtube_url,
};

pub(crate) const PROTOCOL_VERSION: u16 = 1;
pub(crate) const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_URL_BYTES: usize = 8192;
pub(crate) const MAX_CANDIDATES: usize = 8;
pub(crate) const MAX_MEDIA_ENTRIES: usize = 20;
pub(crate) const MAX_FORMATS_PER_MEDIA: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Command {
    Hello,
    AnalyzeSource,
    GetState,
    GetMediaPreview,
    OpenApp,
    OpenAdvanced,
    OpenDownloads,
    RevealResult,
    RevealErrorReport,
    StartDownload,
    StartMediaBatch,
    StartPostExport,
    PauseDownload,
    ResumeDownload,
    CancelDownload,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestEnvelope {
    pub(crate) message_type: String,
    pub(crate) protocol_version: u16,
    pub(crate) request_id: String,
    pub(crate) command: Command,
    #[serde(default)]
    pub(crate) client_origin: String,
    #[serde(default)]
    pub(crate) payload: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtocolRange {
    pub(crate) min: u16,
    pub(crate) max: u16,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelloPayload {
    supported_protocol: ProtocolRange,
    #[serde(default)]
    extension_version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Capabilities {
    pub(crate) analyze_source: bool,
    pub(crate) preview_media: bool,
    pub(crate) open_app: bool,
    pub(crate) open_advanced: bool,
    pub(crate) open_downloads: bool,
    pub(crate) reveal_result: bool,
    pub(crate) reveal_error_report: bool,
    pub(crate) start_download: bool,
    pub(crate) start_clip: bool,
    pub(crate) start_media_batch: bool,
    pub(crate) start_post_export: bool,
    pub(crate) pause_download: bool,
    pub(crate) resume_download: bool,
    pub(crate) cancel_download: bool,
}

impl Capabilities {
    pub(crate) fn current() -> Self {
        Self {
            analyze_source: true,
            preview_media: true,
            open_app: true,
            open_advanced: true,
            open_downloads: true,
            reveal_result: true,
            reveal_error_report: true,
            start_download: true,
            start_clip: true,
            start_media_batch: true,
            start_post_export: true,
            pause_download: true,
            resume_download: true,
            cancel_download: true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseEnvelope {
    pub(crate) message_type: &'static str,
    pub(crate) protocol_version: u16,
    pub(crate) request_id: String,
    pub(crate) command: Command,
    pub(crate) status: String,
    pub(crate) state_revision: u64,
    pub(crate) payload: serde_json::Value,
    pub(crate) capabilities: Capabilities,
    pub(crate) error: Option<ApiError>,
}

impl ResponseEnvelope {
    pub(crate) fn success(
        request: &RequestEnvelope,
        status: impl Into<String>,
        state_revision: u64,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            message_type: "response",
            protocol_version: PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            command: request.command,
            status: status.into(),
            state_revision,
            payload,
            capabilities: Capabilities::current(),
            error: None,
        }
    }

    pub(crate) fn failure(request: &RequestEnvelope, error: ApiError) -> Self {
        let status = match error.code.as_str() {
            "version_mismatch" => "version_mismatch",
            "media_not_found" | "unsupported_source" | "unsupported" => "unsupported",
            "download_busy" | "analysis_busy" => "busy",
            "invalid_request" | "request_conflict" | "message_too_large" => "invalid_request",
            code if code.contains("auth") || code.contains("cookie") => "needs_user",
            _ => "error",
        };
        Self {
            message_type: "response",
            protocol_version: PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            command: request.command,
            status: status.to_string(),
            state_revision: 0,
            payload: serde_json::json!({}),
            capabilities: Capabilities::current(),
            error: Some(error),
        }
    }
}

pub(crate) fn hello_response(request: &RequestEnvelope) -> Result<ResponseEnvelope, ApiError> {
    if request.command != Command::Hello {
        return Err(invalid("İlk companion komutu hello olmalıdır."));
    }
    let payload: HelloPayload = serde_json::from_value(request.payload.clone())
        .map_err(|_| invalid("Hello payload geçersiz."))?;
    if payload.extension_version.chars().count() > 64 {
        return Err(invalid("Eklenti sürümü geçersiz."));
    }
    let selected = negotiate_protocol(
        payload.supported_protocol.min,
        payload.supported_protocol.max,
    )?;
    if payload.extension_version != env!("CARGO_PKG_VERSION") {
        let mut response = ResponseEnvelope::failure(
            request,
            ApiError::new(
                "version_mismatch",
                "MediaDrop eklenti dosyaları güncellendi. Eklentiyi bir kez yenile.",
            )
            .with_action("reload_extension"),
        );
        response.payload = serde_json::json!({
            "expectedExtensionVersion": env!("CARGO_PKG_VERSION")
        });
        return Ok(response);
    }
    Ok(ResponseEnvelope::success(
        request,
        "accepted",
        0,
        serde_json::json!({
            "selectedProtocol": selected,
            "appVersion": env!("CARGO_PKG_VERSION"),
            "hostVersion": env!("CARGO_PKG_VERSION")
        }),
    ))
}

pub(crate) fn hello_requires_extension_refresh(response: &ResponseEnvelope) -> bool {
    response.status == "version_mismatch"
        && response
            .error
            .as_ref()
            .and_then(|error| error.action.as_deref())
            == Some("reload_extension")
}

fn valid_extension_origin(value: &str) -> bool {
    let Some(id) = value
        .strip_prefix("chrome-extension://")
        .and_then(|value| value.strip_suffix('/'))
    else {
        return false;
    };
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

pub(crate) fn parse_request(bytes: &[u8]) -> Result<RequestEnvelope, ApiError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ApiError::new(
            "message_too_large",
            "Companion isteği izin verilen boyutu aşıyor.",
        ));
    }
    let request: RequestEnvelope = serde_json::from_slice(bytes)
        .map_err(|_| invalid("Companion isteği geçerli JSON değil."))?;
    if request.message_type != "request"
        || uuid::Uuid::parse_str(&request.request_id)
            .ok()
            .and_then(|value| value.get_version_num().eq(&4).then_some(value))
            .is_none()
        || !valid_extension_origin(&request.client_origin)
    {
        return Err(invalid("Companion request envelope geçersiz."));
    }
    Ok(request)
}

pub(crate) fn negotiate_protocol(min: u16, max: u16) -> Result<u16, ApiError> {
    if min > max {
        return Err(invalid("Protocol sürüm aralığı geçersiz."));
    }
    if min <= PROTOCOL_VERSION && max >= PROTOCOL_VERSION {
        Ok(PROTOCOL_VERSION)
    } else {
        Err(ApiError::new(
            "version_mismatch",
            "Eklenti ile MediaDrop protokol sürümleri uyumlu değil.",
        )
        .with_action("update_app_or_extension"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourcePayload {
    pub(crate) page_url: String,
    #[serde(default)]
    pub(crate) frame_url: Option<String>,
    #[serde(default)]
    pub(crate) media_type: Option<String>,
    #[serde(default)]
    pub(crate) candidates: Vec<SourceCandidate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCandidate {
    #[serde(default)]
    pub(crate) candidate_url: Option<String>,
    pub(crate) detected_by: String,
    #[serde(default)]
    pub(crate) media_type: Option<String>,
    #[serde(default)]
    pub(crate) duration_seconds: Option<f64>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) playing: bool,
    #[serde(default)]
    pub(crate) visible: bool,
    #[serde(default)]
    pub(crate) live: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedSource {
    pub(crate) analysis_url: String,
    pub(crate) page_url: String,
    pub(crate) frame_url: Option<String>,
    pub(crate) site: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectedAnalysis {
    pub(crate) site: String,
    pub(crate) media: Vec<ProjectedMedia>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectedMedia {
    pub(crate) media_id: String,
    #[serde(rename = "type")]
    pub(crate) media_type: String,
    pub(crate) source_index: usize,
    pub(crate) display_title: String,
    pub(crate) author: String,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) is_story: bool,
    pub(crate) has_audio: bool,
    pub(crate) preview_available: bool,
    pub(crate) title: String,
    pub(crate) uploader: String,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) thumbnail_url: Option<String>,
    pub(crate) formats: Vec<ProjectedFormat>,
    pub(crate) audio_available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ProjectedFormat {
    pub(crate) format_id: String,
    pub(crate) ext: String,
    pub(crate) vcodec: String,
    pub(crate) acodec: String,
    pub(crate) protocol: String,
    pub(crate) width: Option<u64>,
    pub(crate) height: Option<u64>,
    pub(crate) filesize: Option<u64>,
    pub(crate) filesize_approx: Option<u64>,
    pub(crate) abr: Option<f64>,
}

fn bounded_display_text(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

pub(crate) fn media_display_title(
    analysis: &crate::MediaAnalysis,
    item: &crate::MediaItem,
) -> String {
    let opaque = |value: &str| {
        let clean = value.trim();
        let lower = clean.to_lowercase();
        clean.is_empty()
            || clean == item.id
            || clean.eq_ignore_ascii_case("Twitter Video")
            || matches!(
                lower.as_str(),
                "instagram medyasi"
                    | "instagram medyası"
                    | "instagram hikayesi"
                    | "tiktok medyasi"
                    | "tiktok medyası"
            )
    };
    let candidate = if analysis.platform == "twitter" {
        item.text
            .as_deref()
            .filter(|value| {
                !opaque(value) && !value.trim().to_lowercase().starts_with("twitter video #")
            })
            .or_else(|| {
                analysis
                    .twitter_quote
                    .as_ref()
                    .and_then(|quote| quote.outer.text.as_deref())
                    .filter(|value| !opaque(value))
            })
            .or_else(|| (!opaque(&analysis.title)).then_some(analysis.title.as_str()))
            .or_else(|| (!opaque(&item.title)).then_some(item.title.as_str()))
    } else {
        (!opaque(&item.title))
            .then_some(item.title.as_str())
            .or_else(|| item.text.as_deref().filter(|value| !opaque(value)))
            .or_else(|| (!opaque(&analysis.title)).then_some(analysis.title.as_str()))
    };
    let fallback = match analysis.platform.as_str() {
        "twitter" => analysis
            .author
            .name
            .trim()
            .is_empty()
            .then_some("X gönderisi".to_string())
            .unwrap_or_else(|| format!("{} gönderisi", analysis.author.name.trim())),
        "instagram" => {
            let author = if analysis.author.name.trim().is_empty() {
                analysis.uploader.trim()
            } else {
                analysis.author.name.trim()
            };
            if author.is_empty() {
                "Instagram içeriği".to_string()
            } else if item.is_story || analysis.content_kind == "story" {
                format!("{} hikayesi", author)
            } else {
                format!("{} gönderisi", author)
            }
        }
        "tiktok" => "TikTok içeriği".to_string(),
        _ => "Medya".to_string(),
    };
    let title = candidate
        .map(|value| bounded_display_text(value, 512))
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    title
}

fn invalid(message: &str) -> ApiError {
    ApiError::new("invalid_request", message)
}

fn parse_web_url(value: &str) -> Result<Url, ApiError> {
    let clean = value.trim();
    if clean.is_empty() || clean.as_bytes().len() > MAX_URL_BYTES {
        return Err(invalid("Kaynak URL boş veya izin verilen sınırdan uzun."));
    }
    let parsed = Url::parse(clean).map_err(|_| invalid("Kaynak URL geçersiz."))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host_str().is_none()
    {
        return Err(invalid(
            "Yalnız kimlik bilgisi içermeyen HTTP/HTTPS URL kabul edilir.",
        ));
    }
    Ok(parsed)
}

pub(crate) fn normalize_page_url(value: &str) -> Result<String, ApiError> {
    parse_web_url(value).map(|url| url.to_string())
}

fn site_for_url(value: &str) -> Option<&'static str> {
    if is_youtube_url(value) {
        Some("youtube")
    } else if is_instagram_url(value) {
        Some("instagram")
    } else if is_twitter_url(value) {
        Some("twitter")
    } else if is_tiktok_url(value) {
        Some("tiktok")
    } else {
        None
    }
}

pub(crate) fn validate_source_payload(payload: SourcePayload) -> Result<ValidatedSource, ApiError> {
    if payload.candidates.len() > MAX_CANDIDATES {
        return Err(invalid("En fazla sekiz medya adayı gönderilebilir."));
    }
    if let Some(media_type) = payload.media_type.as_deref() {
        if !matches!(media_type, "video" | "audio") {
            return Err(invalid("Medya türü video veya audio olmalıdır."));
        }
    }

    let page = parse_web_url(&payload.page_url)?;
    let page_url = page.to_string();
    let frame_url = payload
        .frame_url
        .as_deref()
        .map(parse_web_url)
        .transpose()?
        .map(|value| value.to_string());

    for candidate in &payload.candidates {
        if candidate.detected_by.trim().is_empty() || candidate.detected_by.len() > 64 {
            return Err(invalid("Aday tespit kaynağı geçersiz."));
        }
        if let Some(candidate_url) = candidate.candidate_url.as_deref() {
            parse_web_url(candidate_url)?;
        }
        if candidate
            .duration_seconds
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(invalid("Aday medya süresi geçersiz."));
        }
    }

    let (analysis_url, site) = if is_supported_media_url(&page_url) {
        (page_url.clone(), site_for_url(&page_url))
    } else if let Some(frame) = frame_url
        .as_deref()
        .filter(|value| is_supported_media_url(value))
    {
        (frame.to_string(), site_for_url(frame))
    } else {
        return Err(ApiError::new(
            "unsupported_source",
            "Bu kaynak MediaDrop companion tarafından henüz desteklenmiyor.",
        ));
    };

    Ok(ValidatedSource {
        analysis_url,
        page_url,
        frame_url,
        site: site.unwrap_or("unknown").to_string(),
    })
}

fn text(value: Option<&serde_json::Value>, limit: usize) -> String {
    value
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(limit)
        .collect()
}

fn public_thumbnail_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let parsed = parse_web_url(raw).ok()?;
    (parsed.scheme() == "https").then(|| parsed.to_string())
}

fn public_thumbnail(value: Option<&serde_json::Value>) -> Option<String> {
    public_thumbnail_url(value.and_then(serde_json::Value::as_str)?)
}

fn projected_format(value: &serde_json::Value) -> Option<ProjectedFormat> {
    let format_id = text(value.get("format_id"), 128);
    if format_id.is_empty() {
        return None;
    }
    Some(ProjectedFormat {
        format_id,
        ext: text(value.get("ext"), 16),
        vcodec: text(value.get("vcodec"), 64),
        acodec: text(value.get("acodec"), 64),
        protocol: text(value.get("protocol"), 32),
        width: value.get("width").and_then(serde_json::Value::as_u64),
        height: value.get("height").and_then(serde_json::Value::as_u64),
        filesize: value.get("filesize").and_then(serde_json::Value::as_u64),
        filesize_approx: value
            .get("filesize_approx")
            .and_then(serde_json::Value::as_u64),
        abr: value.get("abr").and_then(serde_json::Value::as_f64),
    })
}

fn projected_media(value: &serde_json::Value, index: usize) -> ProjectedMedia {
    let formats = value
        .get("formats")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(projected_format)
        .take(MAX_FORMATS_PER_MEDIA)
        .collect::<Vec<_>>();
    let audio_available = formats
        .iter()
        .any(|format| !format.acodec.is_empty() && format.acodec != "none");
    let media_id = text(value.get("id"), 128);
    let title = text(value.get("title"), 512);
    let uploader = text(value.get("uploader"), 256);
    let thumbnail_url = public_thumbnail(value.get("thumbnail"));
    ProjectedMedia {
        media_id: if media_id.is_empty() {
            format!("m{}", index + 1)
        } else {
            media_id
        },
        media_type: "video".to_string(),
        source_index: index,
        display_title: title.clone(),
        author: uploader.clone(),
        width: value
            .get("width")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        height: value
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        is_story: false,
        has_audio: audio_available,
        preview_available: thumbnail_url.is_some(),
        title,
        uploader,
        duration_seconds: value
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0),
        thumbnail_url,
        formats,
        audio_available,
    }
}

pub(crate) fn project_ytdlp_analysis(
    site: &str,
    value: &serde_json::Value,
) -> Result<ProjectedAnalysis, ApiError> {
    let media = value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .take(MAX_MEDIA_ENTRIES)
                .enumerate()
                .map(|(index, entry)| projected_media(entry, index))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![projected_media(value, 0)]);
    if media.is_empty() {
        return Err(ApiError::new(
            "media_not_found",
            "Bu sayfada indirilebilir medya bulunamadı.",
        ));
    }
    Ok(ProjectedAnalysis {
        site: site.chars().take(32).collect(),
        media,
    })
}

pub(crate) fn project_media_analysis(
    analysis: &crate::MediaAnalysis,
) -> Result<ProjectedAnalysis, ApiError> {
    let mut media = analysis
        .items
        .iter()
        .take(MAX_MEDIA_ENTRIES)
        .enumerate()
        .map(|(index, item)| {
            let is_video = item.item_type == "video";
            let title = media_display_title(analysis, item);
            let author = item
                .author_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    if analysis.author.name.trim().is_empty() {
                        analysis.uploader.as_str()
                    } else {
                        analysis.author.name.as_str()
                    }
                })
                .chars()
                .take(256)
                .collect::<String>();
            let formats = is_video
                .then(|| ProjectedFormat {
                    format_id: item.id.chars().take(128).collect(),
                    ext: item.extension.chars().take(16).collect(),
                    vcodec: "video".to_string(),
                    acodec: if item.has_audio { "audio" } else { "none" }.to_string(),
                    protocol: "registry".to_string(),
                    width: item.width.map(u64::from),
                    height: item.height.map(u64::from),
                    filesize: None,
                    filesize_approx: None,
                    abr: None,
                })
                .into_iter()
                .collect();
            ProjectedMedia {
                media_id: if item.id.trim().is_empty() {
                    format!("m{}", index + 1)
                } else {
                    item.id.chars().take(128).collect()
                },
                media_type: item.item_type.chars().take(16).collect(),
                source_index: item.source_index,
                display_title: title.clone(),
                author,
                width: item.width,
                height: item.height,
                is_story: item.is_story,
                has_audio: item.has_audio,
                preview_available: !analysis.analysis_id.trim().is_empty()
                    && !item.id.trim().is_empty(),
                title,
                uploader: analysis.uploader.chars().take(256).collect(),
                duration_seconds: item.duration_ms.map(|value| value as f64 / 1000.0),
                thumbnail_url: None,
                formats,
                audio_available: item.has_audio && analysis.items.len() == 1,
            }
        })
        .collect::<Vec<_>>();
    if media.is_empty() && analysis.platform == "twitter" && analysis.content_kind == "text" {
        let post = analysis.twitter_post.as_ref();
        let display_title = post
            .and_then(|post| post.text.as_deref())
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(&analysis.title)
            .chars()
            .take(512)
            .collect();
        media.push(ProjectedMedia {
            media_id: "post".to_string(),
            media_type: "text".to_string(),
            source_index: 0,
            display_title,
            author: post
                .map(|post| post.author_name.as_str())
                .filter(|author| !author.trim().is_empty())
                .unwrap_or(&analysis.uploader)
                .chars()
                .take(256)
                .collect(),
            width: None,
            height: None,
            is_story: false,
            has_audio: false,
            preview_available: false,
            title: analysis.title.chars().take(512).collect(),
            uploader: analysis.uploader.chars().take(256).collect(),
            duration_seconds: None,
            thumbnail_url: None,
            formats: Vec::new(),
            audio_available: false,
        });
    }
    if media.is_empty() {
        return Err(ApiError::new(
            "media_not_found",
            "Bu sayfada indirilebilir medya bulunamadı.",
        ));
    }
    Ok(ProjectedAnalysis {
        site: analysis.platform.chars().take(32).collect(),
        media,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        hello_response, negotiate_protocol, parse_request, project_ytdlp_analysis,
        validate_source_payload, Command, SourceCandidate, SourcePayload,
    };

    fn payload(page_url: &str) -> SourcePayload {
        SourcePayload {
            page_url: page_url.to_string(),
            frame_url: None,
            media_type: Some("video".to_string()),
            candidates: Vec::new(),
        }
    }

    #[test]
    fn source_validation_rejects_non_web_credentials_and_candidate_overflow() {
        for invalid in [
            "file:///C:/secret.txt",
            "blob:https://www.youtube.com/id",
            "https://user:pass@www.youtube.com/watch?v=1",
        ] {
            let error = validate_source_payload(payload(invalid)).expect_err(invalid);
            assert_eq!(error.code, "invalid_request");
        }

        let mut too_many = payload("https://www.youtube.com/watch?v=1");
        too_many.candidates = (0..9)
            .map(|index| SourceCandidate {
                candidate_url: Some(format!("https://cdn.example/video-{index}.mp4")),
                detected_by: "dom_current_src".to_string(),
                media_type: Some("video".to_string()),
                duration_seconds: None,
                width: None,
                height: None,
                playing: false,
                visible: true,
                live: false,
            })
            .collect();
        let error = validate_source_payload(too_many).expect_err("nine candidates must fail");
        assert_eq!(error.code, "invalid_request");
    }

    #[test]
    fn supported_frame_is_the_only_fallback_and_candidates_never_become_primary() {
        let mut source = payload("https://example.com/article");
        source.frame_url = Some("https://www.youtube.com/embed/abc".to_string());
        source.candidates.push(SourceCandidate {
            candidate_url: Some("https://cdn.example/private.mp4?token=canary".to_string()),
            detected_by: "context_menu_src".to_string(),
            media_type: Some("video".to_string()),
            duration_seconds: Some(30.0),
            width: Some(1280),
            height: Some(720),
            playing: true,
            visible: true,
            live: false,
        });

        let validated = validate_source_payload(source).expect("supported frame fallback");
        assert_eq!(validated.analysis_url, "https://www.youtube.com/embed/abc");
        assert_eq!(validated.site, "youtube");

        let mut unsupported = payload("https://example.com/article");
        unsupported.candidates.push(SourceCandidate {
            candidate_url: Some("https://cdn.example/video.mp4".to_string()),
            detected_by: "context_menu_src".to_string(),
            media_type: Some("video".to_string()),
            duration_seconds: None,
            width: None,
            height: None,
            playing: true,
            visible: true,
            live: false,
        });
        let error = validate_source_payload(unsupported).expect_err("candidate is only a hint");
        assert_eq!(error.code, "unsupported_source");
    }

    #[test]
    fn ytdlp_projection_keeps_format_metadata_but_removes_download_secrets() {
        let raw = json!({
            "id": "abc",
            "title": "Example",
            "uploader": "Uploader",
            "duration": 12.5,
            "thumbnail": "https://img.example/thumb.jpg",
            "formats": [{
                "format_id": "137",
                "ext": "mp4",
                "vcodec": "avc1.640028",
                "acodec": "none",
                "protocol": "https",
                "width": 1920,
                "height": 1080,
                "filesize": 1234,
                "url": "https://cdn.example/video.mp4?token=SECRET_CANARY",
                "http_headers": {"Authorization": "Bearer SECRET_CANARY"},
                "fragments": [{"url": "https://cdn.example/segment?token=SECRET_CANARY"}]
            }]
        });

        let projection = project_ytdlp_analysis("youtube", &raw).expect("projection");
        let serialized = serde_json::to_string(&projection).expect("serialize projection");
        assert!(!serialized.contains("SECRET_CANARY"));
        assert!(!serialized.contains("http_headers"));
        assert!(!serialized.contains("fragments"));
        assert_eq!(projection.media[0].formats[0].format_id, "137");
        assert_eq!(projection.media[0].formats[0].height, Some(1080));
    }

    #[test]
    fn projection_bounds_entries_formats_and_text() {
        let formats = (0..150)
            .map(|index| {
                json!({
                    "format_id": format!("f{index}"),
                    "ext": "mp4",
                    "vcodec": "avc1",
                    "acodec": "none",
                    "protocol": "https"
                })
            })
            .collect::<Vec<_>>();
        let entries = (0..25)
            .map(|index| {
                json!({
                    "id": format!("m{index}"),
                    "title": "x".repeat(700),
                    "formats": formats
                })
            })
            .collect::<Vec<_>>();

        let projection = project_ytdlp_analysis("youtube", &json!({"entries": entries}))
            .expect("bounded projection");
        assert_eq!(projection.media.len(), 20);
        assert_eq!(projection.media[0].formats.len(), 128);
        assert_eq!(projection.media[0].title.chars().count(), 512);
    }

    #[test]
    fn wire_request_requires_exact_shape_uuid_origin_and_size() {
        let valid = json!({
            "messageType": "request",
            "protocolVersion": 1,
            "requestId": "f8f1374f-5513-4ec4-bf2b-4111b914fc4a",
            "command": "hello",
            "clientOrigin": "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
            "payload": {"supportedProtocol":{"min":1,"max":1}}
        });
        let parsed =
            parse_request(serde_json::to_vec(&valid).unwrap().as_slice()).expect("valid request");
        assert_eq!(parsed.command, Command::Hello);

        for (field, replacement) in [
            ("requestId", json!("not-a-uuid")),
            ("clientOrigin", json!("chrome-extension://*/")),
            ("messageType", json!("response")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = replacement;
            let error = parse_request(&serde_json::to_vec(&invalid).unwrap()).expect_err(field);
            assert_eq!(error.code, "invalid_request");
        }

        let oversized = vec![b' '; super::MAX_REQUEST_BYTES + 1];
        assert_eq!(
            parse_request(&oversized).unwrap_err().code,
            "message_too_large"
        );
    }

    #[test]
    fn protocol_negotiation_uses_highest_common_version_or_fails_explicitly() {
        assert_eq!(negotiate_protocol(1, 1).unwrap(), 1);
        assert_eq!(negotiate_protocol(0, 2).unwrap(), 1);
        assert_eq!(
            negotiate_protocol(2, 3).unwrap_err().code,
            "version_mismatch"
        );
        assert_eq!(
            negotiate_protocol(2, 1).unwrap_err().code,
            "invalid_request"
        );
    }

    #[test]
    fn hello_response_negotiates_before_urls_and_advertises_only_implemented_commands() {
        let request = parse_request(
            &serde_json::to_vec(&json!({
                "messageType": "request",
                "protocolVersion": 1,
                "requestId": "f8f1374f-5513-4ec4-bf2b-4111b914fc4a",
                "command": "hello",
                "clientOrigin": "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                "payload": {"supportedProtocol":{"min":1,"max":1},"extensionVersion":env!("CARGO_PKG_VERSION")}
            }))
            .unwrap(),
        )
        .unwrap();
        let response = hello_response(&request).expect("compatible hello");
        assert_eq!(response.status, "accepted");
        assert!(!super::hello_requires_extension_refresh(&response));
        assert_eq!(response.payload["selectedProtocol"], 1);
        assert_eq!(response.payload["appVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(response.capabilities.analyze_source, true);
        assert_eq!(response.capabilities.preview_media, true);
        assert_eq!(response.capabilities.open_advanced, true);
        assert_eq!(response.capabilities.open_downloads, true);
        assert_eq!(response.capabilities.reveal_error_report, true);
        assert_eq!(response.capabilities.start_download, true);
        assert_eq!(response.capabilities.start_clip, true);
        assert_eq!(response.capabilities.start_media_batch, true);
        assert_eq!(response.capabilities.start_post_export, true);
        assert!(serde_json::to_vec(&response).unwrap().len() < super::MAX_RESPONSE_BYTES);
    }

    #[test]
    fn hello_response_requests_extension_reload_when_bundled_files_changed() {
        let request = parse_request(
            &serde_json::to_vec(&json!({
                "messageType": "request",
                "protocolVersion": 1,
                "requestId": "f8f1374f-5513-4ec4-bf2b-4111b914fc4a",
                "command": "hello",
                "clientOrigin": "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
                "payload": {"supportedProtocol":{"min":1,"max":1},"extensionVersion":"0.9.0"}
            }))
            .unwrap(),
        )
        .unwrap();

        let response = hello_response(&request).expect("version mismatch response");

        assert_eq!(response.status, "version_mismatch");
        assert!(super::hello_requires_extension_refresh(&response));
        assert_eq!(
            response.payload["expectedExtensionVersion"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            response.error.and_then(|error| error.action).as_deref(),
            Some("reload_extension")
        );
    }

    #[test]
    fn social_projection_exposes_identity_not_registry_download_urls() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(json!({
            "analysisId":"analysis-1",
            "expiresAtMs":999999,
            "platform":"instagram",
            "contentKind":"video",
            "title":"Post",
            "uploader":"Creator",
            "items":[{
                "id":"item-1",
                "type":"video",
                "sourceIndex":0,
                "previewUrl":"https://cdn.example/video.mp4?token=SECRET_CANARY",
                "posterRef":"https://images.example/video-poster.jpg",
                "audioUrl":"https://cdn.example/audio.m4a?token=SECRET_CANARY",
                "width":720,
                "height":1280,
                "extension":"mp4",
                "isStory":false,
                "durationMs":5000,
                "hasAudio":true,
                "title":"Clip",
                "authorName":"Creator",
                "authorHandle":"creator"
            }],
            "videoInfo":null
        }))
        .unwrap();

        let projection = super::project_media_analysis(&analysis).expect("social projection");
        let serialized = serde_json::to_string(&projection).unwrap();
        assert!(!serialized.contains("SECRET_CANARY"));
        assert_eq!(projection.media[0].media_id, "item-1");
        assert_eq!(projection.media[0].media_type, "video");
        assert_eq!(projection.media[0].source_index, 0);
        assert_eq!(projection.media[0].display_title, "Clip");
        assert_eq!(projection.media[0].author, "Creator");
        assert_eq!(projection.media[0].width, Some(720));
        assert_eq!(projection.media[0].height, Some(1280));
        assert!(!projection.media[0].is_story);
        assert!(projection.media[0].has_audio);
        assert!(projection.media[0].preview_available);
        assert_eq!(projection.media[0].duration_seconds, Some(5.0));
        assert!(projection.media[0].thumbnail_url.is_none());
        assert!(projection.media[0].audio_available);
    }

    #[test]
    fn instagram_story_placeholder_uses_the_author_name() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(json!({
            "analysisId":"analysis-story",
            "expiresAtMs":999999,
            "platform":"instagram",
            "contentKind":"story",
            "title":"Instagram medyasi",
            "uploader":"NASA",
            "author":{"name":"NASA","handle":"nasa"},
            "items":[{
                "id":"story-1",
                "type":"photo",
                "sourceIndex":0,
                "previewUrl":"https://cdn.example/story.jpg",
                "width":1080,
                "height":1920,
                "extension":"jpg",
                "isStory":true,
                "title":"Instagram medyasi"
            }],
            "videoInfo":null
        }))
        .unwrap();

        assert_eq!(
            super::media_display_title(&analysis, &analysis.items[0]),
            "NASA hikayesi"
        );
    }

    #[test]
    fn twitter_projection_uses_post_text_instead_of_an_opaque_media_id() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(json!({
            "analysisId":"analysis-twitter",
            "expiresAtMs":999999,
            "platform":"twitter",
            "contentKind":"video",
            "title":"RkzNjf64SZ6X35d-",
            "uploader":"MediaDrop",
            "items":[{
                "id":"RkzNjf64SZ6X35d-",
                "type":"video",
                "sourceIndex":0,
                "previewUrl":"https://video.twimg.com/video.mp4",
                "width":720,
                "height":1280,
                "extension":"mp4",
                "isStory":false,
                "durationMs":42000,
                "hasAudio":true,
                "title":"RkzNjf64SZ6X35d-",
                "authorName":"Gerçek Yazar",
                "authorHandle":"gercek",
                "text":"Bu gerçek gönderi başlığıdır"
            }],
            "videoInfo":null
        }))
        .unwrap();

        let projection = super::project_media_analysis(&analysis).expect("twitter projection");
        assert_eq!(projection.media[0].title, "Bu gerçek gönderi başlığıdır");
    }

    #[test]
    fn quoted_text_post_projects_a_popup_card_without_fake_download_formats() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(json!({
            "analysisId":"analysis-quote",
            "expiresAtMs":999999,
            "platform":"twitter",
            "contentKind":"text",
            "title":"Dış tweet",
            "uploader":"Dış Yazar",
            "items":[],
            "twitterQuote":{
                "outer":{"id":"2","authorName":"Dış Yazar","authorHandle":"dis","text":"Dış tweet"},
                "quoted":{"id":"1","authorName":"Alıntı Yazar","authorHandle":"alinti","text":"Alıntı tweet"},
                "quotedMediaIndexes":[]
            },
            "videoInfo":null
        }))
        .unwrap();

        let projection = super::project_media_analysis(&analysis).expect("quote projection");
        assert_eq!(projection.media.len(), 1);
        assert_eq!(projection.media[0].title, "Dış tweet");
        assert!(projection.media[0].formats.is_empty());
        assert!(!projection.media[0].audio_available);
    }

    #[test]
    fn text_post_projection_uses_preserved_post_identity() {
        let analysis: crate::MediaAnalysis = serde_json::from_value(json!({
            "analysisId":"analysis-text",
            "expiresAtMs":999999,
            "platform":"twitter",
            "contentKind":"text",
            "title":"X medyasi",
            "uploader":"X/Twitter",
            "items":[],
            "twitterPost":{
                "id":"2090883745628704991",
                "authorName":"NASA",
                "authorHandle":"NASA",
                "text":"The word attitude here refers to spacecraft position."
            },
            "videoInfo":null
        }))
        .unwrap();

        let projection = super::project_media_analysis(&analysis).expect("text projection");
        assert_eq!(projection.media.len(), 1);
        assert_eq!(projection.media[0].media_id, "post");
        assert_eq!(projection.media[0].media_type, "text");
        assert_eq!(
            projection.media[0].display_title,
            "The word attitude here refers to spacecraft position."
        );
        assert_eq!(projection.media[0].author, "NASA");
        assert!(projection.media[0].formats.is_empty());
    }
}
