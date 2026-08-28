#[derive(Clone, Default)]
pub(crate) struct CanonicalInstagramIdentity {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) handle: Option<String>,
    pub(crate) avatar_url: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaItem {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) item_type: String,
    pub(crate) source_index: usize,
    #[serde(default, skip_serializing)]
    pub(crate) preview_url: String,
    #[serde(default, skip_serializing)]
    pub(crate) audio_url: Option<String>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) extension: String,
    pub(crate) is_story: bool,
    #[serde(default)]
    pub(crate) taken_at_ms: Option<u64>,
    #[serde(default)]
    pub(crate) duration_ms: Option<u64>,
    #[serde(default)]
    pub(crate) has_audio: bool,
    #[serde(default)]
    pub(crate) preview_ref: Option<String>,
    #[serde(default)]
    pub(crate) poster_ref: Option<String>,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) author_id: Option<String>,
    pub(crate) author_name: Option<String>,
    pub(crate) author_handle: Option<String>,
    pub(crate) avatar_url: Option<String>,
    #[serde(default)]
    pub(crate) avatar_data_url: Option<String>,
    #[serde(default, skip)]
    pub(crate) canonical_instagram_identity: Option<CanonicalInstagramIdentity>,
    pub(crate) text: Option<String>,
    pub(crate) display_date: Option<String>,
    pub(crate) reply_count: Option<u64>,
    pub(crate) retweet_count: Option<u64>,
    pub(crate) like_count: Option<u64>,
    pub(crate) view_count: Option<u64>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TwitterPostMetadata {
    pub(crate) id: String,
    pub(crate) author_name: String,
    pub(crate) author_handle: String,
    #[serde(default)]
    pub(crate) avatar_url: Option<String>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) display_date: Option<String>,
    #[serde(default)]
    pub(crate) is_verified: bool,
    #[serde(default)]
    pub(crate) reply_count: Option<u64>,
    #[serde(default)]
    pub(crate) retweet_count: Option<u64>,
    #[serde(default)]
    pub(crate) like_count: Option<u64>,
    #[serde(default)]
    pub(crate) view_count: Option<u64>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TwitterQuoteContext {
    pub(crate) outer: TwitterPostMetadata,
    pub(crate) quoted: TwitterPostMetadata,
    #[serde(default)]
    pub(crate) quoted_media_indexes: Vec<usize>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthorIdentity {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) handle: String,
    #[serde(default)]
    pub(crate) avatar_data_url: Option<String>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstagramAuthorDiagnostics {
    #[serde(default)]
    pub(crate) author_source: String,
    #[serde(default)]
    pub(crate) identity_matched: bool,
    #[serde(default)]
    pub(crate) avatar_present: bool,
    #[serde(default)]
    pub(crate) host_class: String,
    #[serde(default)]
    pub(crate) http_status: Option<u16>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaAnalysis {
    #[serde(default)]
    pub(crate) analysis_id: String,
    #[serde(default)]
    pub(crate) expires_at_ms: u128,
    pub(crate) platform: String,
    pub(crate) content_kind: String,
    pub(crate) title: String,
    pub(crate) uploader: String,
    #[serde(default)]
    pub(crate) author: AuthorIdentity,
    pub(crate) items: Vec<MediaItem>,
    #[serde(default)]
    pub(crate) initial_index: usize,
    #[serde(default)]
    pub(crate) requested_item_id: Option<String>,
    #[serde(default)]
    pub(crate) warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instagram_diagnostics: Option<InstagramAuthorDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) twitter_quote: Option<TwitterQuoteContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) twitter_post: Option<TwitterPostMetadata>,
    pub(crate) video_info: Option<serde_json::Value>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaDownloadFile {
    pub(crate) file_path: String,
    pub(crate) file_size: u64,
    pub(crate) title: String,
    pub(crate) source_index: usize,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaDownloadFailure {
    pub(crate) item_id: String,
    pub(crate) source_index: usize,
    pub(crate) message: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaDownloadResult {
    pub(crate) message: String,
    pub(crate) files: Vec<MediaDownloadFile>,
    pub(crate) output_dir: String,
    pub(crate) downloaded_count: usize,
    pub(crate) failed_count: usize,
    pub(crate) failures: Vec<MediaDownloadFailure>,
    pub(crate) mode: String,
}
