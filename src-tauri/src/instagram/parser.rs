use std::collections::HashSet;

use crate::{
    instagram_avatar_url_allowed, instagram_likely_post_media_url, limit_report_text,
    sanitize_report_text, CanonicalInstagramIdentity, MediaItem, TwitterPostMetadata,
    TwitterQuoteContext,
};

pub(crate) fn json_text(value: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(|item| item.as_str()) {
            let clean = text.trim();
            if !clean.is_empty() {
                return clean.to_string();
            }
        }
    }

    String::new()
}

pub(crate) fn json_u32(value: &serde_json::Value, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(number) = value.get(*key).and_then(|item| item.as_u64()) {
            if number <= u32::MAX as u64 {
                return Some(number as u32);
            }
        }

        if let Some(text) = value.get(*key).and_then(|item| item.as_str()) {
            if let Ok(number) = text.trim().parse::<u32>() {
                return Some(number);
            }
        }
    }

    None
}

pub(crate) fn json_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(number) = value.get(*key).and_then(|item| item.as_u64()) {
            return Some(number);
        }

        if let Some(text) = value.get(*key).and_then(|item| item.as_str()) {
            if let Ok(number) = text.trim().replace(',', "").parse::<u64>() {
                return Some(number);
            }
        }
    }

    None
}

fn json_nested<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = value;

    for key in path {
        current = current.get(*key)?;
    }

    Some(current)
}

fn json_nested_text(value: &serde_json::Value, paths: &[&[&str]]) -> String {
    for path in paths {
        let Some(item) = json_nested(value, path) else {
            continue;
        };

        if let Some(text) = item.as_str() {
            let clean = text.trim();
            if !clean.is_empty() {
                return clean.to_string();
            }
        }

        if let Some(number) = item.as_u64() {
            return number.to_string();
        }
    }

    String::new()
}

pub(crate) fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn supported_image_extension(ext: &str) -> bool {
    matches!(
        ext.trim().trim_start_matches('.').to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif"
    )
}

pub(crate) fn supported_video_extension(ext: &str) -> bool {
    matches!(
        ext.trim().trim_start_matches('.').to_lowercase().as_str(),
        "mp4" | "mov" | "webm" | "mkv"
    )
}

fn image_extension_hint(value: &str) -> Option<String> {
    let clean = value.trim().trim_start_matches('.').to_lowercase();

    for extension in [
        "jpg", "jpeg", "png", "webp", "gif", "avif", "mp4", "mov", "webm", "mkv",
    ] {
        if clean == extension || clean.contains(&format!("{}_", extension)) {
            return Some(extension.to_string());
        }
    }

    for (needle, extension) in [
        ("dst-jpg", "jpg"),
        ("dst-webp", "webp"),
        ("dst-png", "png"),
        ("video_mp4", "mp4"),
    ] {
        if clean.contains(needle) {
            return Some(extension.to_string());
        }
    }

    None
}

pub(crate) fn extension_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;

    if let Some(format) = parsed.query_pairs().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case("format")
            || key.eq_ignore_ascii_case("fm")
            || key.eq_ignore_ascii_case("ext")
            || key.eq_ignore_ascii_case("stp")
        {
            image_extension_hint(&value)
        } else {
            None
        }
    }) {
        return Some(format);
    }

    let file_name = parsed
        .path_segments()
        .and_then(|mut segments| segments.next_back())?;
    let ext = file_name.rsplit_once('.')?.1;
    let clean = ext.trim().to_lowercase();

    if supported_image_extension(&clean) || supported_video_extension(&clean) {
        Some(clean)
    } else {
        None
    }
}

fn media_extension_from_value(value: &serde_json::Value, url: &str) -> String {
    let ext = json_text(value, &["extension", "ext", "format"]);
    let clean = ext.trim().trim_start_matches('.').to_lowercase();

    if supported_image_extension(&clean) || supported_video_extension(&clean) {
        return clean;
    }

    extension_from_url(url).unwrap_or_else(|| "jpg".to_string())
}

fn find_string_array_url(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(array) = value.get(*key).and_then(|item| item.as_array()) else {
            continue;
        };

        for item in array {
            if let Some(text) = item.as_str() {
                let clean = text.trim();
                if clean.starts_with("https://") || clean.starts_with("http://") {
                    return Some(clean.to_string());
                }
            }
        }
    }

    None
}

fn gallery_value_is_video(value: &serde_json::Value) -> bool {
    json_text(value, &["type", "typename", "media_type", "kind"])
        .to_ascii_lowercase()
        .contains("video")
        || value.get("is_video").and_then(|item| item.as_bool()) == Some(true)
        || supported_video_extension(
            json_text(value, &["extension", "ext", "format"])
                .trim()
                .trim_start_matches('.'),
        )
}

fn gallery_value_media_url(value: &serde_json::Value) -> Option<String> {
    let is_video = gallery_value_is_video(value);
    let direct_keys: &[&str] = if is_video {
        &[
            "video_url",
            "videoUrl",
            "video_url_https",
            "playback_url",
            "playbackUrl",
            "download_url",
            "file_url",
            "url",
        ]
    } else {
        &[
            "url",
            "image",
            "image_url",
            "imageUrl",
            "media_url_https",
            "media_url",
            "display_url",
            "displayUrl",
            "download_url",
            "file_url",
            "full_size_url",
            "fullSizeUrl",
            "original_img_url",
            "thumbnail_url",
            "thumbnailUrl",
            "src",
        ]
    };
    let direct = json_text(value, direct_keys);

    if direct.starts_with("https://") || direct.starts_with("http://") {
        return Some(direct);
    }

    let array_keys: &[&str] = if is_video {
        &["video_urls", "videoUrls", "videos"]
    } else {
        &[
            "urls",
            "images",
            "media_urls",
            "mediaUrls",
            "media_url_https",
            "media_url",
        ]
    };
    find_string_array_url(value, array_keys)
}

fn tiktok_preferred_video_url(value: &serde_json::Value) -> Option<String> {
    let video = value.get("video")?;
    let play_addr = video
        .get("PlayAddrStruct")
        .or_else(|| video.get("playAddrStruct"))?;
    play_addr
        .get("UrlList")
        .or_else(|| play_addr.get("urlList"))?
        .as_array()?
        .iter()
        .rev()
        .filter_map(|item| item.as_str().map(str::trim))
        .find(|url| url.starts_with("https://"))
        .map(str::to_string)
}

fn gallery_value_poster_url(value: &serde_json::Value) -> Option<String> {
    const POSTER_KEYS: &[&str] = &[
        "thumbnail_url",
        "thumbnailUrl",
        "thumbnail",
        "poster_url",
        "posterUrl",
        "poster",
        "display_url",
        "displayUrl",
        "cover_url",
        "coverUrl",
        "cover",
        "origin_cover",
        "originCover",
        "dynamic_cover",
        "dynamicCover",
    ];
    let direct = json_text(value, POSTER_KEYS);
    let direct = if direct.is_empty() {
        value
            .get("video")
            .map(|video| json_text(video, POSTER_KEYS))
            .unwrap_or_default()
    } else {
        direct
    };
    (direct.starts_with("https://") || direct.starts_with("http://")).then_some(direct)
}

fn gallery_value_audio_url(value: &serde_json::Value) -> Option<String> {
    let direct = json_text(
        value,
        &[
            "audio_url",
            "audioUrl",
            "audio_src",
            "audioSrc",
            "dash_audio_url",
            "dashAudioUrl",
        ],
    );
    if direct.starts_with("https://") || direct.starts_with("http://") {
        return Some(direct);
    }
    if let Some(audio) = value.get("audio") {
        if let Some(url) = audio.as_str().filter(|url| url.starts_with("http")) {
            return Some(url.to_string());
        }
        if audio.is_object() {
            if let Some(url) = gallery_value_media_url(audio) {
                return Some(url);
            }
        }
    }
    find_string_array_url(value, &["audio_urls", "audioUrls"])
}

fn normalize_gallery_media_url(platform: &str, raw_url: &str) -> Option<String> {
    let clean = raw_url.trim();
    if clean.starts_with("https://") {
        return Some(clean.to_string());
    }

    if platform == "twitter" && clean.starts_with("http://") {
        let mut parsed = reqwest::Url::parse(clean).ok()?;
        let host = parsed.host_str()?.to_lowercase();
        if host == "pbs.twimg.com" || host.ends_with(".twimg.com") {
            parsed.set_scheme("https").ok()?;
            return Some(parsed.to_string());
        }
    }

    None
}

fn gallery_url_from_stdout_line(line: &str) -> Option<&str> {
    let clean = line.trim();
    if clean.starts_with('|') {
        return None;
    }

    let first = clean.split_whitespace().next()?;
    if first.starts_with("https://") || first.starts_with("http://") {
        Some(first)
    } else {
        None
    }
}

fn gallery_value_dimension(value: &serde_json::Value, keys: &[&str]) -> Option<u32> {
    json_u32(value, keys).or_else(|| {
        ["original_info", "originalInfo", "dimensions"]
            .iter()
            .find_map(|key| value.get(*key).and_then(|nested| json_u32(nested, keys)))
    })
}

fn gallery_value_count(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    json_u64(value, keys).or_else(|| {
        ["stats", "statistics", "counts"]
            .iter()
            .find_map(|key| value.get(*key).and_then(|nested| json_u64(nested, keys)))
    })
}

fn gallery_value_author_name(value: &serde_json::Value) -> Option<String> {
    non_empty_string(json_nested_text(
        value,
        &[
            &["owner", "full_name"],
            &["owner", "username"],
            &["user", "full_name"],
            &["user", "username"],
            &["author", "nick"],
            &["user", "nick"],
            &["author", "name"],
            &["user", "name"],
            &["fullname"],
            &["full_name"],
            &["username"],
            &["author_name"],
            &["authorName"],
            &["uploader"],
            &["channel"],
            &["creator"],
        ],
    ))
}

fn gallery_value_author_handle(value: &serde_json::Value) -> Option<String> {
    non_empty_string(json_nested_text(
        value,
        &[
            &["owner", "username"],
            &["user", "username"],
            &["author", "name"],
            &["user", "name"],
            &["username"],
            &["screen_name"],
            &["screenName"],
            &["uploader_id"],
            &["channel_id"],
            &["author_handle"],
            &["authorHandle"],
        ],
    ))
}

fn gallery_value_avatar_url(value: &serde_json::Value) -> Option<String> {
    non_empty_string(json_nested_text(
        value,
        &[
            &["owner", "profile_pic_url_hd"],
            &["owner", "profile_pic_url"],
            &["owner", "profile_picture"],
            &["owner", "avatar_url"],
            &["user", "profile_pic_url_hd"],
            &["user", "profile_pic_url"],
            &["user", "profile_picture"],
            &["user", "avatar_url"],
            &["user_profile", "profile_pic_url_hd"],
            &["user_profile", "profile_pic_url"],
            &["userProfile", "profilePicUrlHd"],
            &["userProfile", "profilePicUrl"],
            &["author", "profile_image"],
            &["user", "profile_image"],
            &["author", "profile_image_url_https"],
            &["user", "profile_image_url_https"],
            &["profile_pic_url_hd"],
            &["profile_pic_url"],
            &["profile_image"],
            &["profile_image_url_https"],
            &["avatar_url"],
            &["avatarUrl"],
            &["uploader_avatar_url"],
            &["channel_avatar_url"],
        ],
    ))
}

fn instagram_direct_avatar_url(value: &serde_json::Value) -> Option<String> {
    let candidate = non_empty_string(json_text(
        value,
        &[
            "profile_pic_url_hd",
            "profile_pic_url",
            "profile_picture",
            "profile_image",
            "profile_image_url_https",
            "avatar_url",
            "avatarUrl",
            "uploader_avatar_url",
            "channel_avatar_url",
        ],
    ))?;

    instagram_avatar_url_allowed(&candidate).then_some(candidate)
}

pub(crate) fn normalize_instagram_handle(value: &str) -> String {
    value.trim().trim_start_matches('@').to_ascii_lowercase()
}

fn instagram_identity_value(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        let text = value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .or_else(|| value.as_u64().map(|number| number.to_string()))?;
        Some(text)
    })
}

fn instagram_author_from_node(value: &serde_json::Value) -> Option<CanonicalInstagramIdentity> {
    let handle = non_empty_string(json_text(value, &["username", "user_name", "handle"]));
    let name = non_empty_string(json_text(value, &["full_name", "fullname", "name"]))
        .or_else(|| handle.clone());
    let id = instagram_identity_value(value, &["pk", "id", "user_id", "owner_id"]);
    if id.is_none() && handle.is_none() {
        return None;
    }

    Some(CanonicalInstagramIdentity {
        id,
        name,
        handle,
        avatar_url: instagram_direct_avatar_url(value),
    })
}

fn canonical_instagram_author(value: &serde_json::Value) -> Option<CanonicalInstagramIdentity> {
    let expected_owner_id =
        instagram_identity_value(value, &["owner_id", "owner_pk", "owner_user_id", "user_id"]);
    let expected_handle = non_empty_string(json_text(
        value,
        &["username", "owner_username", "owner_user_name"],
    ))
    .map(|value| normalize_instagram_handle(&value))
    .filter(|value| !value.is_empty());

    let mut has_root_identity_candidate = false;
    let mut matched_root_identity: Option<CanonicalInstagramIdentity> = None;
    for key in ["owner", "user"] {
        let Some(node) = value.get(key) else { continue };
        let Some(author) = instagram_author_from_node(node) else {
            continue;
        };
        has_root_identity_candidate = true;

        let identity_matches = if let Some(expected_owner_id) = expected_owner_id.as_deref() {
            author.id.as_deref() == Some(expected_owner_id)
        } else if let Some(expected_handle) = expected_handle.as_deref() {
            author
                .handle
                .as_deref()
                .map(normalize_instagram_handle)
                .filter(|candidate| !candidate.is_empty())
                .is_some_and(|candidate| candidate == expected_handle)
        } else {
            false
        };
        if identity_matches {
            if let Some(identity) = matched_root_identity.as_mut() {
                identity.id = identity.id.clone().or(author.id);
                identity.name = identity.name.clone().or(author.name);
                identity.handle = identity.handle.clone().or(author.handle);
                identity.avatar_url = identity.avatar_url.clone().or(author.avatar_url);
            } else {
                matched_root_identity = Some(author);
            }
        }
    }

    if let Some(mut identity) = matched_root_identity {
        identity.id = identity.id.or(expected_owner_id);
        identity.handle = identity.handle.or(expected_handle);
        identity.name = identity.name.or_else(|| identity.handle.clone());
        return Some(identity);
    }

    if has_root_identity_candidate {
        return None;
    }

    let owner_id = instagram_identity_value(value, &["owner_id"])?;
    let handle = non_empty_string(json_text(value, &["username"]))
        .map(|handle| normalize_instagram_handle(&handle))
        .filter(|handle| !handle.is_empty())?;
    let name = non_empty_string(json_text(value, &["full_name", "fullname", "name"]));

    Some(CanonicalInstagramIdentity {
        id: Some(owner_id),
        name,
        handle: Some(handle),
        avatar_url: instagram_direct_avatar_url(value),
    })
}

fn gallery_instagram_author_metadata(
    value: &serde_json::Value,
) -> Option<CanonicalInstagramIdentity> {
    canonical_instagram_author(value)
        .or_else(|| {
            let handle = non_empty_string(json_text(value, &["username"]));
            let name = non_empty_string(json_text(value, &["full_name", "fullname", "name"]))
                .or_else(|| handle.clone());
            handle.map(|handle| CanonicalInstagramIdentity {
                id: None,
                name,
                handle: Some(handle),
                avatar_url: instagram_direct_avatar_url(value),
            })
        })
        .or_else(|| value.get("user").and_then(instagram_author_from_node))
}

#[cfg(test)]
pub(crate) fn find_instagram_avatar_url(value: &serde_json::Value) -> Option<String> {
    canonical_instagram_author(value).and_then(|author| author.avatar_url)
}

fn gallery_value_text(value: &serde_json::Value) -> Option<String> {
    non_empty_string(json_nested_text(
        value,
        &[
            &["content"],
            &["full_text"],
            &["text"],
            &["description"],
            &["caption"],
            &["title"],
        ],
    ))
}

fn gallery_value_display_date(value: &serde_json::Value) -> Option<String> {
    non_empty_string(json_nested_text(
        value,
        &[
            &["date"],
            &["created_at"],
            &["createdAt"],
            &["upload_date"],
            &["timestamp"],
        ],
    ))
}

fn instagram_metadata_confirms_post_media(value: &serde_json::Value) -> bool {
    let identity = json_text(
        value,
        &["media_id", "post_id", "post_shortcode", "shortcode", "id"],
    );
    if identity.trim().is_empty() {
        return false;
    }

    let has_dimensions = gallery_value_dimension(value, &["width", "w"]).is_some()
        && gallery_value_dimension(value, &["height", "h"]).is_some();
    let extension = json_text(value, &["extension", "format", "ext"]);
    let has_image_extension = supported_image_extension(extension.trim().trim_start_matches('.'));
    let media_kind =
        json_text(value, &["type", "typename", "kind", "subcategory"]).to_ascii_lowercase();
    let has_media_kind = ["post", "photo", "image", "carousel", "story", "highlight"]
        .iter()
        .any(|kind| media_kind.contains(kind));

    (has_dimensions || has_image_extension) && has_media_kind
}

fn push_gallery_item_from_url(
    raw_url: &str,
    metadata: Option<&serde_json::Value>,
    platform: &str,
    fallback_title: &str,
    items: &mut Vec<MediaItem>,
    seen: &mut HashSet<String>,
) {
    let preferred_tiktok_url = (platform == "tiktok")
        .then(|| metadata.filter(|value| gallery_value_is_video(value)))
        .flatten()
        .and_then(tiktok_preferred_video_url);
    let raw_url = preferred_tiktok_url.as_deref().unwrap_or(raw_url);
    let Some(url) = normalize_gallery_media_url(platform, raw_url) else {
        return;
    };

    if platform == "twitter"
        && metadata.is_some_and(|value| {
            json_text(value, &["type", "typename", "kind"]).eq_ignore_ascii_case("preview")
        })
    {
        // gallery-dl emits a Twitter preview immediately after its video.
        if let Some(video) = items
            .iter_mut()
            .rev()
            .find(|item| item.item_type == "video" && item.poster_ref.is_none())
        {
            video.poster_ref = Some(url);
        }
        return;
    }

    let mut extension = metadata
        .map(|value| media_extension_from_value(value, &url))
        .unwrap_or_else(|| extension_from_url(&url).unwrap_or_else(|| "jpg".to_string()));
    let metadata_is_video = metadata.is_some_and(gallery_value_is_video);
    if metadata_is_video && !supported_video_extension(&extension) {
        extension = "mp4".to_string();
    }
    let item_type = if metadata_is_video || supported_video_extension(&extension) {
        "video"
    } else if supported_image_extension(&extension) {
        "photo"
    } else {
        ""
    };

    if item_type.is_empty() {
        return;
    }

    if platform == "instagram" && item_type == "photo" && !instagram_likely_post_media_url(&url) {
        let confirmed_by_metadata = metadata
            .map(instagram_metadata_confirms_post_media)
            .unwrap_or(false);
        if !confirmed_by_metadata {
            return;
        }
    }

    if !seen.insert(url.clone()) {
        return;
    }

    let source_index = items.len();
    let title = metadata
        .map(|value| gallery_value_title(value, fallback_title))
        .unwrap_or_else(|| fallback_title.to_string());
    let mut id = metadata
        .map(|value| {
            if gallery_value_is_story(value) {
                json_text(
                    value,
                    &["media_id", "story_id", "pk", "id", "id_str", "media_key"],
                )
            } else {
                json_text(
                    value,
                    &[
                        "id",
                        "id_str",
                        "media_id",
                        "media_key",
                        "shortcode",
                        "filename",
                    ],
                )
            }
        })
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{}", platform, source_index));
    if items.iter().any(|item| item.id == id) {
        id = format!("{id}-{source_index}");
    }

    let canonical_instagram_identity = metadata
        .filter(|_| platform == "instagram")
        .and_then(canonical_instagram_author);
    let instagram_author = canonical_instagram_identity.clone().or_else(|| {
        metadata
            .filter(|_| platform == "instagram")
            .and_then(gallery_instagram_author_metadata)
    });
    let taken_at_ms = metadata
        .and_then(|value| {
            json_u64(
                value,
                &["taken_at", "timestamp", "date_utc", "created_at_timestamp"],
            )
        })
        .map(|value| {
            if value < 10_000_000_000 {
                value.saturating_mul(1000)
            } else {
                value
            }
        });
    let duration_ms = metadata.and_then(gallery_value_duration_ms);
    let audio_url = metadata
        .and_then(gallery_value_audio_url)
        .and_then(|raw| normalize_gallery_media_url(platform, &raw))
        .filter(|audio_url| audio_url != &url);
    let poster_ref = (item_type == "video")
        .then(|| metadata.and_then(gallery_value_poster_url))
        .flatten()
        .and_then(|raw| normalize_gallery_media_url(platform, &raw))
        .filter(|poster_url| poster_url != &url);
    let has_audio = audio_url.is_some()
        || metadata.is_some_and(|value| {
            value.get("has_audio").and_then(|item| item.as_bool()) == Some(true)
                || value
                    .get("audio")
                    .is_some_and(|item| !item.is_null() && item.as_bool() != Some(false))
                || non_empty_string(json_text(value, &["audio_codec", "acodec"]))
                    .is_some_and(|codec| codec != "none")
        });

    items.push(MediaItem {
        id: id.clone(),
        item_type: item_type.to_string(),
        source_index,
        preview_url: url,
        audio_url,
        width: metadata.and_then(|value| {
            gallery_value_dimension(value, &["width", "w", "thumbnail_width", "thumbnailWidth"])
        }),
        height: metadata.and_then(|value| {
            gallery_value_dimension(
                value,
                &["height", "h", "thumbnail_height", "thumbnailHeight"],
            )
        }),
        extension,
        is_story: metadata.map(gallery_value_is_story).unwrap_or(false),
        taken_at_ms,
        duration_ms,
        has_audio,
        preview_ref: Some(format!("item:{}", id)),
        poster_ref,
        title,
        author_id: instagram_author
            .as_ref()
            .and_then(|author| author.id.clone()),
        author_name: instagram_author
            .as_ref()
            .and_then(|author| author.name.clone())
            .or_else(|| {
                (platform != "instagram")
                    .then(|| metadata.and_then(gallery_value_author_name))
                    .flatten()
            }),
        author_handle: instagram_author
            .as_ref()
            .and_then(|author| author.handle.clone())
            .or_else(|| {
                (platform != "instagram")
                    .then(|| metadata.and_then(gallery_value_author_handle))
                    .flatten()
            }),
        avatar_url: if platform == "instagram" {
            instagram_author
                .as_ref()
                .and_then(|author| author.avatar_url.clone())
        } else {
            metadata.and_then(gallery_value_avatar_url)
        },
        avatar_data_url: None,
        canonical_instagram_identity,
        text: metadata.and_then(gallery_value_text),
        display_date: metadata.and_then(gallery_value_display_date),
        reply_count: metadata.and_then(|value| {
            gallery_value_count(
                value,
                &[
                    "reply_count",
                    "replies_count",
                    "comments",
                    "comment_count",
                    "comments_count",
                ],
            )
        }),
        retweet_count: metadata.and_then(|value| {
            gallery_value_count(value, &["retweet_count", "retweets_count", "repost_count"])
        }),
        like_count: metadata.and_then(|value| {
            gallery_value_count(
                value,
                &["favorite_count", "like_count", "likes_count", "likes"],
            )
        }),
        view_count: metadata
            .and_then(|value| gallery_value_count(value, &["view_count", "views_count"])),
    });
}

fn gallery_value_is_story(value: &serde_json::Value) -> bool {
    let combined = [
        json_text(value, &["subcategory"]),
        json_text(value, &["type"]),
        json_text(value, &["typename"]),
        json_text(value, &["kind"]),
        json_text(value, &["path"]),
    ]
    .join(" ")
    .to_lowercase();

    !combined.contains("highlight") && (combined.contains("story") || combined.contains("stories"))
}

fn gallery_value_title(value: &serde_json::Value, fallback: &str) -> String {
    gallery_value_text(value).unwrap_or_else(|| fallback.to_string())
}

fn gallery_value_duration_ms(value: &serde_json::Value) -> Option<u64> {
    if let Some(milliseconds) = json_u64(value, &["duration_ms"]) {
        return Some(milliseconds);
    }
    let seconds = value.get("duration").and_then(|duration| {
        duration
            .as_f64()
            .or_else(|| duration.as_str()?.trim().parse::<f64>().ok())
    })?;
    (seconds.is_finite() && seconds >= 0.0 && seconds <= u64::MAX as f64 / 1000.0)
        .then(|| (seconds * 1000.0).round() as u64)
}

pub(crate) fn collect_gallery_items_from_value(
    value: &serde_json::Value,
    platform: &str,
    fallback_title: &str,
    items: &mut Vec<MediaItem>,
    seen: &mut HashSet<String>,
) {
    collect_gallery_items_from_value_at(value, platform, fallback_title, items, seen, false);
}

#[derive(Default)]
struct TwitterMessageRecords {
    directories: Vec<serde_json::Value>,
    media: Vec<(String, serde_json::Value)>,
}

#[derive(Default)]
pub(crate) struct GalleryInventory {
    pub(crate) items: Vec<MediaItem>,
    pub(crate) twitter_quote: Option<TwitterQuoteContext>,
    pub(crate) twitter_post: Option<TwitterPostMetadata>,
}

fn collect_twitter_message_records(
    value: &serde_json::Value,
    records: &mut TwitterMessageRecords,
) {
    let serde_json::Value::Array(array) = value else {
        return;
    };

    match array.first().and_then(|item| item.as_i64()) {
        Some(2) => {
            if let Some(metadata) = array.get(1).filter(|item| item.is_object()) {
                records.directories.push(metadata.clone());
            }
            return;
        }
        Some(3) => {
            if let (Some(url), Some(metadata)) = (
                array.get(1).and_then(|item| item.as_str()),
                array.get(2).filter(|item| item.is_object()),
            ) {
                records.media.push((url.to_string(), metadata.clone()));
            }
            return;
        }
        _ => {}
    }

    for item in array {
        collect_twitter_message_records(item, records);
    }
}

fn twitter_metadata_id(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| item.as_u64().map(|number| number.to_string()))
        })
        .unwrap_or_default()
}

fn twitter_post_metadata(value: &serde_json::Value) -> Option<TwitterPostMetadata> {
    let id = twitter_metadata_id(value, "tweet_id");
    if id.is_empty() {
        return None;
    }

    let is_verified = value
        .get("author")
        .and_then(|author| author.get("verified"))
        .and_then(|verified| verified.as_bool())
        .unwrap_or(false);

    Some(TwitterPostMetadata {
        id,
        author_name: gallery_value_author_name(value).unwrap_or_default(),
        author_handle: gallery_value_author_handle(value).unwrap_or_default(),
        avatar_url: gallery_value_avatar_url(value),
        text: gallery_value_text(value),
        display_date: gallery_value_display_date(value),
        is_verified,
        reply_count: gallery_value_count(value, &["reply_count"]),
        retweet_count: gallery_value_count(value, &["retweet_count"]),
        like_count: gallery_value_count(value, &["favorite_count", "like_count"]),
        view_count: gallery_value_count(value, &["view_count"]),
    })
}

fn twitter_primary_post(records: &TwitterMessageRecords) -> Option<TwitterPostMetadata> {
    records
        .directories
        .iter()
        .find(|metadata| {
            let quote_id = twitter_metadata_id(metadata, "quote_id");
            !twitter_metadata_id(metadata, "tweet_id").is_empty()
                && (quote_id.is_empty() || quote_id == "0")
        })
        .and_then(twitter_post_metadata)
}

fn twitter_quote_context(
    records: &TwitterMessageRecords,
    items: &[MediaItem],
) -> Option<TwitterQuoteContext> {
    let outer = twitter_primary_post(records)?;
    let quoted_record = records.directories.iter().find(|metadata| {
        twitter_metadata_id(metadata, "quote_id") == outer.id
            && twitter_metadata_id(metadata, "tweet_id") != outer.id
    })?;
    let quoted = twitter_post_metadata(quoted_record)?;

    let quoted_urls = records
        .media
        .iter()
        .filter(|(_, metadata)| twitter_metadata_id(metadata, "quote_id") == outer.id)
        .filter_map(|(raw_url, metadata)| {
            normalize_gallery_media_url("twitter", raw_url).or_else(|| {
                gallery_value_media_url(metadata)
                    .and_then(|url| normalize_gallery_media_url("twitter", &url))
            })
        })
        .collect::<HashSet<_>>();
    let quoted_media_indexes = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| quoted_urls.contains(&item.preview_url).then_some(index))
        .collect();

    Some(TwitterQuoteContext {
        outer,
        quoted,
        quoted_media_indexes,
    })
}

fn instagram_metadata_branch_is_profile_image(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();

    normalized.contains("profile_pic")
        || normalized.contains("profilepic")
        || normalized.contains("profile_image")
        || normalized.contains("profileimage")
        || normalized.contains("profile_picture")
        || normalized.contains("hd_profile")
        || normalized.contains("avatar")
}

fn instagram_metadata_branch_is_noncanonical_social_data(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', ' '], "_");
    [
        "comment",
        "comments",
        "reply",
        "replies",
        "tagged_user",
        "tagged_users",
        "edge_media_to_tagged_user",
        "liker",
        "likers",
        "likes",
    ]
    .iter()
    .any(|blocked| normalized == *blocked || normalized.starts_with(&format!("{blocked}_")))
}

fn collect_gallery_items_from_value_at(
    value: &serde_json::Value,
    platform: &str,
    fallback_title: &str,
    items: &mut Vec<MediaItem>,
    seen: &mut HashSet<String>,
    skip_media: bool,
) {
    match value {
        serde_json::Value::Array(array) => {
            if array.first().and_then(|item| item.as_i64()) == Some(2) {
                return;
            }

            if !skip_media && array.first().and_then(|item| item.as_i64()) == Some(3) {
                if let Some(raw_url) = array.get(1).and_then(|item| item.as_str()) {
                    let metadata = array.get(2).filter(|item| item.is_object());
                    let canonical_message_url =
                        normalize_gallery_media_url(platform, raw_url).is_some();
                    push_gallery_item_from_url(
                        raw_url,
                        metadata,
                        platform,
                        fallback_title,
                        items,
                        seen,
                    );
                    if canonical_message_url {
                        // A gallery-dl Message.Url tuple already carries the
                        // canonical downloadable resource at index 1. Its
                        // metadata may also contain Instagram `display_url`,
                        // which is only a JPEG cover for video Stories.
                        return;
                    }
                }
            }

            for item in array {
                collect_gallery_items_from_value_at(
                    item,
                    platform,
                    fallback_title,
                    items,
                    seen,
                    skip_media,
                );
            }
        }
        serde_json::Value::Object(map) => {
            if !skip_media {
                if let Some(url) = gallery_value_media_url(value) {
                    push_gallery_item_from_url(
                        &url,
                        Some(value),
                        platform,
                        fallback_title,
                        items,
                        seen,
                    );
                }
            }

            for (key, nested) in map {
                if nested.is_array() || nested.is_object() {
                    let nested_skip_media = skip_media
                        || (platform == "instagram"
                            && (instagram_metadata_branch_is_profile_image(key)
                                || instagram_metadata_branch_is_noncanonical_social_data(key)));

                    collect_gallery_items_from_value_at(
                        nested,
                        platform,
                        fallback_title,
                        items,
                        seen,
                        nested_skip_media,
                    );
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn gallery_stdout_to_inventory(
    stdout: &str,
    platform: &str,
    fallback_title: &str,
) -> Result<GalleryInventory, String> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut twitter_records = TwitterMessageRecords::default();
    let mut parse_errors = Vec::new();
    let mut observed_output = Vec::new();
    let clean_stdout = stdout.trim();
    let mut parsed_full_json = false;

    if clean_stdout.starts_with('{') || clean_stdout.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(clean_stdout) {
            parsed_full_json = true;
            if platform == "twitter" {
                collect_twitter_message_records(&value, &mut twitter_records);
            }
            collect_gallery_items_from_value(
                &value,
                platform,
                fallback_title,
                &mut items,
                &mut seen,
            );

            let twitter_quote = (platform == "twitter")
                .then(|| twitter_quote_context(&twitter_records, &items))
                .flatten();
            let twitter_post = (platform == "twitter")
                .then(|| twitter_primary_post(&twitter_records))
                .flatten();
            if !items.is_empty() || twitter_quote.is_some() || twitter_post.is_some() {
                return Ok(GalleryInventory {
                    items,
                    twitter_quote,
                    twitter_post,
                });
            }
        }
    }

    if parsed_full_json {
        return Err(format!(
            "gallery-dl JSON ciktisi indirilebilir medya URL'si icermedi. Ornek cikti: {}",
            sanitize_report_text(&limit_report_text(clean_stdout, 600))
        ));
    }

    for line in stdout.lines() {
        let clean = line.trim();
        if clean.is_empty() {
            continue;
        }

        if observed_output.len() < 4 {
            observed_output.push(limit_report_text(clean, 240));
        }

        if let Some(raw_url) = gallery_url_from_stdout_line(clean) {
            push_gallery_item_from_url(
                raw_url,
                None,
                platform,
                fallback_title,
                &mut items,
                &mut seen,
            );
            continue;
        }

        if !clean.starts_with('{') && !clean.starts_with('[') {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(clean) {
            Ok(value) => {
                if platform == "twitter" {
                    collect_twitter_message_records(&value, &mut twitter_records);
                }
                collect_gallery_items_from_value(
                    &value,
                    platform,
                    fallback_title,
                    &mut items,
                    &mut seen,
                );
            }
            Err(err) => parse_errors.push(err.to_string()),
        }
    }

    let twitter_quote = (platform == "twitter")
        .then(|| twitter_quote_context(&twitter_records, &items))
        .flatten();
    let twitter_post = (platform == "twitter")
        .then(|| twitter_primary_post(&twitter_records))
        .flatten();

    if items.is_empty()
        && twitter_quote.is_none()
        && twitter_post.is_none()
        && !observed_output.is_empty()
        && parse_errors.is_empty()
    {
        return Err(format!(
            "gallery-dl cikti verdi ama indirilebilir medya URL'si okunamadi. Ornek cikti: {}",
            sanitize_report_text(&observed_output.join(" | "))
        ));
    }

    if items.is_empty()
        && twitter_quote.is_none()
        && twitter_post.is_none()
        && !parse_errors.is_empty()
    {
        return Err(format!(
            "gallery-dl JSON sonucu okunamadi: {}",
            parse_errors.join(" | ")
        ));
    }

    Ok(GalleryInventory {
        items,
        twitter_quote,
        twitter_post,
    })
}

pub(crate) fn gallery_stdout_to_items(
    stdout: &str,
    platform: &str,
    fallback_title: &str,
) -> Result<Vec<MediaItem>, String> {
    gallery_stdout_to_inventory(stdout, platform, fallback_title)
        .map(|inventory| inventory.items)
}

pub(crate) fn media_content_kind(items: &[MediaItem]) -> String {
    if items.is_empty() {
        return "unknown".to_string();
    }

    if items.iter().all(|item| item.is_story) {
        return "story".to_string();
    }

    if items.len() > 1 {
        return "carousel".to_string();
    }

    items
        .first()
        .map(|item| item.item_type.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn propagate_media_item_metadata(items: &mut [MediaItem]) {
    let canonical_author = items.iter().find_map(|item| {
        let id = item
            .author_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let handle = item
            .author_handle
            .as_deref()
            .map(normalize_instagram_handle)
            .filter(|value| !value.is_empty());
        (id.is_some() || handle.is_some()).then(|| {
            (
                id,
                item.author_name.clone(),
                handle,
                item.avatar_url.clone(),
            )
        })
    });
    let text = items.iter().find_map(|item| item.text.clone());
    let display_date = items.iter().find_map(|item| item.display_date.clone());
    let reply_count = items.iter().find_map(|item| item.reply_count);
    let retweet_count = items.iter().find_map(|item| item.retweet_count);
    let like_count = items.iter().find_map(|item| item.like_count);
    let view_count = items.iter().find_map(|item| item.view_count);

    for item in items {
        if let Some((author_id, author_name, author_handle, avatar_url)) = &canonical_author {
            item.author_id = author_id.clone();
            item.author_name = author_name.clone();
            item.author_handle = author_handle.clone();
            item.avatar_url = avatar_url.clone();
        }
        if item.text.is_none() {
            item.text = text.clone();
        }
        if item.display_date.is_none() {
            item.display_date = display_date.clone();
        }
        if item.reply_count.is_none() {
            item.reply_count = reply_count;
        }
        if item.retweet_count.is_none() {
            item.retweet_count = retweet_count;
        }
        if item.like_count.is_none() {
            item.like_count = like_count;
        }
        if item.view_count.is_none() {
            item.view_count = view_count;
        }
    }
}
