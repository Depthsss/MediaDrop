export function metadataString(value) {
  if (value === null || value === undefined) return "";
  return String(value).trim();
}

export function firstMetadataString(...values) {
  return values.map(metadataString).find(Boolean) || "";
}

export function formatMediaDuration(seconds, unknownLabel = "Süre bilinmiyor") {
  const totalSeconds = Number(seconds);
  if (!Number.isFinite(totalSeconds) || totalSeconds <= 0) return unknownLabel;

  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const secs = Math.floor(totalSeconds % 60);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`
    : `${minutes}:${String(secs).padStart(2, "0")}`;
}

export function isTwitterPostDownloadIntent(value) {
  return metadataString(value) === "download_twitter_post";
}

function hostMatches(host, domain) {
  return host === domain || host.endsWith(`.${domain}`);
}

function urlHost(value) {
  let clean = metadataString(value);
  if (!clean) return "";

  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(clean)) {
    clean = `https://${clean}`;
  }

  try {
    return new URL(clean).hostname.toLowerCase().replace(/^www\./, "");
  } catch {
    return "";
  }
}

export function normalizeTwitterHandle(value) {
  const clean = metadataString(value).replace(/^@+/, "");
  const handle = clean.split(/[/?#\s]/)[0]?.replace(/^@+/, "") || "";
  return handle ? `@${handle}` : "";
}

export function twitterHandleFromUrl(value) {
  let clean = metadataString(value);
  if (!clean) return "";

  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(clean)) {
    clean = `https://${clean}`;
  }

  try {
    const parsed = new URL(clean);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, "");

    if (!hostMatches(host, "twitter.com") && !hostMatches(host, "x.com")) {
      return "";
    }

    const segment = parsed.pathname.split("/").filter(Boolean)[0] || "";
    if (!segment || ["i", "intent", "share"].includes(segment.toLowerCase())) {
      return "";
    }

    return normalizeTwitterHandle(segment);
  } catch {
    return "";
  }
}

export function formatMetadataDate(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) return "";

  return new Intl.DateTimeFormat("tr-TR", {
    day: "2-digit",
    month: "short",
    year: "numeric",
  }).format(date);
}

export function twitterDateFromMetadata(value) {
  const clean = metadataString(value);
  if (!clean) return null;

  const compact = clean.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compact) {
    const [, year, month, day] = compact;
    return new Date(Date.UTC(Number(year), Number(month) - 1, Number(day)));
  }

  const numeric = Number(clean);
  if (Number.isFinite(numeric) && numeric > 0) {
    const date = new Date(numeric > 10_000_000_000 ? numeric : numeric * 1000);
    return Number.isNaN(date.getTime()) ? null : date;
  }

  const naiveUtc = /^\d{4}-\d{2}-\d{2}(?:[ T]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?)?$/.test(clean);
  const normalized = naiveUtc
    ? clean.includes(":")
      ? `${clean.replace(" ", "T")}Z`
      : `${clean}T00:00:00Z`
    : clean;
  const date = new Date(normalized);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatTwitterDisplayDate(value) {
  return formatMetadataDate(twitterDateFromMetadata(value)) || metadataString(value);
}

export function twitterDisplayDateFromUploadDate(value) {
  const clean = metadataString(value);
  return /^\d{8}$/.test(clean) ? formatTwitterDisplayDate(clean) : "";
}

export function cleanTwitterPostText(text) {
  const withoutUrls = metadataString(text)
    .replace(/https?:\/\/\S+/gi, " ")
    .replace(/\bwww\.\S+/gi, " ")
    .replace(/\b(?:x\.com|twitter\.com|t\.co)\/\S+/gi, " ");
  const clean = withoutUrls
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n[ \t]+/g, "\n")
    .replace(/[ \t]{2,}/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  return clean || "Gönderi metni alınamadı.";
}

export function twitterTextPostAvailable(metadata = {}, hasDownloadableMedia = false) {
  const postText = cleanTwitterPostText(metadata?.text);
  return (
    !hasDownloadableMedia &&
    postText !== cleanTwitterPostText("") &&
    !/^twitter video #\d+$/i.test(postText)
  );
}

export function twitterDisplayNameFromTitle(info = {}) {
  const source = firstMetadataString(info?.fulltitle, info?.title);
  const clean = metadataString(source).replace(/\s+/g, " ");
  const match = clean.match(/^(.{1,80}?)\s+on\s+(?:X|Twitter)\s*:/i);
  if (!match) return "";

  const candidate = metadataString(match[1]).replace(/^["“”]+|["“”]+$/g, "").trim();
  if (!candidate || /https?:\/\/|\b(?:x\.com|twitter\.com)\b/i.test(candidate)) return "";

  return candidate;
}

export function twitterDisplayNameWithRichFallback(primary, fallback = "") {
  const cleanPrimary = metadataString(primary);
  const cleanFallback = metadataString(fallback);

  if (!cleanPrimary) return cleanFallback;
  if (!cleanFallback) return cleanPrimary;

  const lowerPrimary = cleanPrimary.toLocaleLowerCase("tr-TR");
  const lowerFallback = cleanFallback.toLocaleLowerCase("tr-TR");

  return lowerFallback.startsWith(lowerPrimary) && cleanFallback.length > cleanPrimary.length
    ? cleanFallback
    : cleanPrimary;
}

export function safeTwitterAvatarDataUrl(value) {
  const clean = metadataString(value);
  if (!/^data:image\/(?:png|jpe?g|webp);base64,/i.test(clean)) return "";
  return clean;
}

function normalizeTwitterEmbeddedPost(value = {}) {
  const source = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  const rawText = metadataString(source.text);
  return {
    id: metadataString(source.id),
    text: rawText ? cleanTwitterPostText(rawText) : "",
    exportText: rawText,
    authorName: metadataString(source.authorName) || "X/Twitter",
    authorHandle: normalizeTwitterHandle(source.authorHandle),
    displayDate: metadataString(source.displayDate),
    avatarUrl: metadataString(source.avatarUrl),
    avatarDataUrl: safeTwitterAvatarDataUrl(source.avatarDataUrl),
    isVerified: metadataBoolean(source.isVerified),
    replyCount: metadataCountFromValue(source.replyCount),
    retweetCount: metadataCountFromValue(source.retweetCount),
    likeCount: metadataCountFromValue(source.likeCount),
    viewCount: metadataCountFromValue(source.viewCount),
  };
}

export function normalizeTwitterQuoteContext(value = {}) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const outer = normalizeTwitterEmbeddedPost(value.outer);
  const quoted = normalizeTwitterEmbeddedPost(value.quoted);
  if (!outer.id || !quoted.id || outer.id === quoted.id) return null;

  const quotedMediaIndexes = [...new Set(
    (Array.isArray(value.quotedMediaIndexes) ? value.quotedMediaIndexes : [])
      .map(Number)
      .filter((index) => Number.isInteger(index) && index >= 0)
  )];

  return { outer, quoted, quotedMediaIndexes };
}

export function twitterAvatarInitial(value) {
  const source = metadataString(value).replace(/^@+/, "") || "X";
  const chars = [...source].filter((char) => /[\p{L}\p{N}]/u.test(char));
  return (chars[0] || "X").toUpperCase();
}

export function getThumbnailCandidates(info = {}) {
  const urls = [];

  if (info.thumbnail) urls.push(info.thumbnail);

  const thumbnails = Array.isArray(info.thumbnails) ? info.thumbnails : [];
  const sorted = thumbnails
    .filter((item) => item && item.url)
    .sort((a, b) => (b.width || 0) - (a.width || 0));

  for (const item of sorted) {
    urls.push(item.url);
  }

  return [...new Set(urls.filter(Boolean))];
}

const TWITTER_AVATAR_ROOT_FIELDS = [
  "profile_image_url_https",
  "profile_image_url",
  "profile_image_url_large",
  "profile_image_url_original",
  "profile_image_url_normal",
  "profile_image_url_bigger",
  "profile_image_url_mini",
  "profile_image",
  "uploader_avatar_url",
  "uploader_avatar",
  "uploader_thumbnail",
  "channel_avatar_url",
  "channel_avatar",
  "channel_thumbnail",
  "channel_favicon",
  "author_avatar_url",
  "author_avatar",
  "author_thumbnail",
  "creator_avatar_url",
  "creator_avatar",
  "creator_thumbnail",
  "avatar_url",
  "avatar",
];

const TWITTER_AVATAR_ENTITY_FIELDS = [
  ...TWITTER_AVATAR_ROOT_FIELDS,
  "thumbnail",
  "image",
  "picture",
  "url",
];

const TWITTER_AVATAR_ENTITY_KEYS = [
  "author",
  "uploader",
  "channel",
  "creator",
  "owner",
  "user",
  "account",
  "profile",
];

export function normalizeTwitterImageUrl(value) {
  let clean = metadataString(value);
  if (!clean) return "";

  if (clean.startsWith("//")) {
    clean = `https:${clean}`;
  }

  try {
    const parsed = new URL(clean);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, "");

    if (parsed.protocol === "http:" && twitterImageHostAllowed(host)) {
      parsed.protocol = "https:";
      clean = parsed.toString();
    }
  } catch {}

  return clean;
}

export function twitterImageHostAllowed(host) {
  return host === "pbs.twimg.com" || host === "abs.twimg.com" || hostMatches(host, "twimg.com");
}

export function isLikelyTwitterProfileImageUrl(value) {
  const clean = normalizeTwitterImageUrl(value);
  if (!clean) return false;

  try {
    const parsed = new URL(clean);
    return /\/profile_images\//i.test(parsed.pathname);
  } catch {
    return /\/profile_images\//i.test(clean);
  }
}

export function isTwitterAvatarImageCandidate(value) {
  const clean = normalizeTwitterImageUrl(value);
  if (!clean) return false;
  if (safeTwitterAvatarDataUrl(clean)) return true;

  try {
    const parsed = new URL(clean);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, "");

    return parsed.protocol === "https:" && twitterImageHostAllowed(host);
  } catch {
    return false;
  }
}

export function twitterAvatarCandidateFromValue(
  value,
  fields = TWITTER_AVATAR_ENTITY_FIELDS,
  seen = new Set()
) {
  if (value === null || value === undefined) return "";

  if (typeof value === "string" || typeof value === "number") {
    const clean = normalizeTwitterImageUrl(value);
    return isTwitterAvatarImageCandidate(clean) ? clean : "";
  }

  if (Array.isArray(value)) {
    const sorted = [...value].sort((a, b) => (b?.width || 0) - (a?.width || 0));

    for (const item of sorted) {
      const found = twitterAvatarCandidateFromValue(item, fields, seen);
      if (found) return found;
    }

    return "";
  }

  if (typeof value !== "object" || seen.has(value)) return "";
  seen.add(value);

  for (const field of fields) {
    if (!Object.prototype.hasOwnProperty.call(value, field)) continue;

    const found = twitterAvatarCandidateFromValue(
      value[field],
      TWITTER_AVATAR_ENTITY_FIELDS,
      seen
    );
    if (found) return found;
  }

  return "";
}

export function twitterAvatarMetadataDebugInfo(info = {}, selectedUrl = "") {
  const candidates = [];
  const seen = new Set();
  const visit = (value, path = "root", depth = 0) => {
    if (value === null || value === undefined || depth > 5 || candidates.length >= 40) return;

    if (typeof value === "string" || typeof value === "number") {
      const clean = normalizeTwitterImageUrl(value);
      if (!clean) return;

      const host = urlHost(clean);
      const looksRelevant =
        /avatar|profile|image|thumbnail|thumb|picture/i.test(path) ||
        /twimg\.com|pbs\.twimg\.com|abs\.twimg\.com/i.test(clean);

      if (!looksRelevant) return;

      let status = "accepted";
      if (safeTwitterAvatarDataUrl(clean)) {
        status = "data_url";
      } else if (!/^https:\/\//i.test(clean)) {
        status = "rejected_non_https";
      } else if (!twitterImageHostAllowed(host)) {
        status = "rejected_host";
      } else if (/thumbnail|thumb/i.test(path) && !isLikelyTwitterProfileImageUrl(clean)) {
        status = "rejected_thumbnail_not_profile";
      }

      candidates.push({
        path,
        host: host || "data-url",
        status,
      });
      return;
    }

    if (typeof value !== "object" || seen.has(value)) return;
    seen.add(value);

    if (Array.isArray(value)) {
      value.slice(0, 16).forEach((item, index) => visit(item, `${path}[${index}]`, depth + 1));
      return;
    }

    for (const [key, item] of Object.entries(value)) {
      visit(item, `${path}.${key}`, depth + 1);
      if (candidates.length >= 40) break;
    }
  };

  visit(info);

  return {
    candidateCount: candidates.length,
    acceptedCount: candidates.filter((candidate) => candidate.status === "accepted").length,
    hosts: [...new Set(candidates.map((candidate) => candidate.host))].filter(Boolean),
    selectedHost: urlHost(selectedUrl),
    candidates,
  };
}

export function twitterAvatarThumbnailCandidate(info = {}) {
  const candidates = getThumbnailCandidates(info);

  for (const candidate of candidates) {
    const clean = normalizeTwitterImageUrl(candidate);
    if (isTwitterAvatarImageCandidate(clean) && isLikelyTwitterProfileImageUrl(clean)) {
      return clean;
    }
  }

  return "";
}

export function twitterProfileImageCandidate(info = {}) {
  const direct = twitterAvatarCandidateFromValue(info, TWITTER_AVATAR_ROOT_FIELDS);
  if (direct) return direct;

  for (const key of TWITTER_AVATAR_ENTITY_KEYS) {
    const found = twitterAvatarCandidateFromValue(info?.[key]);
    if (found) return found;
  }

  return twitterAvatarThumbnailCandidate(info);
}

export function isRemoteImageCandidate(value) {
  return /^https:\/\//i.test(normalizeTwitterImageUrl(value));
}

export function metadataNumber(value) {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }

  const clean = metadataString(value);
  if (!clean) return null;

  const compact = clean.replace(/\s+/g, "").toLowerCase();
  const suffix = compact.match(/([kmb])$/)?.[1] || "";
  const numberText = compact
    .replace(/[kmb]$/i, "")
    .replace(/,/g, ".")
    .replace(/[^\d.]/g, "");
  const number = Number(numberText);

  if (!Number.isFinite(number)) return null;

  if (suffix === "k") return number * 1_000;
  if (suffix === "m") return number * 1_000_000;
  if (suffix === "b") return number * 1_000_000_000;

  return number;
}

export function metadataBoolean(...values) {
  for (const value of values) {
    if (value === true) return true;
    if (value === false) continue;

    const clean = metadataString(value).toLowerCase();
    if (["true", "1", "yes", "verified"].includes(clean)) return true;
  }

  return false;
}

export function normalizeMetadataKey(value) {
  return metadataString(value).replace(/[^a-z0-9]/gi, "").toLowerCase();
}

export function metadataCountFromValue(value) {
  const number = metadataNumber(value);
  if (number === null || number <= 0) return "";

  return Math.round(number);
}

export function findFirstMetadataCount(value, keys, seen = new Set(), depth = 0) {
  if (value === null || value === undefined || depth > 6) return "";

  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findFirstMetadataCount(item, keys, seen, depth + 1);
      if (found !== "") return found;
    }

    return "";
  }

  if (typeof value !== "object") return "";
  if (seen.has(value)) return "";
  seen.add(value);

  const normalizedKeys = new Set(keys.map(normalizeMetadataKey));

  for (const [key, item] of Object.entries(value)) {
    if (!normalizedKeys.has(normalizeMetadataKey(key))) continue;

    const count = metadataCountFromValue(item);
    if (count !== "") return count;
  }

  for (const item of Object.values(value)) {
    const found = findFirstMetadataCount(item, keys, seen, depth + 1);
    if (found !== "") return found;
  }

  return "";
}

export function twitterPostCountMetadata(info = {}) {
  return {
    replyCount: findFirstMetadataCount(info, [
      "comment_count",
      "comments_count",
      "reply_count",
      "replies_count",
    ]),
    retweetCount: findFirstMetadataCount(info, [
      "repost_count",
      "reposts_count",
      "retweet_count",
      "retweets_count",
    ]),
    likeCount: findFirstMetadataCount(info, [
      "like_count",
      "likes_count",
      "favorite_count",
      "favorites_count",
    ]),
    viewCount: findFirstMetadataCount(info, [
      "view_count",
      "views_count",
      "impression_count",
      "impressions_count",
    ]),
  };
}

export function formatTwitterActionCount(value) {
  const number = metadataCountFromValue(value);
  if (number === "") return "";

  return new Intl.NumberFormat("tr-TR", {
    maximumFractionDigits: 0,
  }).format(number);
}

export function formatTwitterCompactCount(value) {
  const number = metadataCountFromValue(value);
  if (number === "") return "";

  if (number >= 1_000_000_000) {
    return `${(number / 1_000_000_000).toFixed(number >= 10_000_000_000 ? 0 : 1).replace(/\.0$/, "")}B`;
  }

  if (number >= 1_000_000) {
    return `${(number / 1_000_000).toFixed(number >= 10_000_000 ? 0 : 1).replace(/\.0$/, "")}M`;
  }

  if (number >= 1_000) {
    return `${(number / 1_000).toFixed(number >= 10_000 ? 0 : 1).replace(/\.0$/, "")}K`;
  }

  return String(number);
}

export function normalizeTwitterPostMetadata(info = {}, fallbackUrl = "") {
  const timestamp = Number(info?.timestamp || 0);
  const safeTimestamp = Number.isFinite(timestamp) && timestamp > 0 ? timestamp : null;
  const timestampDate = safeTimestamp ? formatMetadataDate(new Date(safeTimestamp * 1000)) : "";
  const webpageUrl = firstMetadataString(info?.webpage_url, info?.original_url, fallbackUrl);
  const uploaderUrlHandle = twitterHandleFromUrl(info?.uploader_url);
  const directHandle = normalizeTwitterHandle(firstMetadataString(info?.uploader_id, info?.channel_id));
  const thumbnail = getThumbnailCandidates(info)[0] || "";
  const avatarUrl = twitterProfileImageCandidate(info);
  const rawText = firstMetadataString(info?.description, info?.fulltitle, info?.title);
  const counts = twitterPostCountMetadata(info);
  const metadataAuthorName = firstMetadataString(info?.uploader, info?.channel, info?.creator);
  const titleDisplayName = twitterDisplayNameFromTitle(info);

  return {
    text: cleanTwitterPostText(rawText),
    exportText: rawText,
    authorName:
      twitterDisplayNameWithRichFallback(metadataAuthorName, titleDisplayName) || "X/Twitter",
    authorHandle: directHandle || uploaderUrlHandle,
    timestamp: safeTimestamp,
    displayDate: timestampDate || twitterDisplayDateFromUploadDate(info?.upload_date),
    webpageUrl,
    thumbnail,
    avatarUrl,
    avatarDataUrl: safeTwitterAvatarDataUrl(avatarUrl),
    isVerified: metadataBoolean(
      info?.is_verified,
      info?.verified,
      info?.uploader_verified,
      info?.author_verified,
      info?.creator_verified,
      info?.channel_is_verified
    ),
    replyCount: counts.replyCount,
    retweetCount: counts.retweetCount,
    likeCount: counts.likeCount,
    viewCount: counts.viewCount,
    sourceLabel: "x.com",
  };
}
