use std::sync::{Arc, OnceLock};
use std::time::Duration;

use reqwest::blocking::Client;

use crate::{
    instagram_avatar_redirect_policy, media_redirect_policy, twitter_avatar_redirect_policy,
    twitter_profile_redirect_policy, SafePreviewDnsResolver,
};

fn shared_client(
    slot: &'static OnceLock<Result<Client, String>>,
    build: impl FnOnce() -> Result<Client, String>,
) -> Result<Client, String> {
    slot.get_or_init(build).clone()
}

static CLOUD_REPORT_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static HTTP_RANGE_SHORT_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static HTTP_RANGE_LONG_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static INSTAGRAM_AVATAR_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static MEDIA_SHORT_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static MEDIA_LONG_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static TWITTER_AVATAR_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static TWITTER_PROFILE_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();

pub(crate) fn cloud_report_client() -> Result<Client, String> {
    shared_client(&CLOUD_REPORT_CLIENT, || {
        Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|error| format!("Cloud report client oluşturulamadı: {error}"))
    })
}

pub(crate) fn http_range_client(timeout: Duration) -> Result<Client, String> {
    let (slot, effective_timeout) = if timeout <= Duration::from_secs(45) {
        (&HTTP_RANGE_SHORT_CLIENT, Duration::from_secs(45))
    } else {
        (&HTTP_RANGE_LONG_CLIENT, Duration::from_secs(90))
    };
    shared_client(slot, || {
        Client::builder()
            .dns_resolver(Arc::new(SafePreviewDnsResolver))
            .timeout(effective_timeout)
            .redirect(media_redirect_policy())
            .build()
            .map_err(|error| format!("HTTP range client oluşturulamadı: {error}"))
    })
}

pub(crate) fn instagram_avatar_client() -> Result<Client, String> {
    shared_client(&INSTAGRAM_AVATAR_CLIENT, || {
        Client::builder()
            .dns_resolver(Arc::new(SafePreviewDnsResolver))
            .timeout(Duration::from_secs(25))
            .redirect(instagram_avatar_redirect_policy())
            .build()
            .map_err(|error| format!("Instagram avatar client oluşturulamadı: {error}"))
    })
}

pub(crate) fn media_client(timeout: Duration) -> Result<Client, String> {
    let (slot, effective_timeout) = if timeout <= Duration::from_secs(35) {
        (&MEDIA_SHORT_CLIENT, Duration::from_secs(35))
    } else {
        (&MEDIA_LONG_CLIENT, Duration::from_secs(90))
    };
    shared_client(slot, || {
        Client::builder()
            .dns_resolver(Arc::new(SafePreviewDnsResolver))
            .timeout(effective_timeout)
            .redirect(media_redirect_policy())
            .build()
            .map_err(|error| format!("Medya HTTP client oluşturulamadı: {error}"))
    })
}

pub(crate) fn twitter_avatar_client() -> Result<Client, String> {
    shared_client(&TWITTER_AVATAR_CLIENT, || {
        Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(twitter_avatar_redirect_policy())
            .build()
            .map_err(|error| format!("Avatar client oluşturulamadı: {error}"))
    })
}

pub(crate) fn twitter_profile_client() -> Result<Client, String> {
    shared_client(&TWITTER_PROFILE_CLIENT, || {
        Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(twitter_profile_redirect_policy())
            .build()
            .map_err(|error| format!("Profil client oluşturulamadı: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn error_chain_contains(mut error: &(dyn Error + 'static), needle: &str) -> bool {
        loop {
            if error.to_string().contains(needle) {
                return true;
            }
            let Some(source) = error.source() else {
                return false;
            };
            error = source;
        }
    }

    #[test]
    fn shared_profiles_build_without_network_io_and_are_reusable() {
        cloud_report_client().expect("cloud client");
        cloud_report_client().expect("cached cloud client");
        http_range_client(Duration::from_secs(45)).expect("short range client");
        http_range_client(Duration::from_secs(90)).expect("long range client");
        media_client(Duration::from_secs(35)).expect("short media client");
        media_client(Duration::from_secs(90)).expect("long media client");
    }

    #[test]
    fn media_clients_reject_dns_names_that_resolve_to_private_ips() {
        for client in [
            media_client(Duration::from_secs(35)).expect("media client"),
            http_range_client(Duration::from_secs(45)).expect("range client"),
        ] {
            let error = client
                .get("https://localhost/mediadrop-private-target")
                .send()
                .expect_err("localhost must be rejected before connecting");
            assert!(
                error_chain_contains(&error, "yerel veya ozel IP"),
                "unexpected error chain: {error:?}"
            );
        }
    }
}
