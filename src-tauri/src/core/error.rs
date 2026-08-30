use serde::{Deserialize, Serialize};
use std::fmt;

pub(crate) const STRUCTURED_ERROR_PREFIX: &str = "__MEDIADROP_ERROR__";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FallbackOffer {
    pub(crate) kind: String,
    pub(crate) quality: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiError {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) retryable: bool,
    #[serde(default)]
    pub(crate) action: Option<String>,
    #[serde(default)]
    pub(crate) report_id: Option<String>,
    #[serde(default, alias = "fallback_offer", skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_offer: Option<FallbackOffer>,
}

impl ApiError {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            action: None,
            report_id: None,
            fallback_offer: None,
        }
    }

    pub(crate) fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub(crate) fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub(crate) fn with_report_id(mut self, report_id: impl Into<String>) -> Self {
        self.report_id = Some(report_id.into());
        self
    }

    pub(crate) fn from_legacy(error: impl Into<String>) -> Self {
        let error = error.into();
        if let Some(payload) = error.strip_prefix(STRUCTURED_ERROR_PREFIX) {
            let mut values = serde_json::Deserializer::from_str(payload).into_iter::<ApiError>();
            if let Some(Ok(mut parsed)) = values.next() {
                if parsed.report_id.is_none() {
                    let suffix = &payload[values.byte_offset()..];
                    if let Some((_, reference)) =
                        suffix.rsplit_once("Hata raporu oluşturuldu:")
                    {
                        let reference = reference.trim();
                        if !reference.is_empty() {
                            parsed.report_id = Some(reference.to_string());
                        }
                    }
                }
                return parsed;
            }
        }
        ApiError::new("internal_error", error)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<String> for ApiError {
    fn from(error: String) -> Self {
        Self::from_legacy(error)
    }
}

impl From<&str> for ApiError {
    fn from(error: &str) -> Self {
        Self::from_legacy(error)
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::{ApiError, STRUCTURED_ERROR_PREFIX};

    #[test]
    fn structured_clip_fallback_stays_typed_across_the_companion_boundary() {
        let raw = format!(
            "{}{}",
            STRUCTURED_ERROR_PREFIX,
            r#"{"code":"true_quality_failed","message":"4K klip indirilemedi.","fallback_offer":{"kind":"hls_1080","quality":"1080p","label":"1080p hızlı klip indir"}}"#
        );
        let error = ApiError::from_legacy(raw);
        assert_eq!(error.code, "true_quality_failed");
        assert_eq!(
            error
                .fallback_offer
                .as_ref()
                .map(|offer| offer.kind.as_str()),
            Some("hls_1080")
        );
    }

    #[test]
    fn structured_error_keeps_its_code_when_a_report_reference_is_appended() {
        let raw = format!(
            "{}{}\n\nHata raporu oluşturuldu: C:\\Reports\\instagram.txt",
            STRUCTURED_ERROR_PREFIX,
            r#"{"code":"instagram_auth_required","message":"Instagram oturumu gerekiyor.","retryable":true,"action":"request_cookie_permission"}"#
        );

        let error = ApiError::from_legacy(raw);

        assert_eq!(error.code, "instagram_auth_required");
        assert_eq!(error.action.as_deref(), Some("request_cookie_permission"));
        assert_eq!(
            error.report_id.as_deref(),
            Some(r"C:\Reports\instagram.txt")
        );
    }
}
