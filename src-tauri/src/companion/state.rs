use std::collections::HashMap;

use serde_json::Value;

use crate::companion::protocol::ProjectedAnalysis;

const REQUEST_TTL_MS: u128 = 30 * 60 * 1000;
const MAX_REQUESTS_PER_ORIGIN: usize = 32;

#[derive(Clone, Debug)]
struct RequestEntry {
    command: String,
    payload: Value,
    created_at_ms: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisStatus {
    Analyzing,
    Ready,
    NeedsUser,
    Error,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnalysisSnapshot {
    pub(crate) analysis_request_id: String,
    pub(crate) status: AnalysisStatus,
    pub(crate) site: String,
    pub(crate) state_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) projection: Option<ProjectedAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<crate::core::error::ApiError>,
}

#[derive(Clone)]
pub(crate) enum StoredAnalysis {
    Video {
        source_url: String,
        value: Value,
    },
    Media {
        source_url: String,
        value: crate::MediaAnalysis,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DownloadPlan {
    pub(crate) operation_kind: String,
    pub(crate) source_url: String,
    pub(crate) format_id: String,
    pub(crate) kind: String,
    pub(crate) quality: String,
    pub(crate) title: Option<String>,
    pub(crate) clip_start_seconds: Option<f64>,
    pub(crate) clip_end_seconds: Option<f64>,
    pub(crate) registry_item: Option<(String, String)>,
    pub(crate) registry_batch: Option<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertOutcome {
    Inserted,
    Duplicate,
    Conflict,
}

#[derive(Default)]
pub(crate) struct CompanionStore {
    requests: HashMap<(String, String), RequestEntry>,
    latest: HashMap<String, String>,
    analyses: HashMap<(String, String), AnalysisSnapshot>,
    stored_analyses: HashMap<(String, String), StoredAnalysis>,
    download_plans: HashMap<(String, String), DownloadPlan>,
    revision: u64,
}

impl CompanionStore {
    fn prune(&mut self, now_ms: u128) {
        self.requests
            .retain(|_, entry| now_ms.saturating_sub(entry.created_at_ms) <= REQUEST_TTL_MS);
        self.latest.retain(|origin, request_id| {
            self.requests
                .contains_key(&(origin.clone(), request_id.clone()))
        });
        self.analyses
            .retain(|key, _| self.requests.contains_key(key));
        self.stored_analyses
            .retain(|key, _| self.requests.contains_key(key));
        self.download_plans
            .retain(|key, _| self.requests.contains_key(key));
    }

    pub(crate) fn insert_request(
        &mut self,
        origin: &str,
        request_id: &str,
        command: &str,
        payload: &Value,
        now_ms: u128,
    ) -> InsertOutcome {
        self.insert_request_inner(origin, request_id, command, payload, now_ms, true)
    }

    pub(crate) fn insert_command(
        &mut self,
        origin: &str,
        request_id: &str,
        command: &str,
        payload: &Value,
        now_ms: u128,
    ) -> InsertOutcome {
        self.insert_request_inner(origin, request_id, command, payload, now_ms, false)
    }

    fn insert_request_inner(
        &mut self,
        origin: &str,
        request_id: &str,
        command: &str,
        payload: &Value,
        now_ms: u128,
        update_latest: bool,
    ) -> InsertOutcome {
        self.prune(now_ms);
        let key = (origin.to_string(), request_id.to_string());
        if let Some(existing) = self.requests.get(&key) {
            return if existing.command == command && existing.payload == *payload {
                InsertOutcome::Duplicate
            } else {
                InsertOutcome::Conflict
            };
        }

        let mut origin_entries = self
            .requests
            .iter()
            .filter(|((entry_origin, _), _)| entry_origin == origin)
            .map(|(key, entry)| (key.clone(), entry.created_at_ms))
            .collect::<Vec<_>>();
        if origin_entries.len() >= MAX_REQUESTS_PER_ORIGIN {
            origin_entries.sort_by_key(|(_, created_at)| *created_at);
            if let Some((oldest, _)) = origin_entries.first() {
                self.requests.remove(oldest);
                self.analyses.remove(oldest);
                self.stored_analyses.remove(oldest);
                self.download_plans.remove(oldest);
                if self.latest.get(origin) == Some(&oldest.1) {
                    self.latest.remove(origin);
                }
            }
        }

        self.requests.insert(
            key,
            RequestEntry {
                command: command.to_string(),
                payload: payload.clone(),
                created_at_ms: now_ms,
            },
        );
        if update_latest {
            self.latest
                .insert(origin.to_string(), request_id.to_string());
        }
        InsertOutcome::Inserted
    }

    pub(crate) fn latest_request_id(&mut self, origin: &str, now_ms: u128) -> Option<String> {
        self.prune(now_ms);
        self.latest.get(origin).cloned()
    }

    pub(crate) fn latest_request_id_for_page(
        &mut self,
        origin: &str,
        page_url: &str,
        now_ms: u128,
    ) -> Option<String> {
        self.prune(now_ms);
        self.requests
            .iter()
            .filter(|((entry_origin, _), entry)| {
                entry_origin == origin
                    && entry.command == "analyze_source"
                    && entry.payload.get("pageUrl").and_then(Value::as_str) == Some(page_url)
            })
            .max_by_key(|(_, entry)| entry.created_at_ms)
            .map(|((_, request_id), _)| request_id.clone())
    }

    fn next_revision(&mut self) -> u64 {
        self.revision = self.revision.saturating_add(1);
        self.revision
    }

    pub(crate) fn start_analysis(
        &mut self,
        origin: &str,
        request_id: &str,
        site: &str,
        _now_ms: u128,
    ) -> Option<u64> {
        let key = (origin.to_string(), request_id.to_string());
        if !self.requests.contains_key(&key) {
            return None;
        }
        let revision = self.next_revision();
        self.analyses.insert(
            key,
            AnalysisSnapshot {
                analysis_request_id: request_id.to_string(),
                status: AnalysisStatus::Analyzing,
                site: site.to_string(),
                state_revision: revision,
                projection: None,
                error: None,
            },
        );
        Some(revision)
    }

    pub(crate) fn finish_analysis(
        &mut self,
        origin: &str,
        request_id: &str,
        projection: ProjectedAnalysis,
    ) -> Option<u64> {
        let key = (origin.to_string(), request_id.to_string());
        if !self.analyses.contains_key(&key) {
            return None;
        }
        let revision = self.next_revision();
        let analysis = self.analyses.get_mut(&key)?;
        analysis.status = AnalysisStatus::Ready;
        analysis.site = projection.site.clone();
        analysis.state_revision = revision;
        analysis.projection = Some(projection);
        analysis.error = None;
        Some(revision)
    }

    pub(crate) fn fail_analysis(
        &mut self,
        origin: &str,
        request_id: &str,
        status: AnalysisStatus,
        error: crate::core::error::ApiError,
    ) -> Option<u64> {
        let key = (origin.to_string(), request_id.to_string());
        if !self.analyses.contains_key(&key) {
            return None;
        }
        let revision = self.next_revision();
        let analysis = self.analyses.get_mut(&key)?;
        analysis.status = status;
        analysis.state_revision = revision;
        analysis.projection = None;
        analysis.error = Some(error);
        Some(revision)
    }

    pub(crate) fn set_download_error(
        &mut self,
        origin: &str,
        request_id: &str,
        error: Option<crate::core::error::ApiError>,
    ) -> Option<u64> {
        let key = (origin.to_string(), request_id.to_string());
        if !self.analyses.contains_key(&key) {
            return None;
        }
        let revision = self.next_revision();
        let analysis = self.analyses.get_mut(&key)?;
        analysis.status = if error.is_some() {
            AnalysisStatus::Error
        } else {
            AnalysisStatus::Ready
        };
        analysis.state_revision = revision;
        analysis.error = error;
        Some(revision)
    }

    pub(crate) fn analysis(&self, origin: &str, request_id: &str) -> Option<AnalysisSnapshot> {
        self.analyses
            .get(&(origin.to_string(), request_id.to_string()))
            .cloned()
    }

    pub(crate) fn request_payload(&self, origin: &str, request_id: &str) -> Option<Value> {
        self.requests
            .get(&(origin.to_string(), request_id.to_string()))
            .map(|entry| entry.payload.clone())
    }

    pub(crate) fn store_analysis(
        &mut self,
        origin: &str,
        request_id: &str,
        analysis: StoredAnalysis,
    ) -> bool {
        let key = (origin.to_string(), request_id.to_string());
        if !self.analyses.contains_key(&key) {
            return false;
        }
        self.stored_analyses.insert(key, analysis);
        true
    }

    pub(crate) fn stored_analysis(&self, origin: &str, request_id: &str) -> Option<StoredAnalysis> {
        self.stored_analyses
            .get(&(origin.to_string(), request_id.to_string()))
            .cloned()
    }

    pub(crate) fn store_download_plan(
        &mut self,
        origin: &str,
        request_id: &str,
        plan: DownloadPlan,
    ) -> bool {
        let key = (origin.to_string(), request_id.to_string());
        if !self.stored_analyses.contains_key(&key) {
            return false;
        }
        self.download_plans.insert(key, plan);
        true
    }

    pub(crate) fn download_plan(&self, origin: &str, request_id: &str) -> Option<DownloadPlan> {
        self.download_plans
            .get(&(origin.to_string(), request_id.to_string()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::companion::protocol::{ProjectedAnalysis, ProjectedMedia};

    use super::{AnalysisStatus, CompanionStore, InsertOutcome};

    #[test]
    fn duplicate_request_is_idempotent_but_changed_payload_conflicts() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnop/";
        let request_id = "24d417a8-81ba-4edb-98de-e9267ed1e302";
        let first = json!({"pageUrl":"https://www.youtube.com/watch?v=one"});

        assert_eq!(
            store.insert_request(origin, request_id, "analyze_source", &first, 100),
            InsertOutcome::Inserted
        );
        assert_eq!(
            store.insert_request(origin, request_id, "analyze_source", &first, 101),
            InsertOutcome::Duplicate
        );
        assert_eq!(
            store.insert_request(
                origin,
                request_id,
                "analyze_source",
                &json!({"pageUrl":"https://www.youtube.com/watch?v=two"}),
                102,
            ),
            InsertOutcome::Conflict
        );
    }

    #[test]
    fn request_capacity_evicts_the_oldest_origin_state_immediately() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        for index in 0..=super::MAX_REQUESTS_PER_ORIGIN {
            store.insert_request(
                origin,
                &format!("request-{index}"),
                "get_state",
                &json!({"index":index}),
                index as u128,
            );
        }
        assert_eq!(store.requests.len(), super::MAX_REQUESTS_PER_ORIGIN);
        assert!(!store
            .requests
            .contains_key(&(origin.to_string(), "request-0".to_string())));
    }

    #[test]
    fn latest_request_is_scoped_by_origin_and_expired_entries_disappear() {
        let mut store = CompanionStore::default();
        let first = "chrome-extension://aaaaaaaaaaaaaaaa/";
        let second = "chrome-extension://bbbbbbbbbbbbbbbb/";
        store.insert_request(
            first,
            "11111111-1111-4111-8111-111111111111",
            "get_state",
            &json!({}),
            10,
        );
        store.insert_request(
            second,
            "22222222-2222-4222-8222-222222222222",
            "get_state",
            &json!({}),
            20,
        );

        assert_eq!(
            store.latest_request_id(first, 30).as_deref(),
            Some("11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(
            store.latest_request_id(second, 30).as_deref(),
            Some("22222222-2222-4222-8222-222222222222")
        );
        assert_eq!(store.latest_request_id(first, 30 * 60 * 1000 + 11), None);
    }

    #[test]
    fn latest_analysis_request_can_be_selected_by_the_active_page() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        store.insert_request(
            origin,
            first,
            "analyze_source",
            &json!({"pageUrl":"https://x.com/example/status/1"}),
            10,
        );
        store.insert_request(
            origin,
            second,
            "analyze_source",
            &json!({"pageUrl":"https://x.com/example/status/2"}),
            20,
        );

        assert_eq!(
            store
                .latest_request_id_for_page(origin, "https://x.com/example/status/1", 30)
                .as_deref(),
            Some(first)
        );
        assert_eq!(
            store
                .latest_request_id_for_page(origin, "https://x.com/example/status/2", 30)
                .as_deref(),
            Some(second)
        );
        assert_eq!(
            store.latest_request_id_for_page(origin, "https://x.com/example/status/3", 30),
            None
        );
    }

    #[test]
    fn analysis_snapshot_transitions_are_revisioned_and_origin_scoped() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let request_id = "33333333-3333-4333-8333-333333333333";
        store.insert_request(origin, request_id, "analyze_source", &json!({}), 10);
        let first_revision = store
            .start_analysis(origin, request_id, "youtube", 10)
            .expect("start analysis");
        assert_eq!(
            store.analysis(origin, request_id).unwrap().status,
            AnalysisStatus::Analyzing
        );

        let projection = ProjectedAnalysis {
            site: "youtube".to_string(),
            media: vec![ProjectedMedia {
                media_id: "m1".to_string(),
                media_type: "video".to_string(),
                source_index: 0,
                display_title: "Example".to_string(),
                author: String::new(),
                width: None,
                height: None,
                is_story: false,
                has_audio: true,
                preview_available: false,
                title: "Example".to_string(),
                uploader: String::new(),
                duration_seconds: Some(12.0),
                thumbnail_url: None,
                formats: Vec::new(),
                audio_available: true,
            }],
        };
        let ready_revision = store
            .finish_analysis(origin, request_id, projection)
            .expect("finish analysis");
        let snapshot = store.analysis(origin, request_id).unwrap();
        assert!(ready_revision > first_revision);
        assert_eq!(snapshot.status, AnalysisStatus::Ready);
        assert_eq!(
            snapshot.projection.as_ref().unwrap().media[0].title,
            "Example"
        );
        assert!(store
            .analysis(
                "chrome-extension://otherotherotherotherotherothe/",
                request_id
            )
            .is_none());
    }

    #[test]
    fn download_error_is_redacted_and_keeps_the_ready_projection_for_retry() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let request_id = "55555555-5555-4555-8555-555555555555";
        store.insert_request(origin, request_id, "analyze_source", &json!({}), 10);
        store.start_analysis(origin, request_id, "youtube", 10);
        store.finish_analysis(
            origin,
            request_id,
            ProjectedAnalysis {
                site: "youtube".to_string(),
                media: vec![ProjectedMedia {
                    media_id: "m1".to_string(),
                    media_type: "video".to_string(),
                    source_index: 0,
                    display_title: "Example".to_string(),
                    author: String::new(),
                    width: None,
                    height: None,
                    is_story: false,
                    has_audio: true,
                    preview_available: false,
                    title: "Example".to_string(),
                    uploader: String::new(),
                    duration_seconds: None,
                    thumbnail_url: None,
                    formats: Vec::new(),
                    audio_available: true,
                }],
            },
        );

        store.set_download_error(
            origin,
            request_id,
            Some(crate::core::error::ApiError::new(
                "download_failed",
                "İndirme tamamlanamadı.",
            )),
        );
        let failed = store.analysis(origin, request_id).unwrap();
        assert_eq!(failed.status, AnalysisStatus::Error);
        assert_eq!(failed.error.unwrap().code, "download_failed");
        assert!(failed.projection.is_some());

        store.set_download_error(origin, request_id, None);
        let retry = store.analysis(origin, request_id).unwrap();
        assert_eq!(retry.status, AnalysisStatus::Ready);
        assert!(retry.error.is_none());
        assert!(retry.projection.is_some());
    }

    #[test]
    fn simultaneous_analysis_snapshots_remain_request_scoped() {
        let mut store = CompanionStore::default();
        let origin = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        let first = "44444444-4444-4444-8444-444444444444";
        let second = "66666666-6666-4666-8666-666666666666";
        store.insert_request(
            origin,
            first,
            "analyze_source",
            &json!({"pageUrl":"https://www.youtube.com/watch?v=first"}),
            10,
        );
        store.start_analysis(origin, first, "youtube", 10);
        store.insert_request(
            origin,
            second,
            "analyze_source",
            &json!({"pageUrl":"https://x.com/example/status/2"}),
            11,
        );
        store.start_analysis(origin, second, "twitter", 11);

        assert_eq!(
            store.analysis(origin, first).unwrap().status,
            AnalysisStatus::Analyzing
        );
        assert_eq!(
            store.analysis(origin, second).unwrap().status,
            AnalysisStatus::Analyzing
        );
        store.fail_analysis(
            origin,
            first,
            AnalysisStatus::Error,
            crate::core::error::ApiError::new("analysis_failed", "failed"),
        );
        assert_eq!(
            store.analysis(origin, first).unwrap().status,
            AnalysisStatus::Error
        );
        assert_eq!(
            store.analysis(origin, second).unwrap().status,
            AnalysisStatus::Analyzing
        );
    }
}
