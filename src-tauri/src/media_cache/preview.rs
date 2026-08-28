use crate::*;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, REFERER, USER_AGENT};
use reqwest::redirect::Policy;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::net::ToSocketAddrs;

const PREVIEW_CACHE_DIR: &str = "media-previews";
const PREVIEW_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;
const PREVIEW_CACHE_TTL_MS: u128 = 30 * 60 * 1000;
const PREVIEW_PART_STALE_MS: u128 = 10 * 60 * 1000;
const PREVIEW_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const PREVIEW_DOWNLOAD_REDIRECTS: usize = 4;
const PREVIEW_MAGIC_PREFIX_BYTES: usize = 64;
const PREVIEW_BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

static PREVIEW_CACHE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn preview_cache_lock() -> &'static Mutex<()> {
    PREVIEW_CACHE_LOCK.get_or_init(|| Mutex::new(()))
}

fn preview_ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.octets()[0] == 0)
        }
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|mapped| preview_ip_is_public(IpAddr::V4(mapped)))
            .unwrap_or_else(|| {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_multicast())
            }),
    }
}

pub(crate) fn validate_preview_url_with_resolver<F>(
    url: &reqwest::Url,
    resolve: F,
) -> Result<(), String>
where
    F: FnOnce(&str, u16) -> Result<Vec<IpAddr>, String>,
{
    if url.scheme() != "https" {
        return Err("Onizleme hedefi HTTPS kullanmiyor.".to_string());
    }
    let host = url
        .host_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Onizleme hedefinin hostname bilgisi yok.".to_string())?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = resolve(host, port)?;
    if addresses.is_empty() {
        return Err("Onizleme hedefi hicbir IP adresine cozumlenemedi.".to_string());
    }
    if let Some(blocked) = addresses
        .iter()
        .copied()
        .find(|ip| !preview_ip_is_public(*ip))
    {
        return Err(format!(
            "Onizleme hedefi yerel veya ozel IP adresine cozumlendi: {blocked}"
        ));
    }
    Ok(())
}

pub(crate) fn validate_preview_url(url: &reqwest::Url) -> Result<(), String> {
    validate_preview_url_with_resolver(url, |host, port| {
        (host, port)
            .to_socket_addrs()
            .map(|addresses| addresses.map(|address| address.ip()).collect::<Vec<_>>())
            .map_err(|error| format!("Onizleme hedefi DNS ile cozumlenemedi: {error}"))
    })
}

fn preview_redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        if attempt.previous().len() >= PREVIEW_DOWNLOAD_REDIRECTS {
            return attempt.error("Onizleme redirect limiti asildi.");
        }
        match validate_preview_url(attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(error) => attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                error,
            )),
        }
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SafePreviewDnsResolver;

impl Resolve for SafePreviewDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let resolved = (host.as_str(), 0)
            .to_socket_addrs()
            .map(|addresses| addresses.collect::<Vec<_>>())
            .and_then(|addresses| {
                if addresses.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Onizleme hedefi hicbir IP adresine cozumlenemedi.",
                    ));
                }
                if let Some(blocked) = addresses
                    .iter()
                    .find(|address| !preview_ip_is_public(address.ip()))
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "Onizleme hedefi yerel veya ozel IP adresine cozumlendi: {}",
                            blocked.ip()
                        ),
                    ));
                }
                Ok(addresses)
            });

        Box::pin(async move {
            match resolved {
                Ok(addresses) => Ok(Box::new(addresses.into_iter()) as Addrs),
                Err(error) => Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>),
            }
        })
    }
}

fn preview_http_client() -> Result<reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();

    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .dns_resolver(Arc::new(SafePreviewDnsResolver))
                .redirect(preview_redirect_policy())
                .timeout(PREVIEW_DOWNLOAD_TIMEOUT)
                .build()
                .map_err(|error| format!("Onizleme HTTP istemcisi hazirlanamadi: {error}"))
        })
        .clone()
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedMediaPreview {
    pub(crate) file_path: String,
    pub(crate) media_type: String,
    pub(crate) has_audio: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CachePolicyEntry {
    key: String,
    size: u64,
    last_access_ms: u128,
}

#[derive(Clone, Debug)]
struct DiskCacheEntry {
    policy: CachePolicyEntry,
    path: PathBuf,
}

fn select_cache_evictions(
    entries: &[CachePolicyEntry],
    now_ms: u128,
    incoming_bytes: u64,
    max_bytes: u64,
    ttl_ms: u128,
    protected_key: Option<&str>,
) -> Vec<String> {
    let mut candidates = entries
        .iter()
        .filter(|entry| protected_key != Some(entry.key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|entry| (entry.last_access_ms, entry.key.clone()));

    let mut remaining_bytes = entries.iter().map(|entry| entry.size).sum::<u64>();
    let mut selected = Vec::new();
    let mut selected_keys = HashSet::new();

    for entry in &candidates {
        if now_ms.saturating_sub(entry.last_access_ms) <= ttl_ms {
            continue;
        }
        selected.push(entry.key.clone());
        selected_keys.insert(entry.key.clone());
        remaining_bytes = remaining_bytes.saturating_sub(entry.size);
    }
    for entry in &candidates {
        if remaining_bytes.saturating_add(incoming_bytes) <= max_bytes {
            break;
        }
        if selected_keys.insert(entry.key.clone()) {
            selected.push(entry.key.clone());
            remaining_bytes = remaining_bytes.saturating_sub(entry.size);
        }
    }
    selected
}

fn preview_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("Uygulama onbellek klasoru bulunamadi: {error}"))?;
    Ok(root.join(PREVIEW_CACHE_DIR))
}

fn cache_key(analysis_id: &str, item_id: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    analysis_id.hash(&mut hasher);
    item_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn access_path(media_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.access", media_path.to_string_lossy()))
}

fn entry_last_access_ms(path: &Path) -> u128 {
    fs::read_to_string(access_path(path))
        .ok()
        .and_then(|value| value.trim().parse::<u128>().ok())
        .or_else(|| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .map(|value| {
                    value
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                })
        })
        .unwrap_or(0)
}

fn touch_entry(path: &Path) {
    let _ = fs::write(access_path(path), now_ms().to_string());
}

fn is_cache_media_file(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif" | "mp4" | "m4v" | "mov" | "webm"
        )
}

fn scan_cache_entries(cache_dir: &Path) -> Vec<DiskCacheEntry> {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !is_cache_media_file(&path) {
                return None;
            }
            let metadata = fs::metadata(&path).ok()?;
            Some(DiskCacheEntry {
                policy: CachePolicyEntry {
                    key: entry.file_name().to_string_lossy().into_owned(),
                    size: metadata.len(),
                    last_access_ms: entry_last_access_ms(&path),
                },
                path,
            })
        })
        .collect()
}

fn remove_cache_entry(entry: &DiskCacheEntry) {
    let _ = fs::remove_file(&entry.path);
    let _ = fs::remove_file(access_path(&entry.path));
}

fn partial_file_is_stale(modified_at_ms: u128, now_ms: u128) -> bool {
    now_ms.saturating_sub(modified_at_ms) > PREVIEW_PART_STALE_MS
}

fn remove_stale_partial_files(cache_dir: &Path, current_time_ms: u128) {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().contains(".part-") {
            continue;
        }
        let path = entry.path();
        let Some(modified_at_ms) = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(|modified| {
                modified
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            })
        else {
            continue;
        };
        if partial_file_is_stale(modified_at_ms, current_time_ms) {
            let _ = fs::remove_file(path);
        }
    }
}

fn enforce_cache_policy(
    cache_dir: &Path,
    incoming_bytes: u64,
    protected_path: Option<&Path>,
) -> Result<(), String> {
    if incoming_bytes > PREVIEW_CACHE_MAX_BYTES {
        return Err("Onizleme medyasi 256 MiB onbellek sinirini asiyor.".to_string());
    }
    let entries = scan_cache_entries(cache_dir);
    let protected_key = protected_path
        .and_then(Path::file_name)
        .and_then(OsStr::to_str);
    let policy_entries = entries
        .iter()
        .map(|entry| entry.policy.clone())
        .collect::<Vec<_>>();
    let evictions = select_cache_evictions(
        &policy_entries,
        now_ms(),
        incoming_bytes,
        PREVIEW_CACHE_MAX_BYTES,
        PREVIEW_CACHE_TTL_MS,
        protected_key,
    )
    .into_iter()
    .collect::<HashSet<_>>();
    for entry in &entries {
        if evictions.contains(&entry.policy.key) {
            remove_cache_entry(entry);
        }
    }
    let remaining = scan_cache_entries(cache_dir)
        .iter()
        .map(|entry| entry.policy.size)
        .sum::<u64>();
    if remaining.saturating_add(incoming_bytes) > PREVIEW_CACHE_MAX_BYTES {
        return Err("Onizleme onbelleginde yeterli alan acilamadi.".to_string());
    }
    Ok(())
}

fn find_cached_entry(cache_dir: &Path, key: &str) -> Option<PathBuf> {
    scan_cache_entries(cache_dir)
        .into_iter()
        .find(|entry| {
            entry
                .path
                .file_stem()
                .and_then(OsStr::to_str)
                .is_some_and(|stem| stem == key)
        })
        .map(|entry| entry.path)
}

fn find_valid_cached_entry(cache_dir: &Path, key: &str, item: &MediaItem) -> Option<PathBuf> {
    let path = find_cached_entry(cache_dir, key)?;
    if cached_entry_is_valid(&path, item) {
        return Some(path);
    }
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(access_path(&path));
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetectedPreviewMedia {
    Jpeg,
    Png,
    Webp,
    Gif,
    Avif,
    Mp4,
    Webm,
}

impl DetectedPreviewMedia {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Avif => "avif",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    fn is_video(self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm)
    }

    fn matches_mime(self, mime: &str) -> bool {
        match self {
            Self::Jpeg => matches!(mime, "image/jpeg" | "image/jpg"),
            Self::Png => mime == "image/png",
            Self::Webp => mime == "image/webp",
            Self::Gif => mime == "image/gif",
            Self::Avif => mime == "image/avif",
            Self::Mp4 => matches!(mime, "video/mp4" | "video/quicktime" | "video/x-m4v"),
            Self::Webm => mime == "video/webm",
        }
    }
}

fn normalized_content_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn detect_preview_media_magic(prefix: &[u8]) -> Option<DetectedPreviewMedia> {
    if prefix.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(DetectedPreviewMedia::Jpeg);
    }
    if prefix.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(DetectedPreviewMedia::Png);
    }
    if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        return Some(DetectedPreviewMedia::Webp);
    }
    if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        return Some(DetectedPreviewMedia::Gif);
    }
    if prefix.len() >= 12 && &prefix[4..8] == b"ftyp" {
        let brands = &prefix[8..prefix.len().min(40)];
        if brands
            .windows(4)
            .any(|brand| matches!(brand, b"avif" | b"avis"))
        {
            return Some(DetectedPreviewMedia::Avif);
        }
        return Some(DetectedPreviewMedia::Mp4);
    }
    if prefix.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(DetectedPreviewMedia::Webm);
    }
    None
}

pub(crate) fn validate_declared_media_type(
    item_type: &str,
    content_type: &str,
) -> Result<(), String> {
    let mime = normalized_content_type(content_type);
    if mime.is_empty() || mime == "application/octet-stream" {
        return Ok(());
    }
    let expected_video = item_type == "video";
    let supported = if expected_video {
        matches!(
            mime.as_str(),
            "video/mp4" | "video/quicktime" | "video/x-m4v" | "video/webm"
        )
    } else {
        matches!(
            mime.as_str(),
            "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/gif" | "image/avif"
        )
    };
    supported
        .then_some(())
        .ok_or_else(|| format!("Onizleme sunucusu beklenmeyen MIME turu dondurdu: {mime}"))
}

pub(crate) fn validate_preview_magic(
    item_type: &str,
    content_type: &str,
    prefix: &[u8],
) -> Result<DetectedPreviewMedia, String> {
    let detected = detect_preview_media_magic(prefix)
        .ok_or_else(|| "Onizleme medyasinin dosya imzasi taninamadi.".to_string())?;
    if detected.is_video() != (item_type == "video") {
        return Err(
            "Onizleme medyasinin dosya imzasi beklenen medya turuyle uyusmuyor.".to_string(),
        );
    }
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() || mime == "application/octet-stream" {
        return Ok(detected);
    }
    detected
        .matches_mime(&mime)
        .then_some(detected)
        .ok_or_else(|| format!("Onizleme MIME turu ile dosya imzasi uyusmuyor: {mime}"))
}

pub(crate) fn validate_declared_audio_type(content_type: &str) -> Result<(), String> {
    let mime = normalized_content_type(content_type);
    if mime.is_empty() || mime == "application/octet-stream" {
        return Ok(());
    }
    matches!(
        mime.as_str(),
        "audio/mp4"
            | "audio/x-m4a"
            | "audio/webm"
            | "video/mp4"
            | "video/quicktime"
            | "video/x-m4v"
            | "video/webm"
    )
    .then_some(())
    .ok_or_else(|| format!("Onizleme ses sunucusu beklenmeyen MIME turu dondurdu: {mime}"))
}

pub(crate) fn validate_audio_preview_magic(
    content_type: &str,
    prefix: &[u8],
) -> Result<DetectedPreviewMedia, String> {
    let detected = detect_preview_media_magic(prefix)
        .filter(|media| media.is_video())
        .ok_or_else(|| "Onizleme ses akisi desteklenen bir medya kapsayicisi degil.".to_string())?;
    let mime = normalized_content_type(content_type);
    if mime.is_empty() || mime == "application/octet-stream" {
        return Ok(detected);
    }
    let matches = match detected {
        DetectedPreviewMedia::Mp4 => matches!(
            mime.as_str(),
            "audio/mp4" | "audio/x-m4a" | "video/mp4" | "video/quicktime" | "video/x-m4v"
        ),
        DetectedPreviewMedia::Webm => matches!(mime.as_str(), "audio/webm" | "video/webm"),
        _ => false,
    };
    matches
        .then_some(detected)
        .ok_or_else(|| format!("Onizleme ses MIME turu ile dosya imzasi uyusmuyor: {mime}"))
}

fn separate_audio_url(item: &MediaItem) -> Option<&str> {
    (item.item_type == "video")
        .then_some(item.audio_url.as_deref())
        .flatten()
        .map(str::trim)
        .filter(|url| !url.is_empty() && *url != item.preview_url.trim())
}

fn item_cache_key(analysis_id: &str, item_id: &str, item: &MediaItem) -> String {
    let base = cache_key(analysis_id, item_id);
    if separate_audio_url(item).is_some() {
        format!("{base}-mux-v1")
    } else {
        base
    }
}

fn reusable_cached_preview_path(
    cache_dir: &Path,
    analysis_id: &str,
    item_id: &str,
    item: &MediaItem,
) -> Option<PathBuf> {
    let _guard = preview_cache_lock().lock().ok()?;
    let path =
        find_valid_cached_entry(cache_dir, &item_cache_key(analysis_id, item_id, item), item)?;
    touch_entry(&path);
    Some(path)
}

pub(crate) fn cached_media_preview_path(
    app: &tauri::AppHandle,
    analysis_id: &str,
    item_id: &str,
    item: &MediaItem,
) -> Option<PathBuf> {
    reusable_cached_preview_path(&preview_cache_dir(app).ok()?, analysis_id, item_id, item)
}

fn cached_entry_is_valid(path: &Path, item: &MediaItem) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut prefix = [0_u8; PREVIEW_MAGIC_PREFIX_BYTES];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    validate_preview_magic(&item.item_type, "", &prefix[..read]).is_ok()
}

struct PartialPreviewFile {
    path: PathBuf,
}

impl PartialPreviewFile {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PartialPreviewFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn stream_to_partial<R: Read>(
    mut reader: R,
    path: &Path,
    limit: u64,
) -> Result<(u64, Vec<u8>), String> {
    let mut file = fs::File::create(path)
        .map_err(|error| format!("Onizleme gecici dosyasi olusturulamadi: {error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut prefix = Vec::with_capacity(PREVIEW_MAGIC_PREFIX_BYTES);
    let mut total = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Onizleme medyasi okunamadi: {error}"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > limit {
            return Err("Onizleme medyasi 256 MiB tek oge sinirini asiyor.".to_string());
        }
        if prefix.len() < PREVIEW_MAGIC_PREFIX_BYTES {
            let copy_len = (PREVIEW_MAGIC_PREFIX_BYTES - prefix.len()).min(read);
            prefix.extend_from_slice(&buffer[..copy_len]);
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("Onizleme gecici dosyasina yazilamadi: {error}"))?;
    }
    if total == 0 {
        return Err("Onizleme sunucusu bos medya dondurdu.".to_string());
    }
    file.sync_all()
        .map_err(|error| format!("Onizleme gecici dosyasi kaydedilemedi: {error}"))?;
    Ok((total, prefix))
}

fn promote_partial_no_clobber(
    partial_path: &Path,
    target: &Path,
    item: &MediaItem,
) -> Result<PathBuf, String> {
    if target.exists() {
        return if cached_entry_is_valid(target, item) {
            Ok(target.to_path_buf())
        } else {
            Err("Ayni cache anahtarinda gecersiz bir hedef dosya bulundu.".to_string())
        };
    }
    match fs::hard_link(partial_path, target) {
        Ok(()) => Ok(target.to_path_buf()),
        Err(_) if target.exists() && cached_entry_is_valid(target, item) => {
            Ok(target.to_path_buf())
        }
        Err(error) => Err(format!(
            "Onizleme dosyasi atomik ve no-clobber olarak yerlestirilemedi: {error}"
        )),
    }
}

#[derive(Clone, Copy)]
enum PreviewSourceKind {
    Media,
    Audio,
}

struct PreviewSourceRequest<'a> {
    cache_dir: &'a Path,
    key: &'a str,
    item: &'a MediaItem,
    source_url: &'a str,
    referer: &'a str,
    kind: PreviewSourceKind,
    max_bytes: u64,
}

fn download_preview_source_to_partial(
    request: PreviewSourceRequest<'_>,
) -> Result<(PartialPreviewFile, u64, DetectedPreviewMedia), String> {
    if request.max_bytes == 0 {
        return Err("Onizleme medya ve ses akislari 256 MiB sinirini asiyor.".to_string());
    }
    let source_url = request.source_url.trim();
    if source_url.is_empty() {
        return Err("Kayitli onizleme kaynagi bulunamadi.".to_string());
    }
    let parsed = reqwest::Url::parse(source_url)
        .map_err(|error| format!("Kayitli onizleme adresi gecersiz: {error}"))?;
    validate_preview_url(&parsed)?;

    let client = preview_http_client()?;
    let accept = match request.kind {
        PreviewSourceKind::Media => "video/*,*/*;q=0.8",
        PreviewSourceKind::Audio => "audio/mp4,audio/webm,audio/*,*/*;q=0.8",
    };
    let mut builder = client
        .get(parsed)
        .header(ACCEPT, accept)
        .header(USER_AGENT, PREVIEW_BROWSER_USER_AGENT);
    if !request.referer.trim().is_empty() {
        builder = builder.header(REFERER, request.referer.trim());
    }
    let response = builder
        .send()
        .map_err(|error| format!("Onizleme kaynagi alinamadi: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Onizleme kaynagi HTTP {} ile reddedildi.",
            response.status()
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    match request.kind {
        PreviewSourceKind::Media => {
            validate_declared_media_type(&request.item.item_type, &content_type)?
        }
        PreviewSourceKind::Audio => validate_declared_audio_type(&content_type)?,
    }
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length());
    if content_length.is_some_and(|size| size > request.max_bytes) {
        return Err("Onizleme medya ve ses akislari 256 MiB sinirini asiyor.".to_string());
    }

    let partial_path = request
        .cache_dir
        .join(format!("{}.part-{}", request.key, Uuid::new_v4()));
    let partial = PartialPreviewFile::new(partial_path);
    let (downloaded_bytes, magic_prefix) =
        stream_to_partial(response, partial.path(), request.max_bytes)?;
    let detected = match request.kind {
        PreviewSourceKind::Media => {
            validate_preview_magic(&request.item.item_type, &content_type, &magic_prefix)?
        }
        PreviewSourceKind::Audio => validate_audio_preview_magic(&content_type, &magic_prefix)?,
    };
    Ok((partial, downloaded_bytes, detected))
}

fn download_and_mux_registered_item(
    app: &tauri::AppHandle,
    cache_dir: &Path,
    key: &str,
    item: &MediaItem,
    referer: &str,
    audio_url: &str,
) -> Result<PathBuf, String> {
    let (video, video_bytes, _) = download_preview_source_to_partial(PreviewSourceRequest {
        cache_dir,
        key: &format!("{key}-video"),
        item,
        source_url: &item.preview_url,
        referer,
        kind: PreviewSourceKind::Media,
        max_bytes: PREVIEW_CACHE_MAX_BYTES,
    })?;
    let remaining_bytes = PREVIEW_CACHE_MAX_BYTES
        .checked_sub(video_bytes)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| "Onizleme medya ve ses akislari 256 MiB sinirini asiyor.".to_string())?;
    let (audio, _, _) = download_preview_source_to_partial(PreviewSourceRequest {
        cache_dir,
        key: &format!("{key}-audio"),
        item,
        source_url: audio_url,
        referer,
        kind: PreviewSourceKind::Audio,
        max_bytes: remaining_bytes,
    })?;

    let target = cache_dir.join(format!("{key}.mp4"));
    {
        let _guard = preview_cache_lock()
            .lock()
            .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
        if target.exists() && cached_entry_is_valid(&target, item) {
            touch_entry(&target);
            return Ok(target);
        }
    }

    let ffmpeg = ensure_runtime_tool(app, "ffmpeg")?;
    let ffprobe = ensure_runtime_tool(app, "ffprobe")?;
    if let Err(error) =
        mux_separate_video_audio(&ffmpeg, &ffprobe, video.path(), audio.path(), &target)
    {
        if target.exists() && cached_entry_is_valid(&target, item) {
            touch_entry(&target);
            return Ok(target);
        }
        return Err(format!("Story onizleme sesi birlestirilemedi: {error}"));
    }
    let muxed_bytes = fs::metadata(&target)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("Story onizleme boyutu okunamadi: {error}"))?;
    if muxed_bytes > PREVIEW_CACHE_MAX_BYTES {
        let _ = fs::remove_file(&target);
        return Err("Sesli Story onizlemesi 256 MiB sinirini asiyor.".to_string());
    }

    let _guard = preview_cache_lock()
        .lock()
        .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
    remove_stale_partial_files(cache_dir, now_ms());
    touch_entry(&target);
    enforce_cache_policy(cache_dir, 0, Some(&target))?;
    Ok(target)
}

fn download_registered_item(
    cache_dir: &Path,
    key: &str,
    item: &MediaItem,
    referer: &str,
) -> Result<PathBuf, String> {
    let source_url = item.preview_url.trim();
    if source_url.is_empty() {
        return Err("Medya ogesinin backend onizleme kaynagi bulunamadi.".to_string());
    }
    let parsed = reqwest::Url::parse(source_url)
        .map_err(|error| format!("Kayitli onizleme adresi gecersiz: {error}"))?;
    validate_preview_url(&parsed)?;

    let client = preview_http_client()?;
    let mut request = client
        .get(parsed)
        .header(
            ACCEPT,
            if item.item_type == "video" {
                "video/*,*/*;q=0.8"
            } else {
                "image/avif,image/webp,image/apng,image/*,*/*;q=0.8"
            },
        )
        .header(USER_AGENT, PREVIEW_BROWSER_USER_AGENT);
    if !referer.trim().is_empty() {
        request = request.header(REFERER, referer.trim());
    }
    let response = request
        .send()
        .map_err(|error| format!("Onizleme medyasi alinamadi: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Onizleme medyasi HTTP {} ile reddedildi.",
            response.status()
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    validate_declared_media_type(&item.item_type, &content_type)?;
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length());
    if content_length.is_some_and(|size| size > PREVIEW_CACHE_MAX_BYTES) {
        return Err("Onizleme medyasi 256 MiB tek oge sinirini asiyor.".to_string());
    }

    {
        let _guard = preview_cache_lock()
            .lock()
            .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
        remove_stale_partial_files(cache_dir, now_ms());
        enforce_cache_policy(cache_dir, content_length.unwrap_or(0), None)?;
    }

    let partial_path = cache_dir.join(format!("{key}.part-{}", Uuid::new_v4()));
    let partial = PartialPreviewFile::new(partial_path);
    let (downloaded_bytes, magic_prefix) =
        stream_to_partial(response, partial.path(), PREVIEW_CACHE_MAX_BYTES)?;
    let detected = validate_preview_magic(&item.item_type, &content_type, &magic_prefix)?;
    let target = cache_dir.join(format!("{key}.{}", detected.extension()));

    let _guard = preview_cache_lock()
        .lock()
        .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
    remove_stale_partial_files(cache_dir, now_ms());
    if let Some(existing) = find_valid_cached_entry(cache_dir, key, item) {
        touch_entry(&existing);
        return Ok(existing);
    }
    enforce_cache_policy(cache_dir, downloaded_bytes, Some(&target))?;

    let promoted = promote_partial_no_clobber(partial.path(), &target, item)?;
    touch_entry(&promoted);
    enforce_cache_policy(cache_dir, 0, Some(&promoted))?;
    Ok(promoted)
}

fn prepare_registered_preview_path(
    app: &tauri::AppHandle,
    cache_dir: &Path,
    analysis_id: &str,
    item_id: &str,
    registered: &RegisteredMediaAnalysis,
) -> Result<(PathBuf, MediaItem), String> {
    let item = registered
        .analysis
        .items
        .iter()
        .find(|item| item.id == item_id)
        .cloned()
        .ok_or_else(|| "Medya ogesi analiz kaydinda bulunamadi.".to_string())?;
    let key = item_cache_key(analysis_id, item_id, &item);
    let cached = {
        let _guard = preview_cache_lock()
            .lock()
            .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
        remove_stale_partial_files(cache_dir, now_ms());
        enforce_cache_policy(cache_dir, 0, None)?;
        find_valid_cached_entry(cache_dir, &key, &item).inspect(|path| touch_entry(path))
    };
    let path = if let Some(path) = cached {
        path
    } else if let Some(audio_url) = separate_audio_url(&item) {
        download_and_mux_registered_item(
            app,
            cache_dir,
            &key,
            &item,
            &registered.source_url,
            audio_url,
        )?
    } else {
        download_registered_item(cache_dir, &key, &item, &registered.source_url)?
    };
    Ok((path, item))
}

fn companion_thumbnail_item(item: &MediaItem) -> MediaItem {
    let mut thumbnail = item.clone();
    let poster = item
        .poster_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if item.item_type != "video" || poster.is_none() {
        return thumbnail;
    }
    thumbnail.item_type = "photo".to_string();
    thumbnail.preview_url = poster.unwrap().to_string();
    thumbnail.audio_url = None;
    thumbnail.has_audio = false;
    thumbnail
}

fn prepare_registered_companion_thumbnail_path(
    app: &tauri::AppHandle,
    cache_dir: &Path,
    analysis_id: &str,
    item_id: &str,
    registered: &RegisteredMediaAnalysis,
) -> Result<(PathBuf, MediaItem), String> {
    let original = registered
        .analysis
        .items
        .iter()
        .find(|item| item.id == item_id)
        .cloned()
        .ok_or_else(|| "Medya ogesi analiz kaydinda bulunamadi.".to_string())?;
    let thumbnail = companion_thumbnail_item(&original);
    if thumbnail.item_type == "photo" && original.item_type == "video" {
        let key = format!("{}-companion-thumb-v1", cache_key(analysis_id, item_id));
        let cached = {
            let _guard = preview_cache_lock()
                .lock()
                .map_err(|_| "Onizleme onbellek kilidi alinamadi.".to_string())?;
            remove_stale_partial_files(cache_dir, now_ms());
            enforce_cache_policy(cache_dir, 0, None)?;
            find_valid_cached_entry(cache_dir, &key, &thumbnail).inspect(|path| touch_entry(path))
        };
        if let Some(path) = cached {
            return Ok((path, thumbnail));
        }
        if let Ok(path) =
            download_registered_item(cache_dir, &key, &thumbnail, &registered.source_url)
        {
            return Ok((path, thumbnail));
        }
    }
    prepare_registered_preview_path(app, cache_dir, analysis_id, item_id, registered)
}

fn prepare_media_preview_blocking_with_mode(
    app: &tauri::AppHandle,
    analysis_id: &str,
    item_id: &str,
    companion_thumbnail: bool,
) -> Result<PreparedMediaPreview, String> {
    let analysis_id = analysis_id.trim();
    let item_id = item_id.trim();
    if analysis_id.is_empty() || item_id.is_empty() {
        return Err("Onizleme icin analysisId ve itemId zorunludur.".to_string());
    }
    let cache_dir = preview_cache_dir(app)?;
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Onizleme onbellek klasoru olusturulamadi: {error}"))?;
    let registered = registered_media_analysis(analysis_id)?;
    let prepare = |registered: &RegisteredMediaAnalysis| {
        if companion_thumbnail {
            prepare_registered_companion_thumbnail_path(
                app,
                &cache_dir,
                analysis_id,
                item_id,
                registered,
            )
        } else {
            prepare_registered_preview_path(app, &cache_dir, analysis_id, item_id, registered)
        }
    };
    let (path, item) = match prepare(&registered) {
        Ok(prepared) => prepared,
        Err(error) if media_error_allows_registry_refresh(&error, 0) => {
            let refreshed = refresh_registered_media_analysis(app, analysis_id)?;
            prepare(&refreshed)?
        }
        Err(error) => return Err(error),
    };
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("Onizleme dosya yolu dogrulanamadi: {error}"))?;
    let canonical_root = fs::canonicalize(&cache_dir)
        .map_err(|error| format!("Onizleme onbellek yolu dogrulanamadi: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("Onizleme dosyasi izin verilen onbellek disinda.".to_string());
    }
    Ok(PreparedMediaPreview {
        file_path: path.to_string_lossy().into_owned(),
        media_type: if item.item_type == "video" {
            "video".to_string()
        } else {
            "photo".to_string()
        },
        has_audio: item.has_audio,
    })
}

pub(crate) fn prepare_media_preview_blocking(
    app: &tauri::AppHandle,
    analysis_id: &str,
    item_id: &str,
) -> Result<PreparedMediaPreview, String> {
    prepare_media_preview_blocking_with_mode(app, analysis_id, item_id, false)
}

pub(crate) fn prepare_companion_media_preview_blocking(
    app: &tauri::AppHandle,
    analysis_id: &str,
    item_id: &str,
) -> Result<PreparedMediaPreview, String> {
    prepare_media_preview_blocking_with_mode(app, analysis_id, item_id, true)
}

#[tauri::command]
pub(crate) async fn prepare_media_preview(
    app: tauri::AppHandle,
    analysis_id: String,
    item_id: String,
) -> ApiResult<PreparedMediaPreview> {
    let result: Result<PreparedMediaPreview, String> =
        tauri::async_runtime::spawn_blocking(move || {
            prepare_media_preview_blocking(&app, &analysis_id, &item_id)
        })
        .await
        .map_err(|error| {
            ApiError::new(
                "thread_error",
                format!("Medya onizleme hazirlama thread hatasi: {error}"),
            )
        })?;
    result.map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn entry(key: &str, size: u64, last_access_ms: u128) -> CachePolicyEntry {
        CachePolicyEntry {
            key: key.to_string(),
            size,
            last_access_ms,
        }
    }

    fn media_item(item_type: &str) -> MediaItem {
        MediaItem {
            id: "item".to_string(),
            item_type: item_type.to_string(),
            source_index: 0,
            preview_url: "https://cdn.example.test/item".to_string(),
            audio_url: None,
            width: None,
            height: None,
            extension: if item_type == "video" { "mp4" } else { "jpg" }.to_string(),
            is_story: false,
            taken_at_ms: None,
            duration_ms: None,
            has_audio: item_type == "video",
            preview_ref: None,
            poster_ref: None,
            title: "item".to_string(),
            author_id: None,
            author_name: None,
            author_handle: None,
            avatar_url: None,
            avatar_data_url: None,
            canonical_instagram_identity: None,
            text: None,
            display_date: None,
            reply_count: None,
            retweet_count: None,
            like_count: None,
            view_count: None,
        }
    }

    #[test]
    fn companion_thumbnail_prefers_a_registered_video_poster() {
        let mut item = media_item("video");
        item.poster_ref = Some("https://images.example.test/poster.jpg".to_string());
        item.audio_url = Some("https://cdn.example.test/audio.m4a".to_string());

        let thumbnail = companion_thumbnail_item(&item);

        assert_eq!(thumbnail.item_type, "photo");
        assert_eq!(
            thumbnail.preview_url,
            "https://images.example.test/poster.jpg"
        );
        assert!(thumbnail.audio_url.is_none());
        assert!(!thumbnail.has_audio);
        assert_eq!(
            companion_thumbnail_item(&media_item("photo")).item_type,
            "photo"
        );
    }

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mediadrop-preview-{label}-{}", Uuid::new_v4()))
    }

    #[test]
    fn cache_policy_removes_expired_entries_even_below_limit() {
        let result = select_cache_evictions(
            &[entry("expired", 10, 100), entry("fresh", 10, 950)],
            1_000,
            0,
            100,
            500,
            None,
        );
        assert_eq!(result, vec!["expired"]);
    }

    #[test]
    fn cache_policy_uses_lru_and_preserves_active_entry() {
        let result = select_cache_evictions(
            &[
                entry("oldest", 40, 700),
                entry("middle", 40, 800),
                entry("active", 40, 600),
            ],
            1_000,
            30,
            100,
            500,
            Some("active"),
        );
        assert_eq!(result, vec!["oldest", "middle"]);
    }

    #[test]
    fn cache_key_uses_analysis_and_item_identity() {
        assert_ne!(
            cache_key("analysis-a", "item"),
            cache_key("analysis-b", "item")
        );
        assert_ne!(
            cache_key("analysis", "item-a"),
            cache_key("analysis", "item-b")
        );
        assert_eq!(cache_key("analysis", "item"), cache_key("analysis", "item"));
    }

    #[test]
    fn separate_audio_uses_a_versioned_mux_cache_key() {
        let mut item = media_item("video");
        let plain = item_cache_key("analysis", "item", &item);
        item.audio_url = Some("https://cdn.example.test/audio.m4a".to_string());
        let muxed = item_cache_key("analysis", "item", &item);
        assert_ne!(plain, muxed);
        assert!(muxed.ends_with("-mux-v1"));

        item.audio_url = Some(item.preview_url.clone());
        assert_eq!(item_cache_key("analysis", "item", &item), plain);
    }

    #[test]
    fn valid_cached_preview_is_available_for_final_download_reuse() {
        let dir = test_dir("reuse");
        fs::create_dir_all(&dir).unwrap();
        let item = media_item("photo");
        let target = dir.join(format!("{}.jpg", item_cache_key("analysis", "item", &item)));
        fs::write(&target, [0xff, 0xd8, 0xff, 0xe0]).unwrap();

        assert_eq!(
            reusable_cached_preview_path(&dir, "analysis", "item", &item),
            Some(target.clone())
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dns_policy_rejects_any_private_or_local_resolution() {
        let url = reqwest::Url::parse("https://cdn.example.test/media").unwrap();
        for blocked in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.10.20",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
            "::",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            let ip = blocked.parse::<IpAddr>().unwrap();
            assert!(validate_preview_url_with_resolver(&url, |_, _| Ok(vec![ip])).is_err());
        }
        assert!(validate_preview_url_with_resolver(&url, |_, _| {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        })
        .is_ok());
        assert!(validate_preview_url_with_resolver(&url, |_, _| {
            Ok(vec![
                "93.184.216.34".parse().unwrap(),
                "127.0.0.1".parse().unwrap(),
            ])
        })
        .is_err());
    }

    #[test]
    fn dns_policy_requires_https_and_a_resolved_address() {
        let http = reqwest::Url::parse("http://cdn.example.test/media").unwrap();
        assert!(validate_preview_url_with_resolver(&http, |_, _| {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        })
        .is_err());
        let https = reqwest::Url::parse("https://cdn.example.test/media").unwrap();
        assert!(validate_preview_url_with_resolver(&https, |_, _| Ok(Vec::new())).is_err());
    }

    #[test]
    fn magic_validation_accepts_images_mp4_and_webm() {
        let photo = media_item("photo");
        assert_eq!(
            validate_preview_magic(&photo.item_type, "image/jpeg", &[0xff, 0xd8, 0xff, 0xe0])
                .unwrap(),
            DetectedPreviewMedia::Jpeg
        );
        assert_eq!(
            validate_preview_magic(
                &photo.item_type,
                "image/png",
                &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
            )
            .unwrap(),
            DetectedPreviewMedia::Png
        );

        let video = media_item("video");
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00\x00\x00isommp42";
        assert_eq!(
            validate_preview_magic(&video.item_type, "video/mp4", mp4).unwrap(),
            DetectedPreviewMedia::Mp4
        );
        assert_eq!(
            validate_preview_magic(
                &video.item_type,
                "video/webm",
                &[0x1a, 0x45, 0xdf, 0xa3, 0x9f, 0x42, 0x86, 0x81]
            )
            .unwrap(),
            DetectedPreviewMedia::Webm
        );
    }

    #[test]
    fn magic_validation_rejects_mime_or_media_kind_mismatch() {
        let photo = media_item("photo");
        let video = media_item("video");
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00\x00\x00isommp42";
        assert!(validate_preview_magic(&photo.item_type, "image/jpeg", mp4).is_err());
        assert!(validate_preview_magic(&video.item_type, "video/webm", mp4).is_err());
        assert!(validate_preview_magic(&photo.item_type, "text/html", b"<html>").is_err());
    }

    #[test]
    fn direct_download_validation_rejects_html_as_photo_media() {
        assert!(validate_declared_media_type("photo", "text/html").is_err());
        assert!(validate_preview_magic("photo", "text/html", b"<html>").is_err());
    }

    #[test]
    fn separate_audio_magic_accepts_mp4_and_webm_containers_only() {
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00\x00\x00isommp42";
        assert_eq!(
            validate_audio_preview_magic("audio/mp4", mp4).unwrap(),
            DetectedPreviewMedia::Mp4
        );
        assert_eq!(
            validate_audio_preview_magic(
                "audio/webm",
                &[0x1a, 0x45, 0xdf, 0xa3, 0x9f, 0x42, 0x86, 0x81]
            )
            .unwrap(),
            DetectedPreviewMedia::Webm
        );
        assert!(validate_audio_preview_magic("text/html", b"<html>").is_err());
        assert!(validate_audio_preview_magic("audio/webm", mp4).is_err());
    }

    #[test]
    fn bounded_stream_aborts_and_partial_guard_cleans_file() {
        let dir = test_dir("bounded");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("item.part-test");
        {
            let partial = PartialPreviewFile::new(path.clone());
            let error =
                stream_to_partial(Cursor::new(vec![0_u8; 17]), partial.path(), 16).unwrap_err();
            assert!(error.contains("sinirini asiyor"));
            assert!(path.exists());
        }
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_cleanup_preserves_fresh_files_and_removes_old_ones() {
        let dir = test_dir("stale");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("item.part-test");
        fs::write(&path, b"partial").unwrap();
        let current = now_ms();
        remove_stale_partial_files(&dir, current);
        assert!(path.exists());
        remove_stale_partial_files(&dir, current + PREVIEW_PART_STALE_MS + 1_000);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn promotion_never_overwrites_an_existing_valid_cache_file() {
        let dir = test_dir("promote");
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("item.jpg");
        let partial = dir.join("item.part-test");
        let winner = [0xff, 0xd8, 0xff, 0x01];
        let loser = [0xff, 0xd8, 0xff, 0x02];
        fs::write(&target, winner).unwrap();
        fs::write(&partial, loser).unwrap();
        let promoted = promote_partial_no_clobber(&partial, &target, &media_item("photo")).unwrap();
        assert_eq!(promoted, target);
        assert_eq!(fs::read(&target).unwrap(), winner);
        let _ = fs::remove_dir_all(dir);
    }
}
