import {
  cleanTwitterPostText,
  formatTwitterDisplayDate,
  metadataString,
  normalizeTwitterHandle,
  normalizeTwitterQuoteContext,
  safeTwitterAvatarDataUrl,
  twitterHandleFromUrl,
} from "./metadata.js";
import {
  hydrateTwitterAvatarDataUrl,
  pngDataUrlToBase64,
  renderTwitterPhotoPostCardPng,
  renderTwitterPostCardPng,
  renderTwitterTextPostCardPng,
  twitterPostErrorCode,
} from "./renderer.js";
import { convertFileSrc, invoke, listen } from "../../app/tauri.js";
import { loadRasterImageSource } from "../preview/raster-loader.js";

let rendererStarted = false;
const MAX_PREVIEW_DATA_URL_CHARS = 350_000;

export function previewCanvasSize(width, height, mediaType = "photo") {
  const maxWidth = 480;
  const maxHeight = mediaType === "video" ? 270 : 480;
  if (!(Number(width) > 0) || !(Number(height) > 0)) {
    return { width: maxWidth, height: maxHeight };
  }
  const sourceWidth = Number(width);
  const sourceHeight = Number(height);
  const scale = Math.min(maxWidth / sourceWidth, maxHeight / sourceHeight, 1);
  return {
    width: Math.max(1, Math.round(sourceWidth * scale)),
    height: Math.max(1, Math.round(sourceHeight * scale)),
  };
}

function embeddedMetadata(post = {}) {
  return {
    ...post,
    text: cleanTwitterPostText(post.text),
    exportText: metadataString(post.exportText || post.text),
    authorName: metadataString(post.authorName) || "X/Twitter",
    authorHandle: normalizeTwitterHandle(post.authorHandle),
    displayDate: formatTwitterDisplayDate(post.displayDate),
    avatarDataUrl: safeTwitterAvatarDataUrl(post.avatarDataUrl),
    sourceLabel: "x.com",
  };
}

export function companionTwitterRenderModel({ analysis = {}, sourceUrl = "", mediaId = "" } = {}) {
  const items = Array.isArray(analysis.items) ? analysis.items : [];
  const itemIndex = items.findIndex((entry) => metadataString(entry?.id) === metadataString(mediaId));
  const item = itemIndex >= 0 ? items[itemIndex] : null;
  const quote = normalizeTwitterQuoteContext(analysis.twitterQuote);
  const post = analysis.twitterPost && typeof analysis.twitterPost === "object"
    ? analysis.twitterPost
    : null;
  let secondaryMedia = null;
  let metadata;

  if (quote) {
    const quotedIndexes = new Set(quote.quotedMediaIndexes);
    const activeMediaRole = quotedIndexes.has(itemIndex) ? "quoted" : "outer";
    const secondaryRole = activeMediaRole === "quoted" ? "outer" : "quoted";
    const secondaryItem = items.find((_, index) =>
      secondaryRole === "quoted" ? quotedIndexes.has(index) : !quotedIndexes.has(index)
    );
    if (secondaryItem) {
      const width = Number(secondaryItem.width);
      const height = Number(secondaryItem.height);
      secondaryMedia = {
        itemId: metadataString(secondaryItem.id),
        role: secondaryRole,
        aspectRatio: width > 0 && height > 0 ? width / height : 16 / 9,
      };
    }
    metadata = {
      ...embeddedMetadata(quote.outer),
      webpageUrl: sourceUrl,
      quotedPost: embeddedMetadata(quote.quoted),
      quotedMediaIndexes: quote.quotedMediaIndexes,
      activeMediaRole,
      hasOuterMedia: items.some((_, index) => !quotedIndexes.has(index)),
      hasQuotedMedia: items.some((_, index) => quotedIndexes.has(index)),
    };
  } else {
    const rawText = metadataString(item?.text || post?.text || (
      metadataString(analysis.contentKind).toLowerCase() === "text" ? analysis.title : ""
    ));
    const authorName = metadataString(
      analysis.author?.name || item?.authorName || post?.authorName || analysis.uploader,
    ) || "X/Twitter";
    metadata = {
      text: rawText ? cleanTwitterPostText(rawText) : "",
      exportText: rawText,
      authorName,
      authorHandle: normalizeTwitterHandle(
        analysis.author?.handle || item?.authorHandle || post?.authorHandle,
      ) || twitterHandleFromUrl(sourceUrl),
      displayDate: formatTwitterDisplayDate(item?.displayDate || post?.displayDate),
      avatarUrl: metadataString(item?.avatarUrl || post?.avatarUrl),
      avatarDataUrl: safeTwitterAvatarDataUrl(
        analysis.author?.avatarDataUrl || item?.avatarDataUrl,
      ),
      isVerified: Boolean(item?.isVerified ?? post?.isVerified),
      replyCount: item?.replyCount ?? post?.replyCount,
      retweetCount: item?.retweetCount ?? post?.retweetCount,
      likeCount: item?.likeCount ?? post?.likeCount,
      viewCount: item?.viewCount ?? post?.viewCount,
      webpageUrl: sourceUrl,
      sourceLabel: "x.com",
    };
  }

  const mode = metadataString(item?.type || analysis.contentKind).toLowerCase();
  const normalizedMode = mode === "photo" ? "photo" : mode === "video" ? "video" : "text";
  metadata.duration = Number(item?.durationMs || 0) / 1000;
  metadata.quality = item?.height ? `${item.height}p` : normalizedMode === "text" ? "Metin" : "Otomatik";
  metadata.textOnly = normalizedMode === "text";
  const title = metadataString(metadata.text || analysis.title || metadata.authorName)
    || "X/Twitter gönderisi";

  return { mode: normalizedMode, title, metadata, item, itemIndex, secondaryMedia };
}

async function hydrateMetadata(metadata) {
  if (!metadata.quotedPost) return hydrateTwitterAvatarDataUrl(metadata);
  const [outer, quotedPost] = await Promise.all([
    hydrateTwitterAvatarDataUrl(metadata),
    hydrateTwitterAvatarDataUrl(metadata.quotedPost),
  ]);
  return { ...outer, quotedPost };
}

async function renderTask(request) {
  const model = companionTwitterRenderModel(request.payload);
  const secondaryPreviewPath = metadataString(request.payload?.secondaryPreviewPath);
  const renderMetadata = model.secondaryMedia && secondaryPreviewPath
    ? {
        ...model.metadata,
        secondaryMedia: {
          role: model.secondaryMedia.role,
          source: convertFileSrc(secondaryPreviewPath),
          aspectRatio: model.secondaryMedia.aspectRatio,
        },
      }
    : model.metadata;
  const metadata = await hydrateMetadata(renderMetadata);
  if (model.mode === "photo") {
    const previewPath = metadataString(request.payload?.previewPath);
    if (!previewPath) {
      throw Object.assign(new Error("preview missing"), { debugCode: "preview_failed" });
    }
    const rendered = await renderTwitterPhotoPostCardPng(
      metadata,
      convertFileSrc(previewPath),
      model.item,
    );
    return {
      ok: true,
      mode: "photo",
      title: rendered.title || model.title,
      cardPngBase64: pngDataUrlToBase64(rendered.dataUrl),
    };
  }
  if (model.mode === "text") {
    const rendered = await renderTwitterTextPostCardPng(metadata);
    return {
      ok: true,
      mode: "text",
      title: rendered.title || model.title,
      cardPngBase64: pngDataUrlToBase64(rendered.dataUrl),
    };
  }
  const rendered = await renderTwitterPostCardPng(metadata);
  return {
    ok: true,
    mode: "video",
    title: model.title,
    cardPngBase64: pngDataUrlToBase64(rendered.dataUrl),
    cardOverlayPngBase64: pngDataUrlToBase64(rendered.overlayDataUrl),
    cardLayout: rendered.layout,
  };
}

function previewDataUrl(source, width, height) {
  const size = previewCanvasSize(width, height, source instanceof HTMLVideoElement ? "video" : "photo");
  const canvas = document.createElement("canvas");
  canvas.width = size.width;
  canvas.height = size.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("preview canvas unavailable");
  context.fillStyle = "#18181b";
  context.fillRect(0, 0, size.width, size.height);
  context.drawImage(source, 0, 0, size.width, size.height);
  for (const quality of [0.82, 0.68, 0.52, 0.4]) {
    const dataUrl = canvas.toDataURL("image/jpeg", quality);
    if (dataUrl.length <= MAX_PREVIEW_DATA_URL_CHARS) return dataUrl;
  }
  throw new Error("preview too large");
}

function waitForVideo(video) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("preview timeout")), 10_000);
    video.onloadeddata = () => {
      clearTimeout(timeout);
      resolve();
    };
    video.onerror = () => {
      clearTimeout(timeout);
      reject(new Error("preview load failed"));
    };
  });
}

export async function createLocalPreviewVideoSource(
  value,
  {
    fetchFn = globalThis.fetch?.bind(globalThis),
    createObjectURL = globalThis.URL?.createObjectURL?.bind(globalThis.URL),
    revokeObjectURL = globalThis.URL?.revokeObjectURL?.bind(globalThis.URL),
  } = {},
) {
  const source = metadataString(value);
  if (!source || typeof fetchFn !== "function" || typeof createObjectURL !== "function") {
    throw new Error("preview source unavailable");
  }
  const response = await fetchFn(source, { cache: "no-store", credentials: "omit" });
  if (!response?.ok) throw new Error("preview source fetch failed");
  const blob = await response.blob();
  if (!blob?.size) throw new Error("preview source empty");
  const objectUrl = createObjectURL(blob);
  let released = false;
  return {
    source: objectUrl,
    release() {
      if (released) return;
      released = true;
      revokeObjectURL?.(objectUrl);
    },
  };
}

async function renderPreviewTask(payload = {}) {
  const previewPath = metadataString(payload.previewPath);
  if (!previewPath) throw new Error("preview path missing");
  const source = convertFileSrc(previewPath);
  if (payload.mediaType === "video") {
    const video = document.createElement("video");
    const localSource = await createLocalPreviewVideoSource(source);
    video.muted = true;
    video.playsInline = true;
    video.preload = "auto";
    try {
      video.src = localSource.source;
      video.load();
      await waitForVideo(video);
      return {
        ok: true,
        dataUrl: previewDataUrl(
          video,
          Number(video.videoWidth || payload.width),
          Number(video.videoHeight || payload.height),
        ),
        durationSeconds: Number.isFinite(video.duration) && video.duration > 0
          ? video.duration
          : null,
      };
    } finally {
      video.pause();
      video.removeAttribute("src");
      video.load();
      localSource.release();
    }
  }
  const image = await loadRasterImageSource(source);
  if (!image) throw new Error("preview image unavailable");
  return {
    ok: true,
    dataUrl: previewDataUrl(
      image,
      Number(image.naturalWidth || image.width || payload.width),
      Number(image.naturalHeight || image.height || payload.height),
    ),
    durationSeconds: null,
  };
}

export async function startCompanionRenderer() {
  if (rendererStarted) return;
  rendererStarted = true;
  await listen("companion-render-request", (event) => {
    const request = event?.payload || {};
    if (!["twitter_post_export", "media_preview"].includes(request.kind)
      || !metadataString(request.taskId)) return;
    const task = request.kind === "media_preview"
      ? renderPreviewTask(request.payload)
      : renderTask(request);
    void task.then(
      (result) => invoke("complete_companion_render", { taskId: request.taskId, result }),
      (error) => invoke("complete_companion_render", {
        taskId: request.taskId,
        result: {
          ok: false,
          errorCode: request.kind === "media_preview"
            ? "preview_failed"
            : twitterPostErrorCode(error, "renderer_result_invalid"),
          stage: metadataString(error?.stage || error?.debugCode || "renderer_result_invalid"),
        },
      }),
    ).catch(() => {});
  });
  await invoke("companion_renderer_ready");
}
