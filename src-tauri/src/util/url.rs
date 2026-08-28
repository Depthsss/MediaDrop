pub(crate) fn url_host(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let without_scheme = lower
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lower.as_str());
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_matches('.');
    let without_auth = authority.rsplit('@').next().unwrap_or(authority);
    let host_port = without_auth.trim_matches('.');
    let host = host_port.split(':').next().unwrap_or("").trim_matches('.');

    host.strip_prefix("www.").unwrap_or(host).to_string()
}

pub(crate) fn host_matches(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub(crate) fn is_youtube_url(url: &str) -> bool {
    let host = url_host(url);
    host_matches(&host, "youtube.com") || host == "youtu.be"
}

pub(crate) fn is_twitter_url(url: &str) -> bool {
    let host = url_host(url);
    host_matches(&host, "twitter.com") || host_matches(&host, "x.com") || host == "t.co"
}

pub(crate) fn is_instagram_url(url: &str) -> bool {
    let host = url_host(url);
    host_matches(&host, "instagram.com") || host_matches(&host, "instagr.am")
}

pub(crate) fn is_tiktok_url(url: &str) -> bool {
    host_matches(&url_host(url), "tiktok.com")
}

pub(crate) fn is_supported_media_url(url: &str) -> bool {
    is_youtube_url(url) || is_instagram_url(url) || is_twitter_url(url) || is_tiktok_url(url)
}

pub(crate) fn unsupported_media_link_message() -> &'static str {
    "Sadece YouTube, Instagram, X/Twitter ve TikTok linkleri destekleniyor."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_hosts_require_exact_or_subdomain_matches() {
        assert!(is_instagram_url("https://www.instagram.com/p/abc/"));
        assert!(is_twitter_url("https://x.com/user/status/1"));
        assert!(is_youtube_url("https://youtu.be/abc"));
        assert!(is_tiktok_url("https://vm.tiktok.com/abc"));
        assert!(!is_instagram_url("https://instagram.com.evil.example/p/abc"));
        assert!(!is_twitter_url("https://notx.com/status/1"));
    }
}
