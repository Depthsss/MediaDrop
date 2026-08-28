use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::MediaAnalysis;

pub(crate) const MEDIA_ANALYSIS_TTL_MS: u128 = 30 * 60 * 1000;
const MEDIA_ANALYSIS_REGISTRY_CAPACITY: usize = 64;

#[derive(Clone)]
pub(crate) struct RegisteredMediaAnalysis {
    pub(crate) analysis: MediaAnalysis,
    pub(crate) source_url: String,
    pub(crate) auth_mode: Option<String>,
    registered_at_ms: u128,
    refresh_attempted: bool,
}

static MEDIA_ANALYSIS_REGISTRY: OnceLock<
    Arc<Mutex<HashMap<String, RegisteredMediaAnalysis>>>,
> = OnceLock::new();

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn registry() -> &'static Arc<Mutex<HashMap<String, RegisteredMediaAnalysis>>> {
    MEDIA_ANALYSIS_REGISTRY.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub(crate) fn register_media_analysis(
    analysis: &MediaAnalysis,
    source_url: &str,
    auth_mode: Option<&str>,
) -> Result<(), String> {
    let now = now_ms();
    let mut entries = registry()
        .lock()
        .map_err(|_| "Medya analiz kaydi kilidi alinamadi.".to_string())?;
    entries.retain(|_, entry| entry.analysis.expires_at_ms > now);
    if entries.len() >= MEDIA_ANALYSIS_REGISTRY_CAPACITY {
        if let Some(oldest) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.registered_at_ms)
            .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
    }
    entries.insert(
        analysis.analysis_id.clone(),
        RegisteredMediaAnalysis {
            analysis: analysis.clone(),
            source_url: source_url.to_string(),
            auth_mode: auth_mode.map(str::to_string),
            registered_at_ms: now,
            refresh_attempted: false,
        },
    );
    Ok(())
}

pub(crate) fn registered_media_analysis(
    analysis_id: &str,
) -> Result<RegisteredMediaAnalysis, String> {
    let clean = analysis_id.trim();
    if clean.is_empty() {
        return Err("Medya analiz kimligi bos.".to_string());
    }
    let now = now_ms();
    let mut entries = registry()
        .lock()
        .map_err(|_| "Medya analiz kaydi kilidi alinamadi.".to_string())?;
    entries.retain(|_, entry| entry.analysis.expires_at_ms > now);
    entries.get(clean).cloned().ok_or_else(|| {
        "Medya analizi bulunamadi veya suresi doldu. Linki yeniden analiz et.".to_string()
    })
}

pub(crate) fn require_media_registry_identity(
    analysis_id: &str,
    item_id: &str,
) -> Result<(String, String), String> {
    let analysis_id = analysis_id.trim();
    let item_id = item_id.trim();
    if analysis_id.is_empty() {
        return Err("Medya indirme icin analysisId zorunludur.".to_string());
    }
    if item_id.is_empty() {
        return Err("Medya indirme icin itemId zorunludur.".to_string());
    }
    Ok((analysis_id.to_string(), item_id.to_string()))
}

pub(crate) fn media_error_allows_registry_refresh(
    error: &str,
    refresh_attempts: usize,
) -> bool {
    refresh_attempts == 0
        && ["http 401", "http 403", "http 410"]
            .iter()
            .any(|signal| error.to_lowercase().contains(signal))
}

pub(crate) fn claim_registry_refresh(refresh_attempted: &mut bool) -> bool {
    if *refresh_attempted {
        return false;
    }
    *refresh_attempted = true;
    true
}

pub(crate) fn begin_registered_media_refresh(
    analysis_id: &str,
) -> Result<(String, Option<String>), String> {
    let mut entries = registry()
        .lock()
        .map_err(|_| "Medya analiz kaydi kilidi alinamadi.".to_string())?;
    let entry = entries
        .get_mut(analysis_id.trim())
        .ok_or_else(|| "Medya analizi bulunamadi veya suresi doldu.".to_string())?;
    if !claim_registry_refresh(&mut entry.refresh_attempted) {
        return Err("Medya kaynagi bu analiz icin daha once yenilendi.".to_string());
    }
    Ok((entry.source_url.clone(), entry.auth_mode.clone()))
}

pub(crate) fn replace_registered_media_analysis(
    analysis_id: &str,
    analysis: MediaAnalysis,
) -> Result<RegisteredMediaAnalysis, String> {
    let mut entries = registry()
        .lock()
        .map_err(|_| "Medya analiz kaydi kilidi alinamadi.".to_string())?;
    let entry = entries
        .get_mut(analysis_id.trim())
        .ok_or_else(|| "Medya analiz kaydi yenileme sirasinda kayboldu.".to_string())?;
    entry.analysis = analysis;
    entry.registered_at_ms = now_ms();
    Ok(entry.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_policy_only_accepts_expired_signed_url_statuses_once() {
        assert!(media_error_allows_registry_refresh("HTTP 403 Forbidden", 0));
        assert!(!media_error_allows_registry_refresh("HTTP 403 Forbidden", 1));
        assert!(!media_error_allows_registry_refresh("HTTP 500", 0));
    }

    #[test]
    fn identity_requires_both_registry_keys() {
        assert!(require_media_registry_identity("", "item").is_err());
        assert!(require_media_registry_identity("analysis", "").is_err());
        assert_eq!(
            require_media_registry_identity(" analysis ", " item ").unwrap(),
            ("analysis".to_string(), "item".to_string())
        );
    }
}
