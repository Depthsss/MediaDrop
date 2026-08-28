function text(value) {
  return String(value ?? "").trim();
}

export function mediaCardDescription(item = {}, analysis = {}) {
  if (
    text(analysis?.platform).toLowerCase() === "instagram" &&
    (Boolean(item?.isStory) || text(analysis?.contentKind).toLowerCase() === "story")
  ) {
    return "";
  }
  return text(item?.text || analysis?.title || item?.title);
}

export function instagramAnalysisNeedsAvatarAuth(analysis = {}) {
  return (
    text(analysis?.platform).toLowerCase() === "instagram" &&
    Array.isArray(analysis?.items) &&
    analysis.items.length > 0 &&
    !text(analysis?.author?.avatarDataUrl)
  );
}

export function twitterMediaPostDownloadKind(platform = "", item = null) {
  if (text(platform).toLowerCase() !== "twitter" || !item) return "";
  return mediaItemType(item);
}

function sourceIndex(value, fallbackIndex = 0) {
  const numeric = Number(value);
  return Number.isInteger(numeric) && numeric >= 0 ? numeric : Math.max(0, fallbackIndex);
}

const MEDIA_ANALYSIS_WARNING_MESSAGES = Object.freeze({
  requestedStoryUnavailable:
    "Bağlantıdaki hikaye artık aktif değil; hesabın ilk erişilebilir hikayesi gösteriliyor.",
  instagramAuthenticatedPublicFallback:
    "Kayıtlı Instagram oturumuyla analiz tamamlanamadı; gönderi herkese açık verilerle gösteriliyor.",
});

function finiteTimestamp(value) {
  const numeric = Number(value);
  return Number.isFinite(numeric) && numeric > 0 ? Math.trunc(numeric) : 0;
}

function normalizedWarnings(value) {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.map(text).filter(Boolean))];
}

export function normalizeMediaAnalysis(analysis = {}) {
  const source = analysis && typeof analysis === "object" && !Array.isArray(analysis)
    ? analysis
    : {};
  return {
    ...source,
    analysisId: text(source.analysisId),
    expiresAtMs: finiteTimestamp(source.expiresAtMs),
    warnings: normalizedWarnings(source.warnings),
  };
}

export function isMediaAnalysisExpired(analysis = {}, nowMs = Date.now()) {
  const normalized = normalizeMediaAnalysis(analysis);
  const now = finiteTimestamp(nowMs);
  return Boolean(
    normalized.analysisId &&
    normalized.expiresAtMs &&
    now >= normalized.expiresAtMs
  );
}

export function mediaAnalysisWarningMessages(analysis = {}) {
  return normalizeMediaAnalysis(analysis).warnings
    .map((warning) => MEDIA_ANALYSIS_WARNING_MESSAGES[warning] || "")
    .filter(Boolean);
}

export function mediaAnalysisAuthorIdentity(analysis = {}, item = {}) {
  const normalized = normalizeMediaAnalysis(analysis);
  const registryBacked = Boolean(normalized.analysisId);
  const author = normalized.author && typeof normalized.author === "object" && !Array.isArray(normalized.author)
    ? normalized.author
    : {};
  const legacy = registryBacked || !item || typeof item !== "object" || Array.isArray(item)
    ? {}
    : item;

  return {
    registryBacked,
    id: text(author.id || legacy.authorId),
    name: text(author.name || legacy.authorName),
    handle: text(author.handle || legacy.authorHandle),
    avatarDataUrl: text(author.avatarDataUrl || legacy.avatarDataUrl || legacy.avatarUrl),
    avatarUrl: text(author.avatarUrl || legacy.avatarUrl),
  };
}

export function supportsLegacyVideoFallback(platform = "") {
  return text(platform).toLowerCase() !== "instagram";
}

export function shouldTryMediaInventory(platform = "", url = "") {
  return (
    text(platform).toLowerCase() !== "tiktok" ||
    !/\/video\/\d+(?:[/?#]|$)/i.test(text(url))
  );
}

export function normalizeRasterImageSource(value) {
  const clean = text(value);
  if (!clean) return "";

  if (/^data:image\/(?:png|jpe?g|webp|gif|avif);base64,/i.test(clean)) {
    return clean;
  }

  if (/^(?:asset|blob):/i.test(clean)) {
    return clean;
  }

  try {
    const parsed = new URL(clean);
    const host = parsed.hostname.toLowerCase();
    const localHost =
      host === "asset.localhost" ||
      host === "localhost" ||
      host === "127.0.0.1" ||
      host === "::1";

    if (["http:", "https:"].includes(parsed.protocol) && localHost) {
      return clean;
    }
  } catch {}

  return "";
}

export function mediaItemType(item = {}) {
  return text(item?.type || item?.itemType).toLowerCase() === "video" ? "video" : "photo";
}

export function normalizeMediaItem(item = {}, fallbackIndex = 0) {
  const source = item && typeof item === "object" && !Array.isArray(item) ? item : {};
  return {
    ...source,
    id: text(source.id),
    type: mediaItemType(source),
    sourceIndex: sourceIndex(source.sourceIndex, fallbackIndex),
    previewUrl: text(source.previewUrl),
    previewRef: text(source.previewRef),
    isStory: Boolean(source.isStory),
  };
}

export function mediaItemKindLabel(item = {}) {
  const isVideo = mediaItemType(item) === "video";
  if (item?.isStory) return isVideo ? "Video hikayesi" : "Fotoğraf hikayesi";
  return isVideo ? "Video" : "Fotoğraf";
}

export function mediaPreviewPolicy(analysis = {}, item = {}) {
  const analysisId = text(analysis?.analysisId);
  const itemId = text(item?.id);
  const registryBacked = Boolean(analysisId && itemId);

  return {
    analysisId,
    itemId,
    registryBacked,
    allowLegacyFallback: false,
    refreshAccessOnDisplay: registryBacked,
    legacySource: "",
    cacheKey: registryBacked ? `${analysisId}:${itemId}` : "",
  };
}

export function clipPreviewAttemptBudgetMs(deadlineMs, nowMs, attemptsLeft = 1) {
  const remaining = Math.max(0, Math.floor(Number(deadlineMs) - Number(nowMs)));
  const attempts = Math.floor(Number(attemptsLeft));
  if (!Number.isFinite(remaining) || attempts <= 0) return 0;
  return attempts > 1 ? Math.min(remaining, 3_000) : remaining;
}

export function isClipPreviewBuildActive(expectedToken, currentToken, deadlineMs, nowMs) {
  return expectedToken === currentToken && clipPreviewAttemptBudgetMs(deadlineMs, nowMs) > 0;
}

export function nativeClipPlayerState(paused, readyState) {
  if (paused) return 2;
  return Number(readyState) < 2 ? 3 : 1;
}

export function clipPreviewStreamSources(preview) {
  if (typeof preview === "string") {
    const url = text(preview);
    return { videoUrls: url ? [url] : [], audioUrl: "" };
  }
  if (!preview || typeof preview !== "object" || Array.isArray(preview)) {
    return { videoUrls: [], audioUrl: "" };
  }

  const rawVideoUrls = Array.isArray(preview.urls) && preview.urls.length
    ? preview.urls
    : [preview.url];
  const videoUrls = [...new Set(rawVideoUrls.map(text).filter(Boolean))];

  return {
    videoUrls,
    audioUrl: text(preview.audioUrl || preview.audio_url),
  };
}

export function nativeClipAudioSyncTarget(
  videoTime,
  audioTime,
  {
    videoReadyState = 4,
    audioReadyState = 4,
    seeking = false,
    maxDriftSeconds = 0.35,
  } = {}
) {
  const video = Number(videoTime);
  const audio = Number(audioTime);
  const maxDrift = Math.max(0, Number(maxDriftSeconds) || 0);
  if (
    !Number.isFinite(video)
    || video < 0
    || seeking
    || Number(videoReadyState) < 3
    || Number(audioReadyState) < 3
  ) return null;
  return !Number.isFinite(audio) || Math.abs(video - audio) > maxDrift ? video : null;
}

export function normalizeMediaPreviewResponse(result, convertFileSrc) {
  if (typeof result === "string") return text(result);
  if (!result || typeof result !== "object" || Array.isArray(result)) return "";
  const filePath = text(result.filePath);
  if (filePath) {
    if (typeof convertFileSrc !== "function") return "";
    // Tauri accepts forward slashes on Windows and this keeps the generated
    // asset URL stable across platform-specific path separators.
    return text(convertFileSrc(filePath.replaceAll("\\", "/")));
  }
  return text(
    result.previewUrl ||
    result.streamUrl ||
    result.localUrl ||
    result.dataUrl ||
    result.url
  );
}

export function reusableMediaPreviewValue(cached = {}, forcePrepare = false) {
  if (!forcePrepare && cached?.source) return cached.source;
  if (!forcePrepare && cached?.dataUrl) return cached.dataUrl;
  return cached?.promise || "";
}

export function mediaItemHasPreview(item, analysis = {}) {
  const normalized = normalizeMediaItem(item);
  return Boolean(text(analysis?.analysisId) && normalized.id);
}

function sameMediaItem(left, right) {
  if (!left || !right) return false;
  const leftId = text(left.id);
  const rightId = text(right.id);
  if (leftId && rightId) return leftId === rightId;
  return sourceIndex(left.sourceIndex, -1) === sourceIndex(right.sourceIndex, -2);
}

export function mediaInitialIndex(analysis = {}, rawItems = [], items = []) {
  if (!items.length) return 0;

  const initialIndex = Number(analysis?.initialIndex);
  if (Number.isInteger(initialIndex) && initialIndex >= 0) {
    const initialItem = rawItems[initialIndex];
    const filteredIndex = items.findIndex((item) => sameMediaItem(item, initialItem));
    if (filteredIndex >= 0) return filteredIndex;
    return Math.min(initialIndex, items.length - 1);
  }

  const requestedItemId = text(analysis?.requestedItemId);
  if (requestedItemId) {
    const requestedIndex = items.findIndex((item) => text(item?.id) === requestedItemId);
    if (requestedIndex >= 0) return requestedIndex;
  }

  return 0;
}

export function normalizeMediaAnalysisItems(analysis = {}) {
  const rawItems = Array.isArray(analysis?.items) ? analysis.items : [];
  const items = rawItems
    .map((item, index) => normalizeMediaItem(item, index))
    .filter((item) => mediaItemHasPreview(item, analysis));

  return {
    items,
    initialIndex: mediaInitialIndex(analysis, rawItems, items),
  };
}

export function selectMediaPreviewPrefetchItems(
  items = [],
  startIndex = 0,
  { canPrefetch = () => true, isPrepared = () => false } = {}
) {
  return items
    .map((item, index) => ({ item, index }))
    .filter(({ item, index }) =>
      Math.abs(index - startIndex) <= 1 &&
      canPrefetch(item) &&
      !isPrepared(item)
    )
    .sort((left, right) =>
      Math.abs(left.index - startIndex) - Math.abs(right.index - startIndex) ||
      left.index - right.index
    );
}
