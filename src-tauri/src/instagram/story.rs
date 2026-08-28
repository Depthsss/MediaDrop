use super::parser::normalize_instagram_handle;
use crate::{host_matches, MediaAnalysis, MediaItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstagramStoryRequest {
    Direct {
        username: String,
        requested_media_id: Option<String>,
    },
    Share {
        token: String,
        query_media_id: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstagramShareStoryTarget {
    pub(crate) username: String,
    pub(crate) media_id: String,
}

pub(crate) fn instagram_highlight_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !parsed
        .host_str()
        .map(|host| host_matches(&host.to_ascii_lowercase(), "instagram.com"))
        .unwrap_or(false)
    {
        return false;
    }
    matches!(
        parsed
            .path_segments()
            .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
            .as_deref(),
        Some([kind, target, ..])
            if kind.eq_ignore_ascii_case("stories") && target.eq_ignore_ascii_case("highlights")
    )
}

fn canonical_instagram_story_media_id(value: &str) -> Option<String> {
    let clean = value.trim().split('_').next().unwrap_or("");
    (!clean.is_empty() && clean.chars().all(|ch| ch.is_ascii_digit())).then(|| clean.to_string())
}

fn valid_instagram_story_username(value: &str) -> Option<String> {
    let clean = normalize_instagram_handle(value);
    (!clean.is_empty()
        && clean.len() <= 30
        && clean
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.')))
    .then_some(clean)
}

pub(crate) fn instagram_story_profile_url(username: &str) -> Result<String, String> {
    let username = valid_instagram_story_username(username)
        .ok_or_else(|| "Instagram Story schema: canonical username gecersiz.".to_string())?;
    let mut url = reqwest::Url::parse("https://www.instagram.com/stories/")
        .map_err(|_| "Instagram Story profil linki olusturulamadi.".to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Instagram Story profil linki olusturulamadi.".to_string())?
        .pop_if_empty()
        .push(&username);
    Ok(url.to_string())
}

pub(crate) fn instagram_story_request(url: &str) -> Option<InstagramStoryRequest> {
    if instagram_highlight_url(url) {
        return None;
    }
    let parsed = reqwest::Url::parse(url).ok()?;
    if !parsed
        .host_str()
        .map(|host| host_matches(&host.to_ascii_lowercase(), "instagram.com"))
        .unwrap_or(false)
    {
        return None;
    }
    let segments = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [kind, username, item_id, ..] if kind.eq_ignore_ascii_case("stories") => {
            Some(InstagramStoryRequest::Direct {
                username: valid_instagram_story_username(username)?,
                requested_media_id: Some(canonical_instagram_story_media_id(item_id)?),
            })
        }
        [kind, username] if kind.eq_ignore_ascii_case("stories") => {
            Some(InstagramStoryRequest::Direct {
                username: valid_instagram_story_username(username)?,
                requested_media_id: None,
            })
        }
        [kind, share_token, ..]
            if kind.eq_ignore_ascii_case("s")
                && !share_token.trim().is_empty()
                && share_token.len() <= 512 =>
        {
            let query_media_id = match parsed
                .query_pairs()
                .find(|(key, _)| key.eq_ignore_ascii_case("story_media_id"))
            {
                Some((_, value)) => Some(canonical_instagram_story_media_id(&value)?),
                None => None,
            };
            Some(InstagramStoryRequest::Share {
                token: (*share_token).to_string(),
                query_media_id,
            })
        }
        _ => None,
    }
}

pub(crate) fn resolve_instagram_share_story_target(
    items: &[MediaItem],
    query_media_id: Option<&str>,
) -> Result<InstagramShareStoryTarget, String> {
    let query_media_id = query_media_id.and_then(canonical_instagram_story_media_id);
    let mut candidates = Vec::new();

    for item in items.iter().filter(|item| item.is_story) {
        let Some(username) = item
            .canonical_instagram_identity
            .as_ref()
            .and_then(|identity| identity.handle.as_deref())
            .and_then(valid_instagram_story_username)
        else {
            continue;
        };
        let Some(media_id) = canonical_instagram_story_media_id(&item.id) else {
            continue;
        };
        if query_media_id
            .as_deref()
            .is_some_and(|requested| requested != media_id)
        {
            continue;
        }
        if !candidates
            .iter()
            .any(|candidate: &InstagramShareStoryTarget| {
                candidate.username == username && candidate.media_id == media_id
            })
        {
            candidates.push(InstagramShareStoryTarget { username, media_id });
        }
    }

    if candidates.len() != 1 {
        return Err(
            "Instagram Story schema: share hedefinin canonical owner/media_id bilgisi belirsiz."
                .to_string(),
        );
    }
    Ok(candidates.remove(0))
}

pub(crate) fn canonical_owner_story_items(
    items: Vec<MediaItem>,
    expected_username: &str,
) -> Result<Vec<MediaItem>, String> {
    let expected_username = valid_instagram_story_username(expected_username)
        .ok_or_else(|| "Instagram Story schema: canonical username gecersiz.".to_string())?;
    let mut accepted = Vec::new();

    for mut item in items.into_iter().filter(|item| item.is_story) {
        let username = item
            .canonical_instagram_identity
            .as_ref()
            .and_then(|identity| identity.handle.as_deref())
            .and_then(valid_instagram_story_username)
            .ok_or_else(|| "Instagram Story schema: Story owner bilgisi bulunamadi.".to_string())?;
        if username != expected_username {
            return Err("Instagram Story schema: Story owner eslesmesi bozuldu.".to_string());
        }
        item.id = canonical_instagram_story_media_id(&item.id)
            .ok_or_else(|| "Instagram Story schema: media_id gecersiz.".to_string())?;
        accepted.push(item);
    }

    if accepted.is_empty() {
        return Err("Instagram hikaye bulunamadi: aktif Story listesi bos.".to_string());
    }
    Ok(accepted)
}

pub(crate) fn apply_story_policy(analysis: &mut MediaAnalysis, clean_url: &str) {
    let Some(story_request) = instagram_story_request(clean_url) else {
        return;
    };

    let link_requested_media_id = match &story_request {
        InstagramStoryRequest::Direct {
            requested_media_id, ..
        } => requested_media_id.clone(),
        InstagramStoryRequest::Share { .. } => None,
    };
    let requested_item_id = analysis
        .requested_item_id
        .as_deref()
        .and_then(canonical_instagram_story_media_id)
        .or(link_requested_media_id);
    for item in &mut analysis.items {
        if item.is_story {
            if let Some(media_id) = canonical_instagram_story_media_id(&item.id) {
                item.id = media_id;
            }
        }
    }
    analysis.items.sort_by(|left, right| {
        left.taken_at_ms
            .unwrap_or(u64::MAX)
            .cmp(&right.taken_at_ms.unwrap_or(u64::MAX))
            .then_with(|| {
                canonical_instagram_story_media_id(&left.id)
                    .and_then(|id| id.parse::<u64>().ok())
                    .cmp(
                        &canonical_instagram_story_media_id(&right.id)
                            .and_then(|id| id.parse::<u64>().ok()),
                    )
            })
            .then_with(|| left.source_index.cmp(&right.source_index))
    });
    for (index, item) in analysis.items.iter_mut().enumerate() {
        item.source_index = index;
    }
    analysis.content_kind = "story".to_string();
    analysis.requested_item_id = requested_item_id.clone();
    let matched_index = requested_item_id
        .as_deref()
        .and_then(|requested| analysis.items.iter().position(|item| item.id == requested));
    analysis.initial_index = matched_index.unwrap_or(0);
    if requested_item_id.is_some()
        && matched_index.is_none()
        && !analysis
            .warnings
            .iter()
            .any(|warning| warning == "requestedStoryUnavailable")
    {
        analysis
            .warnings
            .push("requestedStoryUnavailable".to_string());
    }
}
