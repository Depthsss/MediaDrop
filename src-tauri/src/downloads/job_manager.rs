use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use crate::core::error::{ApiError, STRUCTURED_ERROR_PREFIX};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DownloadJobStop {
    Pause,
    Cancel,
}

#[derive(Debug, Default)]
struct DownloadJobState {
    active_job_id: Option<String>,
    stop_request: Option<DownloadJobStop>,
    owner: Option<DownloadJobOwner>,
    snapshot: Option<DownloadJobSnapshot>,
    last_snapshot: Option<DownloadJobSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DownloadJobOwner {
    origin: String,
    analysis_request_id: String,
}

#[derive(Clone, Debug)]
struct DownloadJobSnapshot {
    job_id: String,
    owner: Option<DownloadJobOwner>,
    operation_kind: String,
    result: Option<DownloadJobResult>,
    status: String,
    stage: String,
    percent: Option<f64>,
    downloaded_mb: Option<f64>,
    total_mb: Option<f64>,
    speed_mb: Option<f64>,
    updated_at_ms: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DownloadResultKind {
    File,
    Directory,
}

impl DownloadResultKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

#[derive(Clone, Debug)]
struct DownloadJobResult {
    target: PathBuf,
    kind: DownloadResultKind,
    display_name: String,
    file_count: usize,
    failed_count: usize,
    size_bytes: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadJobResultView {
    pub(crate) kind: String,
    pub(crate) display_name: String,
    pub(crate) file_count: usize,
    pub(crate) failed_count: usize,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) can_reveal: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadJobView {
    pub(crate) job_id: String,
    pub(crate) operation_kind: String,
    pub(crate) result: Option<DownloadJobResultView>,
    pub(crate) status: String,
    pub(crate) stage: String,
    pub(crate) percent: Option<f64>,
    pub(crate) downloaded_mb: Option<f64>,
    pub(crate) total_mb: Option<f64>,
    pub(crate) speed_mb: Option<f64>,
    pub(crate) controllable: bool,
    #[serde(skip)]
    pub(crate) owned_by_request: bool,
}

static DOWNLOAD_JOBS: OnceLock<Mutex<DownloadJobState>> = OnceLock::new();

fn jobs() -> &'static Mutex<DownloadJobState> {
    DOWNLOAD_JOBS.get_or_init(|| Mutex::new(DownloadJobState::default()))
}

fn encoded_error(error: ApiError) -> String {
    let json = serde_json::to_string(&error).unwrap_or_else(|_| {
        format!(
            "{{\"code\":\"{}\",\"message\":\"{}\"}}",
            error.code, error.message
        )
    });
    format!("{STRUCTURED_ERROR_PREFIX}{json}")
}

fn busy_error() -> String {
    encoded_error(
        ApiError::new("download_busy", "Başka bir indirme işlemi devam ediyor.")
            .with_retryable(true)
            .with_action("wait_for_active_download"),
    )
}

/// Owns the single active product-level download slot.
///
/// Dropping the guard releases only the slot it acquired, so a stale worker can
/// never clear a newer job after an error or cancellation race.
#[derive(Debug)]
pub(crate) struct DownloadJobGuard {
    job_id: String,
    temp_dir: PathBuf,
}

impl DownloadJobGuard {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn id(&self) -> &str {
        &self.job_id
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }
}

impl Drop for DownloadJobGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
        if let Ok(mut state) = jobs().lock() {
            if state.active_job_id.as_deref() == Some(self.job_id.as_str()) {
                if let Some(mut snapshot) = state.snapshot.take() {
                    if !matches!(
                        snapshot.status.as_str(),
                        "completed" | "error" | "paused" | "cancelled"
                    ) {
                        snapshot.status = match state.stop_request {
                            Some(DownloadJobStop::Pause) => "paused",
                            Some(DownloadJobStop::Cancel) => "cancelled",
                            None => snapshot.status.as_str(),
                        }
                        .to_string();
                    }
                    snapshot.updated_at_ms = now_ms();
                    if snapshot.owner.is_some()
                        || matches!(
                            snapshot.status.as_str(),
                            "completed" | "error" | "paused" | "cancelled"
                        )
                    {
                        state.last_snapshot = Some(snapshot);
                    }
                }
                state.active_job_id = None;
                state.stop_request = None;
                state.owner = None;
            }
        }
    }
}

pub(crate) fn begin_download_job() -> Result<DownloadJobGuard, String> {
    begin_download_job_inner(None)
}

pub(crate) fn begin_download_job_owned(
    origin: &str,
    analysis_request_id: &str,
) -> Result<DownloadJobGuard, String> {
    begin_download_job_inner(Some(DownloadJobOwner {
        origin: origin.to_string(),
        analysis_request_id: analysis_request_id.to_string(),
    }))
}

fn begin_download_job_inner(owner: Option<DownloadJobOwner>) -> Result<DownloadJobGuard, String> {
    let mut state = jobs()
        .lock()
        .map_err(|_| "İndirme iş yöneticisi kilidi alınamadı.".to_string())?;

    if state.active_job_id.is_some() {
        return Err(busy_error());
    }

    let job_id = Uuid::new_v4().to_string();
    let temp_root = download_jobs_temp_root();
    fs::create_dir_all(&temp_root)
        .map_err(|error| format!("İndirme geçici kökü oluşturulamadı: {error}"))?;
    let temp_dir = temp_root.join(&job_id);
    fs::create_dir(&temp_dir)
        .map_err(|error| format!("İndirme geçici klasörü oluşturulamadı: {error}"))?;
    state.active_job_id = Some(job_id.clone());
    state.stop_request = None;
    state.owner = owner.clone();
    state.snapshot = Some(DownloadJobSnapshot {
        job_id: job_id.clone(),
        owner,
        operation_kind: "download".to_string(),
        result: None,
        status: "preparing".to_string(),
        stage: "preparing".to_string(),
        percent: Some(0.0),
        downloaded_mb: None,
        total_mb: None,
        speed_mb: None,
        updated_at_ms: now_ms(),
    });
    Ok(DownloadJobGuard { job_id, temp_dir })
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn progress_stage(phase: &str) -> (&'static str, &'static str) {
    let lower = phase.to_lowercase();
    if lower.contains("kart") && lower.contains("hazırl") {
        ("preparing", "rendering_card")
    } else if lower.contains("doğrulan") || lower.contains("validat") {
        ("validating", "validating")
    } else if [
        "birleştir",
        "dönüştür",
        "işlen",
        "ffmpeg",
        "mp4 oluştur",
        "encode",
    ]
    .iter()
    .any(|token| lower.contains(token))
    {
        ("postprocessing", "postprocessing")
    } else if lower.contains("hazırl") {
        ("preparing", "preparing")
    } else {
        ("downloading", "downloading")
    }
}

pub(crate) fn update_download_job_progress(
    percent: Option<f64>,
    downloaded_mb: Option<f64>,
    total_mb: Option<f64>,
    speed_mb: Option<f64>,
    phase: &str,
) {
    let Ok(mut state) = jobs().lock() else {
        return;
    };
    let Some(snapshot) = state.snapshot.as_mut() else {
        return;
    };
    let (status, stage) = progress_stage(phase);
    snapshot.status = status.to_string();
    snapshot.stage = stage.to_string();
    if let Some(value) = percent.filter(|value| value.is_finite()) {
        snapshot.percent = Some(value.clamp(0.0, 100.0));
    }
    snapshot.downloaded_mb = downloaded_mb.filter(|value| value.is_finite() && *value >= 0.0);
    snapshot.total_mb = total_mb.filter(|value| value.is_finite() && *value >= 0.0);
    snapshot.speed_mb = speed_mb.filter(|value| value.is_finite() && *value >= 0.0);
    snapshot.updated_at_ms = now_ms();
}

pub(crate) fn record_download_job_terminal(job_id: &str, status: &str) {
    if !matches!(status, "completed" | "error" | "paused" | "cancelled") {
        return;
    }
    let Ok(mut state) = jobs().lock() else {
        return;
    };
    let active_terminal = state
        .snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
        .map(|snapshot| {
            snapshot.status = status.to_string();
            snapshot.stage = status.to_string();
            if status == "completed" {
                snapshot.percent = Some(100.0);
            }
            snapshot.updated_at_ms = now_ms();
            snapshot.clone()
        });
    if let Some(snapshot) = active_terminal {
        state.last_snapshot = Some(snapshot);
    } else if let Some(snapshot) = state
        .last_snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
    {
        snapshot.status = status.to_string();
        snapshot.stage = status.to_string();
        if status == "completed" {
            snapshot.percent = Some(100.0);
        }
        snapshot.updated_at_ms = now_ms();
    }
}

pub(crate) fn record_download_job_operation(job_id: &str, operation_kind: &str) {
    let Ok(mut state) = jobs().lock() else {
        return;
    };
    if let Some(snapshot) = state
        .snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
    {
        snapshot.operation_kind = operation_kind.to_string();
        snapshot.updated_at_ms = now_ms();
        return;
    }
    if let Some(snapshot) = state
        .last_snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
    {
        snapshot.operation_kind = operation_kind.to_string();
        snapshot.updated_at_ms = now_ms();
    }
}

pub(crate) fn record_download_job_result(
    job_id: &str,
    target: PathBuf,
    kind: DownloadResultKind,
    display_name: String,
    file_count: usize,
    failed_count: usize,
    size_bytes: Option<u64>,
) {
    let Ok(mut state) = jobs().lock() else {
        return;
    };
    let result = DownloadJobResult {
        target,
        kind,
        display_name,
        file_count,
        failed_count,
        size_bytes,
    };
    if let Some(snapshot) = state
        .snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
    {
        snapshot.result = Some(result);
        snapshot.updated_at_ms = now_ms();
        return;
    }
    if let Some(snapshot) = state
        .last_snapshot
        .as_mut()
        .filter(|snapshot| snapshot.job_id == job_id)
    {
        snapshot.result = Some(result);
        snapshot.updated_at_ms = now_ms();
    }
}

pub(crate) fn download_job_snapshot_for(
    origin: Option<&str>,
    analysis_request_id: Option<&str>,
) -> Option<DownloadJobView> {
    let mut state = jobs().lock().ok()?;
    if state
        .last_snapshot
        .as_ref()
        .is_some_and(|snapshot| now_ms().saturating_sub(snapshot.updated_at_ms) > 30 * 60 * 1000)
    {
        state.last_snapshot = None;
    }
    let snapshot = state.snapshot.as_ref().or(state.last_snapshot.as_ref())?;
    let active = state.active_job_id.as_deref() == Some(snapshot.job_id.as_str());
    let owned_by_request = snapshot.owner.as_ref().is_some_and(|owner| {
        Some(owner.origin.as_str()) == origin
            && Some(owner.analysis_request_id.as_str()) == analysis_request_id
    });
    if !active && !owned_by_request {
        return None;
    }
    let controllable = active && owned_by_request;
    Some(DownloadJobView {
        job_id: snapshot.job_id.clone(),
        operation_kind: snapshot.operation_kind.clone(),
        result: snapshot
            .result
            .as_ref()
            .map(|result| DownloadJobResultView {
                kind: result.kind.as_str().to_string(),
                display_name: result.display_name.clone(),
                file_count: result.file_count,
                failed_count: result.failed_count,
                size_bytes: result.size_bytes,
                can_reveal: result.target.exists(),
            }),
        status: snapshot.status.clone(),
        stage: snapshot.stage.clone(),
        percent: snapshot.percent,
        downloaded_mb: snapshot.downloaded_mb,
        total_mb: snapshot.total_mb,
        speed_mb: snapshot.speed_mb,
        controllable,
        owned_by_request,
    })
}

pub(crate) fn download_job_result_target_for(
    origin: &str,
    analysis_request_id: &str,
    job_id: &str,
) -> Result<PathBuf, String> {
    let state = jobs()
        .lock()
        .map_err(|_| "İndirme iş yöneticisi kilidi alınamadı.".to_string())?;
    let snapshot = state
        .snapshot
        .as_ref()
        .filter(|snapshot| snapshot.job_id == job_id)
        .or_else(|| {
            state
                .last_snapshot
                .as_ref()
                .filter(|snapshot| snapshot.job_id == job_id)
        })
        .ok_or_else(|| {
            encoded_error(ApiError::new(
                "result_missing",
                "İndirilen dosya artık bulunamıyor.",
            ))
        })?;
    let owned = snapshot.owner.as_ref().is_some_and(|owner| {
        owner.origin == origin && owner.analysis_request_id == analysis_request_id
    });
    if !owned {
        return Err(encoded_error(ApiError::new(
            "not_job_owner",
            "Bu indirme sonucu farklı bir MediaDrop oturumuna ait.",
        )));
    }
    let target = snapshot
        .result
        .as_ref()
        .map(|result| result.target.clone())
        .filter(|target| target.exists())
        .ok_or_else(|| {
            encoded_error(ApiError::new(
                "result_missing",
                "İndirilen dosya artık bulunamıyor.",
            ))
        })?;
    Ok(target)
}

pub(crate) fn ensure_download_job_history_owner(
    origin: &str,
    analysis_request_id: &str,
    job_id: &str,
) -> Result<(), String> {
    let state = jobs()
        .lock()
        .map_err(|_| "İndirme iş yöneticisi kilidi alınamadı.".to_string())?;
    let matches = state.last_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.job_id == job_id
            && snapshot.status == "paused"
            && snapshot.owner.as_ref().is_some_and(|owner| {
                owner.origin == origin && owner.analysis_request_id == analysis_request_id
            })
    });
    if matches {
        Ok(())
    } else {
        Err(encoded_error(ApiError::new(
            "not_job_owner",
            "Bu indirme işi devam ettirilemez.",
        )))
    }
}

pub(crate) fn ensure_download_job_owner(
    origin: &str,
    analysis_request_id: &str,
    job_id: &str,
) -> Result<(), String> {
    let state = jobs()
        .lock()
        .map_err(|_| "İndirme iş yöneticisi kilidi alınamadı.".to_string())?;
    let owner_matches = state.active_job_id.as_deref() == Some(job_id)
        && state.owner.as_ref().is_some_and(|owner| {
            owner.origin == origin && owner.analysis_request_id == analysis_request_id
        });
    if owner_matches {
        Ok(())
    } else {
        Err(encoded_error(ApiError::new(
            "not_job_owner",
            "Bu indirme işi farklı bir MediaDrop oturumu tarafından başlatıldı.",
        )))
    }
}

fn download_jobs_temp_root() -> PathBuf {
    std::env::temp_dir().join("mediadrop-jobs")
}

pub(crate) fn current_download_job_id() -> Option<String> {
    jobs()
        .lock()
        .ok()
        .and_then(|state| state.active_job_id.clone())
}

pub(crate) fn request_download_job_stop(
    expected_job_id: &str,
    request: DownloadJobStop,
) -> Result<(), String> {
    let expected_job_id = expected_job_id.trim();
    let mut state = jobs()
        .lock()
        .map_err(|_| "İndirme iş yöneticisi kilidi alınamadı.".to_string())?;
    if expected_job_id.is_empty() || state.active_job_id.as_deref() != Some(expected_job_id) {
        return Err(encoded_error(ApiError::new(
            "download_job_mismatch",
            "Duraklatma veya iptal isteği artık etkin olmayan bir indirmeye ait.",
        )));
    }

    if state.stop_request != Some(DownloadJobStop::Cancel) {
        state.stop_request = Some(request);
    }
    Ok(())
}

pub(crate) fn current_download_job_stop() -> Option<DownloadJobStop> {
    jobs().lock().ok().and_then(|state| state.stop_request)
}

pub(crate) fn cleanup_stale_download_job_dirs(minimum_age: Duration) -> usize {
    let root = download_jobs_temp_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let old_enough = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .map(|modified| now.duration_since(modified).unwrap_or_default() >= minimum_age)
            .unwrap_or(false);
        if old_enough && fs::remove_dir_all(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn only_one_download_job_can_be_active_and_drop_releases_it() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            state.active_job_id = None;
        }

        let first = begin_download_job().expect("first job");
        assert_eq!(current_download_job_id().as_deref(), Some(first.id()));
        let first_temp_dir = first.temp_dir().to_path_buf();
        assert!(first_temp_dir.is_dir());

        let error = begin_download_job().expect_err("second job must be rejected");
        assert!(error.starts_with(STRUCTURED_ERROR_PREFIX));
        assert!(error.contains("download_busy"));

        drop(first);
        assert!(current_download_job_id().is_none());
        assert!(!first_temp_dir.exists());

        let next = begin_download_job().expect("slot released");
        assert_eq!(current_download_job_id().as_deref(), Some(next.id()));
        drop(next);
    }

    #[test]
    fn stop_requests_are_scoped_to_the_active_job_and_cancel_supersedes_pause() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            *state = DownloadJobState::default();
        }

        let job = begin_download_job().expect("job should start");
        assert!(request_download_job_stop("stale-job", DownloadJobStop::Pause).is_err());
        assert_eq!(current_download_job_stop(), None);

        request_download_job_stop(job.id(), DownloadJobStop::Pause)
            .expect("active job should pause");
        assert_eq!(current_download_job_stop(), Some(DownloadJobStop::Pause));

        request_download_job_stop(job.id(), DownloadJobStop::Cancel)
            .expect("cancel should supersede pause");
        assert_eq!(current_download_job_stop(), Some(DownloadJobStop::Cancel));

        drop(job);
        assert_eq!(current_download_job_stop(), None);
    }

    #[test]
    fn companion_owner_controls_only_its_job_and_progress_is_snapshotted() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            *state = DownloadJobState::default();
        }
        let job = begin_download_job_owned(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
            "11111111-1111-4111-8111-111111111111",
        )
        .expect("owned job");
        update_download_job_progress(
            Some(42.5),
            Some(10.0),
            Some(20.0),
            Some(2.5),
            "Video indiriliyor...",
        );

        let owner_view = download_job_snapshot_for(
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"),
            Some("11111111-1111-4111-8111-111111111111"),
        )
        .unwrap();
        assert_eq!(owner_view.job_id, job.id());
        assert_eq!(owner_view.status, "downloading");
        assert_eq!(owner_view.percent, Some(42.5));
        assert!(owner_view.controllable);

        let other_view = download_job_snapshot_for(
            Some("chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/"),
            Some("11111111-1111-4111-8111-111111111111"),
        )
        .unwrap();
        assert!(!other_view.controllable);
        assert!(ensure_download_job_owner(
            "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/",
            "11111111-1111-4111-8111-111111111111",
            job.id(),
        )
        .is_err());
        assert!(ensure_download_job_owner(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
            "11111111-1111-4111-8111-111111111111",
            job.id(),
        )
        .is_ok());
        drop(job);
    }

    #[test]
    fn terminal_snapshot_survives_guard_drop_for_state_recovery() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            *state = DownloadJobState::default();
        }
        let job = begin_download_job_owned(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
            "22222222-2222-4222-8222-222222222222",
        )
        .unwrap();
        let job_id = job.id().to_string();
        drop(job);
        record_download_job_terminal(&job_id, "completed");

        let snapshot = download_job_snapshot_for(
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"),
            Some("22222222-2222-4222-8222-222222222222"),
        )
        .expect("terminal snapshot");
        assert_eq!(snapshot.status, "completed");
        assert_eq!(snapshot.percent, Some(100.0));
        assert!(!snapshot.controllable);
    }

    #[test]
    fn completed_result_is_owner_scoped_and_never_serializes_its_path() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            *state = DownloadJobState::default();
        }
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let analysis_request_id = "33333333-3333-4333-8333-333333333333";
        let job = begin_download_job_owned(origin, analysis_request_id).unwrap();
        let job_id = job.id().to_string();
        let target = std::env::temp_dir().join(format!(
            "mediadrop-result-canary-{}-secret.mp4",
            Uuid::new_v4()
        ));
        fs::write(&target, b"result").unwrap();

        record_download_job_operation(&job_id, "post_export");
        record_download_job_result(
            &job_id,
            target.clone(),
            DownloadResultKind::File,
            "Gonderi karti.mp4".to_string(),
            1,
            0,
            Some(6),
        );
        record_download_job_terminal(&job_id, "completed");
        drop(job);

        let view = download_job_snapshot_for(Some(origin), Some(analysis_request_id)).unwrap();
        assert_eq!(view.operation_kind, "post_export");
        let result = view.result.as_ref().expect("safe result projection");
        assert_eq!(result.kind, "file");
        assert_eq!(result.display_name, "Gonderi karti.mp4");
        assert_eq!(result.file_count, 1);
        assert_eq!(result.failed_count, 0);
        assert_eq!(result.size_bytes, Some(6));
        assert!(result.can_reveal);

        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains(target.to_string_lossy().as_ref()));
        assert_eq!(
            download_job_result_target_for(origin, analysis_request_id, &job_id).unwrap(),
            target
        );
        assert!(download_job_result_target_for(
            "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/",
            analysis_request_id,
            &job_id,
        )
        .unwrap_err()
        .contains("not_job_owner"));

        fs::remove_file(&target).unwrap();
        assert!(
            download_job_result_target_for(origin, analysis_request_id, &job_id)
                .unwrap_err()
                .contains("result_missing")
        );
    }

    #[test]
    fn a_full_progress_value_does_not_hide_validation_or_postprocessing() {
        let _serial = TEST_LOCK.lock().expect("test lock");
        if let Ok(mut state) = jobs().lock() {
            *state = DownloadJobState::default();
        }
        let job = begin_download_job_owned(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/",
            "55555555-5555-4555-8555-555555555555",
        )
        .unwrap();
        update_download_job_progress(Some(100.0), None, None, None, "İndiriliyor...");
        update_download_job_progress(None, None, None, None, "Çıktı doğrulanıyor...");
        let snapshot = download_job_snapshot_for(
            Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop/"),
            Some("55555555-5555-4555-8555-555555555555"),
        )
        .unwrap();
        assert_eq!(snapshot.status, "validating");
        assert_eq!(snapshot.percent, Some(100.0));
        drop(job);
    }

    #[test]
    fn post_card_rendering_has_a_stable_popup_stage() {
        assert_eq!(
            progress_stage("Gönderi kartı hazırlanıyor..."),
            ("preparing", "rendering_card")
        );
    }
}
