import { invoke } from "../../app/tauri.js";

import { loadRasterImageSource } from "../preview/raster-loader.js";
import {
  cleanTwitterPostText,
  formatMediaDuration,
  formatTwitterActionCount,
  isRemoteImageCandidate,
  metadataString,
  normalizeTwitterHandle,
  normalizeTwitterImageUrl,
  safeTwitterAvatarDataUrl,
  twitterAvatarInitial,
  twitterHandleFromUrl,
} from "./metadata.js";

const TWITTER_POST_TEMPLATE_PLACEHOLDERS = [
  "display_name",
  "handle",
  "date",
  "tweet_text",
  "avatar_initial",
  "platform_label",
  "quality_label",
  "duration_label",
  "reply_count",
  "retweet_count",
  "like_count",
  "source_label",
];

function urlHost(value) {
  let clean = String(value || "").trim();
  if (!clean) return "";
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(clean)) clean = `https://${clean}`;
  try {
    return new URL(clean).hostname.toLowerCase().replace(/^www\./, "");
  } catch {
    return "";
  }
}

async function loadTwitterPostMp4Template() {
  try {
    const template = await invoke("get_twitter_post_mp4_template");
    const templateType = typeof template;
    const templateLength = templateType === "string" ? template.length : 0;

    console.debug("Twitter post MP4 template loaded", {
      type: templateType,
      length: templateLength,
      hasVideoSlot:
        templateType === "string" && template.includes("data-video-slot"),
    });

    if (templateType !== "string") {
      throw twitterPostTemplateError(
        "template_command_failed",
        `get_twitter_post_mp4_template string dönmedi: ${templateType}`
      );
    }

    const clean = metadataString(template);

    if (!clean) {
      throw twitterPostTemplateError("template_empty", "X/Twitter MP4 template dosyası boş.");
    }

    if (!template.includes("data-video-slot")) {
      throw twitterPostTemplateError(
        "template_missing_data_video_slot",
        "X/Twitter MP4 template içinde data-video-slot bulunamadı."
      );
    }

    return template;
  } catch (error) {
    if (isTwitterPostTemplateError(error)) {
      throw ensureTwitterPostTemplateError(
        error,
        "template_load_failed",
        "X/Twitter MP4 template yüklenemedi"
      );
    }
    throw twitterPostTemplateError(
      "template_load_failed",
      `X/Twitter MP4 template yüklenemedi: ${error?.message || error || "bilinmeyen hata"}`
    );
  }
}

function setStageErrorMetadata(error, key, value) {
  if (value === undefined) return;

  try {
    error[key] = value;
  } catch {}
}

function createStageError(stage, message, options = {}) {
  const debugCode = metadataString(stage) || "unknown_error";
  const cleanMessage = metadataString(message) || "Bilinmeyen hata.";
  const hasCause = Object.prototype.hasOwnProperty.call(options, "cause");
  let wrapped = null;

  if (hasCause) {
    try {
      wrapped = new Error(`${debugCode}: ${cleanMessage}`, { cause: options.cause });
    } catch {
      wrapped = new Error(`${debugCode}: ${cleanMessage}`);
      setStageErrorMetadata(wrapped, "cause", options.cause);
    }
  } else {
    wrapped = new Error(`${debugCode}: ${cleanMessage}`);
  }

  setStageErrorMetadata(wrapped, "stage", debugCode);
  setStageErrorMetadata(wrapped, "debugCode", debugCode);
  setStageErrorMetadata(wrapped, "originalMessage", options.originalMessage);
  setStageErrorMetadata(wrapped, "details", options.details);
  setStageErrorMetadata(wrapped, "layoutSnapshot", options.layoutSnapshot);
  setStageErrorMetadata(wrapped, "renderDebug", options.renderDebug);

  return wrapped;
}

function twitterPostTemplateError(stage, message, options = {}) {
  return createStageError(stage, message, options);
}

function isTwitterPostTemplateError(error) {
  return (
    Boolean(error?.debugCode || error?.stage || error?.errorCode) ||
    /^[a-z_]+:/.test(String(error?.message || error || ""))
  );
}

function twitterPostErrorCode(error, fallback = "unknown_error") {
  if (error?.debugCode) return error.debugCode;
  if (error?.stage) return error.stage;
  if (error?.errorCode) return error.errorCode;

  const match = String(error?.message || error || "").match(/^([a-z_]+):/);
  return match?.[1] || fallback;
}

function stripStagePrefix(value, stage) {
  return String(value || "").replace(new RegExp(`^${stage}:\\s*`), "");
}

function ensureTwitterPostTemplateError(error, fallbackStage, fallbackMessage, options = {}) {
  if (isTwitterPostTemplateError(error)) {
    const debugCode = twitterPostErrorCode(error, fallbackStage);
    const rawMessage = String(error?.message || error || "");
    const detail = stripStagePrefix(rawMessage, debugCode) || fallbackMessage;

    return createStageError(debugCode, detail, {
      ...options,
      cause: error,
      originalMessage: rawMessage,
      details: options.details ?? error?.details,
      layoutSnapshot: options.layoutSnapshot ?? error?.layoutSnapshot,
      renderDebug: options.renderDebug ?? error?.renderDebug,
    });
  }

  const originalMessage = String(error?.message || error || "bilinmeyen hata");
  return createStageError(fallbackStage, `${fallbackMessage}: ${originalMessage}`, {
    ...options,
    cause: error,
    originalMessage,
  });
}

function escapeHtml(value) {
  return metadataString(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function fillTwitterPostMp4Template(template, values) {
  try {
    const source = metadataString(template);

    if (!source) {
      throw twitterPostTemplateError("template_empty", "X/Twitter MP4 template dosyası boş.");
    }

    const rendered = source.replace(/{{\s*([a-z_]+)\s*}}/g, (_, key) =>
      escapeHtml(values?.[key] ?? "")
    );

    if (!metadataString(rendered)) {
      throw twitterPostTemplateError(
        "placeholder_render_failed",
        "X/Twitter MP4 template placeholder çıktısı boş."
      );
    }

    const missing = TWITTER_POST_TEMPLATE_PLACEHOLDERS.filter((key) =>
      rendered.includes(`{{${key}}}`)
    );
    if (missing.length > 0) {
      throw twitterPostTemplateError(
        "placeholder_render_failed",
        `X/Twitter MP4 template placeholder değişimi tamamlanamadı: ${missing.join(", ")}`
      );
    }

    if (!rendered.includes("data-video-slot")) {
      throw twitterPostTemplateError(
        "template_missing_data_video_slot",
        "Placeholder sonrası HTML içinde data-video-slot bulunamadı."
      );
    }

    return rendered;
  } catch (error) {
    if (isTwitterPostTemplateError(error)) {
      throw ensureTwitterPostTemplateError(
        error,
        "placeholder_render_failed",
        "X/Twitter MP4 template placeholder değişimi başarısız"
      );
    }
    throw twitterPostTemplateError(
      "placeholder_render_failed",
      `X/Twitter MP4 template placeholder değişimi başarısız: ${error?.message || error}`
    );
  }
}

function evenFloor(value) {
  const number = Math.max(0, Math.floor(Number(value) || 0));
  return number % 2 === 0 ? number : number - 1;
}

function evenCeil(value) {
  const number = Math.max(0, Math.ceil(Number(value) || 0));
  return number % 2 === 0 ? number : number + 1;
}

function isValidTwitterPostCardLayout(layout) {
  if (!layout || typeof layout !== "object") return false;

  const {
    outputWidth,
    outputHeight,
    videoX,
    videoY,
    videoWidth,
    videoHeight,
  } = layout;

  return (
    [outputWidth, outputHeight, videoX, videoY, videoWidth, videoHeight].every(Number.isFinite) &&
    outputWidth > 0 &&
    outputHeight > 0 &&
    videoX >= 0 &&
    videoY >= 0 &&
    videoWidth > 0 &&
    videoHeight > 0 &&
    videoX + videoWidth <= outputWidth &&
    videoY + videoHeight <= outputHeight
  );
}

function normalizedPostCardMediaAspectRatio(value) {
  const ratio = Number(value);
  if (!Number.isFinite(ratio) || ratio <= 0) return 16 / 9;
  return Math.min(2.35, Math.max(0.72, ratio));
}

function twitterPostTemplateValues(metadata = {}) {
  const fallbackText = cleanTwitterPostText("");
  const cleanedText = cleanTwitterPostText(metadata.text);
  const tweetText =
    metadataString(metadata.exportText) || (cleanedText === fallbackText ? "" : cleanedText);
  const displayName = metadataString(metadata.authorName) || "X/Twitter";
  const duration = Number(metadata.duration || 0);

  const quotedPost = metadata?.quotedPost && typeof metadata.quotedPost === "object"
    ? twitterPostTemplateValues({ ...metadata.quotedPost, quotedPost: null })
    : null;

  return {
    display_name: displayName,
    handle: metadataString(metadata.authorHandle),
    date: metadataString(metadata.displayDate),
    tweet_text: tweetText,
    avatar_initial: twitterAvatarInitial(displayName || metadata.authorHandle),
    avatar_data_url: safeTwitterAvatarDataUrl(metadata.avatarDataUrl),
    platform_label: "X / Twitter",
    quality_label: metadataString(metadata.quality) || "Otomatik",
    duration_label: Number.isFinite(duration) && duration > 0 ? formatMediaDuration(duration) : "",
    reply_count: metadataString(metadata.replyCount),
    retweet_count: metadataString(metadata.retweetCount),
    like_count: metadataString(metadata.likeCount),
    source_label: metadataString(metadata.sourceLabel) || "x.com",
    quoted_post: quotedPost,
  };
}

function waitAnimationFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

function parseTwitterPostTemplateHtml(html) {
  return new DOMParser().parseFromString(html, "text/html");
}

function twitterPostShadowHtml(doc) {
  const styles = Array.from(doc.head?.querySelectorAll("style") || [])
    .map((style) => style.outerHTML)
    .join("\n");
  const body = doc.body?.innerHTML || "";

  if (!body.trim()) {
    throw twitterPostTemplateError(
      "placeholder_render_failed",
      "X/Twitter MP4 template gövdesi boş."
    );
  }

  return `${styles}\n${body}`;
}

function findTwitterPostTemplateExternalResourceRisks(html) {
  const source = metadataString(html);
  const risks = new Set();

  if (!source) return [];

  const doc = parseTwitterPostTemplateHtml(source);
  const unsafeElements = [
    "img",
    "image",
    "picture",
    "source",
    "video",
    "audio",
    "iframe",
    "object",
    "embed",
    "link",
    "script",
    "svg",
  ];

  for (const selector of unsafeElements) {
    if (doc.querySelector(selector)) risks.add(`<${selector}>`);
  }

  const resourceAttributes = ["src", "href", "xlink:href", "poster"];
  for (const element of Array.from(doc.querySelectorAll("*"))) {
    for (const attribute of resourceAttributes) {
      const value = metadataString(element.getAttribute(attribute));
      if (!value) continue;
      if (/^(?:data:|#|about:blank$)/i.test(value)) continue;
      risks.add(`${element.tagName.toLowerCase()}[${attribute}]`);
    }

    const styleAttr = metadataString(element.getAttribute("style"));
    if (styleAttr && cssTextContainsTwitterPostResourceRisk(styleAttr)) {
      risks.add(`${element.tagName.toLowerCase()}[style]`);
    }
  }

  for (const style of Array.from(doc.querySelectorAll("style"))) {
    if (cssTextContainsTwitterPostResourceRisk(style.textContent || "")) {
      risks.add("<style>");
    }
  }

  return Array.from(risks);
}

function cssTextContainsTwitterPostResourceRisk(cssText) {
  const clean = metadataString(cssText);
  if (!clean) return false;

  return (
    /@(?:font-face|import)\b/i.test(clean) ||
    /\b(?:background-image|mask-image|border-image)\s*:/i.test(clean) ||
    /url\s*\(/i.test(clean) ||
    /\b(?:https?:|file:|blob:)\//i.test(clean)
  );
}

function assertTwitterPostTemplateExportSafe(html, label) {
  const risks = findTwitterPostTemplateExternalResourceRisks(html);

  if (risks.length > 0) {
    throw twitterPostTemplateError(
      "template_contains_external_resource",
      `${label} içinde canvas export için güvenli olmayan kaynak var: ${risks.join(", ")}`,
      { details: risks }
    );
  }
}

function roundedRectPath(ctx, x, y, width, height, radius) {
  const safeRadius = Math.max(0, Math.min(radius, width / 2, height / 2));

  ctx.beginPath();
  ctx.moveTo(x + safeRadius, y);
  ctx.lineTo(x + width - safeRadius, y);
  ctx.quadraticCurveTo(x + width, y, x + width, y + safeRadius);
  ctx.lineTo(x + width, y + height - safeRadius);
  ctx.quadraticCurveTo(x + width, y + height, x + width - safeRadius, y + height);
  ctx.lineTo(x + safeRadius, y + height);
  ctx.quadraticCurveTo(x, y + height, x, y + height - safeRadius);
  ctx.lineTo(x, y + safeRadius);
  ctx.quadraticCurveTo(x, y, x + safeRadius, y);
  ctx.closePath();
}

function fillRoundedRect(ctx, x, y, width, height, radius, fillStyle) {
  roundedRectPath(ctx, x, y, width, height, radius);
  ctx.fillStyle = fillStyle;
  ctx.fill();
}

function strokeRoundedRect(ctx, x, y, width, height, radius, strokeStyle, lineWidth = 1) {
  roundedRectPath(ctx, x, y, width, height, radius);
  ctx.strokeStyle = strokeStyle;
  ctx.lineWidth = lineWidth;
  ctx.stroke();
}

function ellipsizeCanvasText(ctx, text, maxWidth) {
  const clean = metadataString(text);
  if (!clean || ctx.measureText(clean).width <= maxWidth) return clean;

  const ellipsis = "...";
  let low = 0;
  let high = clean.length;

  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (ctx.measureText(`${clean.slice(0, mid)}${ellipsis}`).width <= maxWidth) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }

  return `${clean.slice(0, low).trimEnd()}${ellipsis}`;
}

function ellipsizeCanvasTextWithSuffix(ctx, text, maxWidth, suffix = "...") {
  const clean = metadataString(text);
  const cleanSuffix = metadataString(suffix) || "...";
  if (!clean) return "";
  if (ctx.measureText(`${clean}${cleanSuffix}`).width <= maxWidth) {
    return `${clean}${cleanSuffix}`;
  }

  const suffixWidth = ctx.measureText(cleanSuffix).width;
  if (suffixWidth >= maxWidth) return ellipsizeCanvasText(ctx, cleanSuffix, maxWidth);

  let low = 0;
  let high = clean.length;

  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (ctx.measureText(`${clean.slice(0, mid).trimEnd()}${cleanSuffix}`).width <= maxWidth) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }

  return `${clean.slice(0, low).trimEnd()}${cleanSuffix}`;
}

function wrapCanvasText(ctx, text, maxWidth, maxLines) {
  const clean = metadataString(text);
  if (!clean || maxLines <= 0) return [];

  const lines = [];
  const preserveAll = maxLines === Number.POSITIVE_INFINITY;
  let truncated = false;
  const pushLine = (value) => {
    const line = metadataString(value);
    if (!line) return;
    if (!preserveAll || ctx.measureText(line).width <= maxWidth) {
      lines.push(preserveAll ? line : ellipsizeCanvasText(ctx, line, maxWidth));
      return;
    }

    let chunk = "";
    for (const character of line) {
      const candidate = `${chunk}${character}`;
      if (chunk && ctx.measureText(candidate).width > maxWidth) {
        lines.push(chunk);
        chunk = character;
      } else {
        chunk = candidate;
      }
    }
    if (chunk) lines.push(chunk);
  };
  const paragraphs = clean.split(/\n+/);

  paragraphLoop:
  for (let paragraphIndex = 0; paragraphIndex < paragraphs.length; paragraphIndex += 1) {
    const paragraph = paragraphs[paragraphIndex];
    const words = paragraph.trim().split(/\s+/).filter(Boolean);
    let line = "";

    for (let wordIndex = 0; wordIndex < words.length; wordIndex += 1) {
      const word = words[wordIndex];
      const candidate = line ? `${line} ${word}` : word;

      if (ctx.measureText(candidate).width <= maxWidth) {
        line = candidate;
        continue;
      }

      if (line) pushLine(line);
      line = word;

      if (lines.length >= maxLines) {
        truncated = wordIndex < words.length || paragraphIndex < paragraphs.length - 1;
        break paragraphLoop;
      }
    }

    if (lines.length >= maxLines) {
      truncated = paragraphIndex < paragraphs.length - 1;
      break;
    }
    if (line) pushLine(line);
    if (lines.length >= maxLines) {
      truncated = paragraphIndex < paragraphs.length - 1;
      break;
    }
  }

  if (lines.length > maxLines) {
    lines.length = maxLines;
    truncated = true;
  }
  if (truncated && lines.length === maxLines) {
    lines[maxLines - 1] = ellipsizeCanvasTextWithSuffix(ctx, lines[maxLines - 1], maxWidth);
  }

  return lines;
}

function wrapFullCanvasText(ctx, text, maxWidth) {
  return wrapCanvasText(ctx, text, maxWidth, Number.POSITIVE_INFINITY);
}

function isTaintedCanvasError(error) {
  return /tainted|may not be exported|securityerror/i.test(String(error?.message || error || ""));
}

function cssNumber(value, fallback = 0) {
  const number = Number.parseFloat(value);
  return Number.isFinite(number) ? number : fallback;
}

function isTransparentCssColor(value) {
  const clean = metadataString(value).toLowerCase().replace(/\s+/g, "");
  return !clean || clean === "transparent" || /^rgba?\(0,0,0,0\)$/.test(clean);
}

function cssColor(value, fallback = "transparent") {
  const clean = metadataString(value);
  return isTransparentCssColor(clean) ? fallback : clean;
}

function cssRadius(style, rect, fallback = 0) {
  const radius = cssNumber(style.borderTopLeftRadius || style.borderRadius, fallback);
  return Math.max(0, Math.min(radius, rect.width / 2, rect.height / 2));
}

function cssBorderWidth(style) {
  return Math.max(
    cssNumber(style.borderTopWidth, 0),
    cssNumber(style.borderRightWidth, 0),
    cssNumber(style.borderBottomWidth, 0),
    cssNumber(style.borderLeftWidth, 0)
  );
}

function relativeRect(element, rootRect) {
  if (!element) return null;

  const rect = element.getBoundingClientRect();
  if (
    !Number.isFinite(rect.width) ||
    !Number.isFinite(rect.height) ||
    rect.width <= 0 ||
    rect.height <= 0
  ) {
    return null;
  }

  return {
    x: rect.left - rootRect.left,
    y: rect.top - rootRect.top,
    width: rect.width,
    height: rect.height,
  };
}

function queryTwitterPostRenderElements(root) {
  const query = (selector) => root.querySelector(selector);

  return {
    root,
    card: query("[data-card], .card, .post-card"),
    header: query("[data-header], .header"),
    avatar: query("[data-avatar], .avatar"),
    displayName: query("[data-display-name], .display-name, .author strong"),
    handle: query("[data-handle], .handle, .meta span:first-child"),
    metaDot: query("[data-meta-dot], .dot"),
    date: query("[data-date], .date, .meta span:last-child"),
    platformBadge: query("[data-platform-badge], .platform-badge, .platform-pill"),
    platformMark: query("[data-platform-mark], .x-logo"),
    divider: query("[data-divider], .divider"),
    tweetText: query("[data-tweet-text], .tweet-text"),
    mediaFrame: query("[data-media-frame], .media-frame, .media-shell"),
    videoSlot: query("[data-video-slot]"),
    videoOverlay: query("[data-video-overlay], .video-overlay"),
    qualityChip: query("[data-quality-chip], .quality-chip, .media-label span:first-child"),
    durationChip: query("[data-duration-chip], .duration-chip, .media-label span:last-child"),
    footer: query("[data-footer], .footer"),
    footerRule: query("[data-footer-rule], .footer-rule"),
    actionCounts: Array.from(root.querySelectorAll("[data-action-count]")),
    icons: Array.from(root.querySelectorAll("[data-icon]")),
    renderBoxes: Array.from(
      root.querySelectorAll("[data-render-box], [data-action-row], [data-action-item], [data-source-row]")
    ),
    sourceLabel: query("[data-source-label], .source-label"),
    brandMark: query("[data-brand-mark], .brand-mark"),
  };
}

function cssGradientColors(backgroundImage) {
  return metadataString(backgroundImage).match(/rgba?\([^)]+\)|#[0-9a-fA-F]{3,8}|transparent/g) || [];
}

function fillCssBackground(ctx, rect, style, fallback = "transparent") {
  const backgroundImage = metadataString(style.backgroundImage);
  const backgroundColor = cssColor(style.backgroundColor, fallback);

  if (/linear-gradient/i.test(backgroundImage)) {
    const colors = cssGradientColors(backgroundImage);
    const gradient = /to top/i.test(backgroundImage)
      ? ctx.createLinearGradient(rect.x, rect.y + rect.height, rect.x, rect.y)
      : /90deg|to right/i.test(backgroundImage)
        ? ctx.createLinearGradient(rect.x, rect.y, rect.x + rect.width, rect.y)
        : ctx.createLinearGradient(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);

    if (colors.length > 0) {
      colors.forEach((color, index) => {
        gradient.addColorStop(colors.length === 1 ? 0 : index / (colors.length - 1), color);
      });
      ctx.fillStyle = gradient;
      ctx.fill();
      return;
    }
  }

  if (/radial-gradient/i.test(backgroundImage)) {
    const colors = cssGradientColors(backgroundImage);
    const centerX = rect.x + rect.width / 2;
    const centerY = rect.y + rect.height / 2;

    ctx.save();
    ctx.translate(centerX, centerY);
    ctx.scale(Math.max(rect.width / rect.height, 0.01), 1);
    const gradient = ctx.createRadialGradient(0, 0, 0, 0, 0, rect.height / 2);

    if (colors.length > 0) {
      colors.forEach((color, index) => {
        gradient.addColorStop(colors.length === 1 ? 0 : index / (colors.length - 1), color);
      });
      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.arc(0, 0, rect.height / 2, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
      return;
    }

    ctx.restore();
  }

  if (!isTransparentCssColor(backgroundColor)) {
    ctx.fillStyle = backgroundColor;
    ctx.fill();
  }
}

function fillElementBackground(ctx, element, rootRect, fallback = "transparent") {
  const rect = relativeRect(element, rootRect);
  if (!rect) return null;

  const style = getComputedStyle(element);
  const radius = cssRadius(style, rect);

  roundedRectPath(ctx, rect.x, rect.y, rect.width, rect.height, radius);
  fillCssBackground(ctx, rect, style, fallback);

  return { rect, style, radius };
}

function drawElementBox(ctx, element, rootRect, fallback = "transparent") {
  const box = fillElementBackground(ctx, element, rootRect, fallback);
  if (!box) return null;

  const { rect, style, radius } = box;
  const borderWidth = cssBorderWidth(style);
  const borderColor = cssColor(style.borderTopColor, "transparent");

  if (borderWidth > 0 && !isTransparentCssColor(borderColor)) {
    strokeRoundedRect(ctx, rect.x, rect.y, rect.width, rect.height, radius, borderColor, borderWidth);
  }

  return box;
}

function drawElementPseudoBackground(ctx, element, rootRect, pseudo) {
  if (!element) return;

  const elementRect = relativeRect(element, rootRect);
  if (!elementRect) return;

  const style = getComputedStyle(element, pseudo);
  const content = metadataString(style.content);
  if (!content || content === "none" || content === "normal") return;

  const width = cssNumber(style.width, 0);
  const height = cssNumber(style.height, 0);
  if (width <= 0 || height <= 0) return;

  const left = cssNumber(style.left, NaN);
  const right = cssNumber(style.right, NaN);
  const top = cssNumber(style.top, NaN);
  const bottom = cssNumber(style.bottom, NaN);
  const x = Number.isFinite(left)
    ? elementRect.x + left
    : Number.isFinite(right)
      ? elementRect.x + elementRect.width - right - width
      : elementRect.x;
  const y = Number.isFinite(top)
    ? elementRect.y + top
    : Number.isFinite(bottom)
      ? elementRect.y + elementRect.height - bottom - height
      : elementRect.y;

  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, width, height);
  fillCssBackground(ctx, { x, y, width, height }, style, "transparent");
  ctx.restore();
}

function canvasFontFromComputedStyle(style) {
  const fontStyle = metadataString(style.fontStyle);
  const fontWeight = metadataString(style.fontWeight) || "400";
  const fontSize = metadataString(style.fontSize) || "16px";
  const fontFamily =
    metadataString(style.fontFamily) || 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Arial, sans-serif';

  return `${fontStyle && fontStyle !== "normal" ? `${fontStyle} ` : ""}${fontWeight} ${fontSize} ${fontFamily}`;
}

function canvasTextLineHeight(style, fallbackSize = 16) {
  const lineHeight = cssNumber(style.lineHeight, NaN);
  if (Number.isFinite(lineHeight) && lineHeight > 0) return lineHeight;

  return cssNumber(style.fontSize, fallbackSize) * 1.2;
}

function elementText(element, fallback = "") {
  return metadataString(element?.textContent) || metadataString(fallback);
}

function drawSingleLineElementText(ctx, element, rootRect, fallback = "", options = {}) {
  const text = elementText(element, fallback);
  const rect = relativeRect(element, rootRect);
  if (!text || !rect) return;

  const style = getComputedStyle(element);
  const paddingLeft = cssNumber(style.paddingLeft, 0);
  const paddingRight = cssNumber(style.paddingRight, 0);
  const fontSize = cssNumber(style.fontSize, 16);
  const availableWidth = Math.max(0, rect.width - paddingLeft - paddingRight);
  const align = options.align || style.textAlign || "left";
  const valign = options.valign || "top";
  const x =
    align === "center"
      ? rect.x + rect.width / 2
      : align === "right"
        ? rect.x + rect.width - paddingRight
        : rect.x + paddingLeft;
  const y = valign === "center" ? rect.y + (rect.height - fontSize) / 2 : rect.y + cssNumber(style.paddingTop, 0);

  ctx.save();
  ctx.font = canvasFontFromComputedStyle(style);
  ctx.fillStyle = cssColor(style.color, "#ffffff");
  ctx.textAlign = align;
  ctx.textBaseline = "top";
  ctx.fillText(ellipsizeCanvasText(ctx, text, availableWidth), x, y);
  ctx.restore();
}

function drawWrappedElementText(ctx, element, rootRect, fallback = "") {
  const text = elementText(element, fallback);
  const rect = relativeRect(element, rootRect);
  if (!text || !rect) return;

  const style = getComputedStyle(element);
  const paddingLeft = cssNumber(style.paddingLeft, 0);
  const paddingRight = cssNumber(style.paddingRight, 0);
  const paddingTop = cssNumber(style.paddingTop, 0);
  const maxWidth = Math.max(0, rect.width - paddingLeft - paddingRight);
  const lineHeight = canvasTextLineHeight(style, 30);
  const clamp = Number.parseInt(style.webkitLineClamp || style.lineClamp || "", 10);
  const maxLines = Number.isFinite(clamp) && clamp > 0 ? clamp : Math.max(1, Math.floor(rect.height / lineHeight));

  ctx.save();
  ctx.font = canvasFontFromComputedStyle(style);
  ctx.fillStyle = cssColor(style.color, "#ffffff");
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  wrapCanvasText(ctx, text, maxWidth, maxLines).forEach((line, index) => {
    ctx.fillText(line, rect.x + paddingLeft, rect.y + paddingTop + index * lineHeight);
  });

  ctx.restore();
}

function safeRasterImageDataUrl(value) {
  const clean = metadataString(value);
  if (!/^data:image\/(?:png|jpe?g|webp|gif|avif);base64,/i.test(clean)) return "";
  return clean;
}

function drawCircularImage(ctx, image, rect, inset = 0) {
  if (!image || !rect) return false;

  const x = rect.x + inset;
  const y = rect.y + inset;
  const width = Math.max(0, rect.width - inset * 2);
  const height = Math.max(0, rect.height - inset * 2);
  const radius = Math.min(width, height) / 2;

  if (width <= 0 || height <= 0 || radius <= 0) return false;

  ctx.save();
  ctx.beginPath();
  ctx.arc(x + width / 2, y + height / 2, radius, 0, Math.PI * 2);
  ctx.clip();
  ctx.drawImage(image, x, y, width, height);
  ctx.restore();

  return true;
}

function drawContainedImage(ctx, image, rect) {
  if (!image || !rect) return false;

  const naturalWidth = Number(image.naturalWidth || image.width || 0);
  const naturalHeight = Number(image.naturalHeight || image.height || 0);
  const width = Number(rect.width || 0);
  const height = Number(rect.height || 0);

  if (naturalWidth <= 0 || naturalHeight <= 0 || width <= 0 || height <= 0) {
    return false;
  }

  const scale = Math.min(width / naturalWidth, height / naturalHeight);
  const drawWidth = naturalWidth * scale;
  const drawHeight = naturalHeight * scale;
  const drawX = rect.x + (width - drawWidth) / 2;
  const drawY = rect.y + (height - drawHeight) / 2;

  ctx.drawImage(image, drawX, drawY, drawWidth, drawHeight);
  return true;
}

function imageAspectRatioFromItem(image, item = {}) {
  const width = Number(image?.naturalWidth || image?.width || item?.width || 0);
  const height = Number(image?.naturalHeight || image?.height || item?.height || 0);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return 16 / 9;
  }

  return width / height;
}

function insetRect(rect, amount) {
  return {
    x: rect.x + amount,
    y: rect.y + amount,
    width: Math.max(0, rect.width - amount * 2),
    height: Math.max(0, rect.height - amount * 2),
  };
}

function iconDrawingRect(rect) {
  const size = Math.max(0, Math.min(rect.width, rect.height));
  const inset = size * 0.08;
  const x = rect.x + (rect.width - size) / 2;
  const y = rect.y + (rect.height - size) / 2;

  return insetRect({ x, y, width: size, height: size }, inset);
}

function canvasElementOpacity(element) {
  const opacity = Number.parseFloat(getComputedStyle(element).opacity);
  return Number.isFinite(opacity) ? Math.max(0, Math.min(opacity, 1)) : 1;
}

const TWITTER_ACTION_ICON_VIEWBOX = 24;
// Lucide Icons (ISC); see THIRD_PARTY_NOTICES.md.
const TWITTER_ACTION_ICON_PATHS = {
  reply:
    "M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092a10 10 0 1 0-4.777-4.719",
  repost:
    "m2 9 3-3 3 3 M13 18H7a2 2 0 0 1-2-2V6 m17 9-3 3-3-3 M11 6h6a2 2 0 0 1 2 2v10",
  like:
    "M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5",
  share:
    "M12 2v13 m4-9-4-4-4 4 M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8",
  bookmark:
    "M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z",
};
const TWITTER_ACTION_ICON_PATH_CACHE = new Map();
const TWITTER_ACTION_ICON_FALLBACKS = {
  reply(ctx) {
    ctx.moveTo(21, 11.5);
    ctx.bezierCurveTo(21, 16.2, 17, 20, 12, 20);
    ctx.bezierCurveTo(10.8, 20, 9.7, 19.8, 8.7, 19.4);
    ctx.lineTo(4, 21);
    ctx.lineTo(5.3, 16.9);
    ctx.bezierCurveTo(3.9, 15.4, 3, 13.5, 3, 11.5);
    ctx.bezierCurveTo(3, 6.8, 7, 3, 12, 3);
    ctx.bezierCurveTo(17, 3, 21, 6.8, 21, 11.5);
    ctx.closePath();
  },
  repost(ctx) {
    ctx.moveTo(3.5, 7.2);
    ctx.lineTo(16.6, 7.2);
    ctx.bezierCurveTo(18.5, 7.2, 20, 8.7, 20, 10.6);
    ctx.lineTo(20, 11.4);

    ctx.moveTo(16.8, 3.9);
    ctx.lineTo(20.1, 7.2);
    ctx.lineTo(16.8, 10.5);

    ctx.moveTo(20.5, 16.8);
    ctx.lineTo(7.4, 16.8);
    ctx.bezierCurveTo(5.5, 16.8, 4, 15.3, 4, 13.4);
    ctx.lineTo(4, 12.6);

    ctx.moveTo(7.2, 20.1);
    ctx.lineTo(3.9, 16.8);
    ctx.lineTo(7.2, 13.5);
  },
  like(ctx) {
    ctx.moveTo(20.8, 4.6);
    ctx.bezierCurveTo(19.1, 2.9, 16.4, 2.9, 14.7, 4.6);
    ctx.lineTo(12, 7.3);
    ctx.lineTo(9.3, 4.6);
    ctx.bezierCurveTo(7.6, 2.9, 4.9, 2.9, 3.2, 4.6);
    ctx.bezierCurveTo(1.4, 6.4, 1.4, 9.3, 3.2, 11.1);
    ctx.lineTo(12, 20);
    ctx.lineTo(20.8, 11.1);
    ctx.bezierCurveTo(22.6, 9.3, 22.6, 6.4, 20.8, 4.6);
    ctx.closePath();
  },
  share(ctx) {
    ctx.moveTo(12, 3.5);
    ctx.lineTo(12, 15.2);

    ctx.moveTo(7.3, 8.2);
    ctx.lineTo(12, 3.5);
    ctx.lineTo(16.7, 8.2);

    ctx.moveTo(5.2, 13.2);
    ctx.lineTo(5.2, 18.2);
    ctx.bezierCurveTo(5.2, 19.6, 6.3, 20.7, 7.7, 20.7);
    ctx.lineTo(16.3, 20.7);
    ctx.bezierCurveTo(17.7, 20.7, 18.8, 19.6, 18.8, 18.2);
    ctx.lineTo(18.8, 13.2);
  },
  bookmark(ctx) {
    ctx.moveTo(7.7, 3);
    ctx.lineTo(16.3, 3);
    ctx.bezierCurveTo(17, 3, 17.5, 3.5, 17.5, 4.2);
    ctx.lineTo(17.5, 20.5);
    ctx.lineTo(12, 17.2);
    ctx.lineTo(6.5, 20.5);
    ctx.lineTo(6.5, 4.2);
    ctx.bezierCurveTo(6.5, 3.5, 7, 3, 7.7, 3);
    ctx.closePath();
  },
};

function cachedCanvasPath(path) {
  if (typeof Path2D === "undefined") return null;

  const cachedPath = TWITTER_ACTION_ICON_PATH_CACHE.get(path);
  if (cachedPath) return cachedPath;

  try {
    const canvasPath = new Path2D(path);
    TWITTER_ACTION_ICON_PATH_CACHE.set(path, canvasPath);
    return canvasPath;
  } catch (error) {
    return null;
  }
}

function drawStrokePathIcon(ctx, path, x, y, size, color, options = {}) {
  const cleanPath = typeof path === "string" ? path.trim() : "";
  const iconSize = Math.max(0, Number(size) || 0);
  if (!cleanPath || iconSize <= 0) return false;

  const viewBoxSize = options.viewBoxSize || TWITTER_ACTION_ICON_VIEWBOX;
  const scale = iconSize / viewBoxSize;
  const fallback = options.fallback;

  ctx.save();
  ctx.translate(x, y);
  ctx.scale(scale, scale);
  ctx.strokeStyle = color;
  ctx.lineWidth = options.strokeWidth || 2.1;
  ctx.lineCap = options.lineCap || "round";
  ctx.lineJoin = options.lineJoin || "round";

  const canvasPath = cachedCanvasPath(cleanPath);
  if (canvasPath) {
    ctx.stroke(canvasPath);
  } else if (typeof fallback === "function") {
    ctx.beginPath();
    fallback(ctx);
    ctx.stroke();
  } else {
    ctx.restore();
    return false;
  }

  ctx.restore();
  return true;
}

function drawTwitterActionPathIcon(ctx, iconName, rect, color, options = {}) {
  const size = Math.max(0, Math.min(rect.width, rect.height));
  const x = rect.x + (rect.width - size) / 2;
  const y = rect.y + (rect.height - size) / 2;
  return drawStrokePathIcon(ctx, TWITTER_ACTION_ICON_PATHS[iconName], x, y, size, color, {
    ...options,
    fallback: options.fallback || TWITTER_ACTION_ICON_FALLBACKS[iconName],
  });
}

function iconStrokeWidth(rect, ratio = 0.095) {
  return Math.max(1.6, Math.min(rect.width, rect.height) * ratio);
}

function prepareIconStroke(ctx, rect, color, ratio = 0.095) {
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = iconStrokeWidth(rect, ratio);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
}

function drawReplyIcon(ctx, rect, color) {
  drawTwitterActionPathIcon(ctx, "reply", rect, color, { strokeWidth: 2.05 });
}

function drawRepostIcon(ctx, rect, color) {
  drawTwitterActionPathIcon(ctx, "repost", rect, color, { strokeWidth: 2.05 });
}

function drawRetweetIcon(ctx, rect, color) {
  drawRepostIcon(ctx, rect, color);
}

function drawLikeIcon(ctx, rect, color) {
  drawTwitterActionPathIcon(ctx, "like", rect, color, { strokeWidth: 2.05 });
}

function drawHeartIcon(ctx, rect, color) {
  drawLikeIcon(ctx, rect, color);
}

function drawShareIcon(ctx, rect, color) {
  drawTwitterActionPathIcon(ctx, "share", rect, color, { strokeWidth: 2.05 });
}

function drawBookmarkIcon(ctx, rect, color) {
  drawTwitterActionPathIcon(ctx, "bookmark", rect, color, { strokeWidth: 2 });
}

function drawXIcon(ctx, rect, color) {
  const r = iconDrawingRect(rect);
  const inset = Math.min(r.width, r.height) * 0.18;
  const x1 = r.x + inset;
  const y1 = r.y + inset;
  const x2 = r.x + r.width - inset;
  const y2 = r.y + r.height - inset;

  prepareIconStroke(ctx, r, color, 0.12);
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.moveTo(x2, y1);
  ctx.lineTo(x1, y2);
  ctx.stroke();
}

function drawTwitterMoreIcon(ctx, rect, color) {
  const r = iconDrawingRect(rect);
  const cy = r.y + r.height / 2;
  const radius = Math.max(1.5, r.width * 0.08);
  const spacing = r.width * 0.22;
  const cx = r.x + r.width / 2;

  ctx.save();
  ctx.fillStyle = color;
  for (const x of [cx - spacing, cx, cx + spacing]) {
    ctx.beginPath();
    ctx.arc(x, cy, radius, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

function drawVerifiedIcon(ctx, rect, color) {
  const r = iconDrawingRect(rect);
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;
  const radius = Math.min(r.width, r.height) * 0.43;
  const innerRadius = radius * 0.82;

  ctx.fillStyle = color;
  ctx.beginPath();
  for (let index = 0; index < 16; index += 1) {
    const angle = -Math.PI / 2 + (index * Math.PI * 2) / 16;
    const pointRadius = index % 2 === 0 ? radius : innerRadius;
    const x = cx + Math.cos(angle) * pointRadius;
    const y = cy + Math.sin(angle) * pointRadius;

    if (index === 0) {
      ctx.moveTo(x, y);
    } else {
      ctx.lineTo(x, y);
    }
  }
  ctx.closePath();
  ctx.fill();

  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = Math.max(1.5, radius * 0.18);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(cx - radius * 0.42, cy + radius * 0.02);
  ctx.lineTo(cx - radius * 0.12, cy + radius * 0.3);
  ctx.lineTo(cx + radius * 0.46, cy - radius * 0.32);
  ctx.stroke();
}

function drawIconElement(ctx, element, rootRect) {
  const icon = metadataString(element?.dataset?.icon).toLowerCase();
  const rect = relativeRect(element, rootRect);
  if (!icon || !rect) return;

  const style = getComputedStyle(element);
  const color = metadataString(element.dataset.iconColor) || cssColor(style.color, "#71767b");
  const opacity = canvasElementOpacity(element);

  ctx.save();
  ctx.globalAlpha *= opacity;

  if (icon === "reply" || icon === "comment") {
    drawReplyIcon(ctx, rect, color);
  } else if (icon === "retweet" || icon === "repost") {
    drawRetweetIcon(ctx, rect, color);
  } else if (icon === "like" || icon === "heart") {
    drawHeartIcon(ctx, rect, color);
  } else if (icon === "bookmark" || icon === "save") {
    drawBookmarkIcon(ctx, rect, color);
  } else if (icon === "share" || icon === "upload") {
    drawShareIcon(ctx, rect, color);
  } else if (icon === "x") {
    drawXIcon(ctx, rect, color);
  } else if (icon === "verified" || icon === "check") {
    drawVerifiedIcon(ctx, rect, color);
  }

  ctx.restore();
}

function drawIconElements(ctx, elements, rootRect) {
  for (const element of elements || []) {
    drawIconElement(ctx, element, rootRect);
  }
}

const TWITTER_CARD_FONT_FAMILY = "MediaDropInstrumentSans";
const TWITTER_CARD_FONT_SOURCES = [
  { file: "InstrumentSans-Regular.ttf", weight: "400" },
  { file: "InstrumentSans-Medium.ttf", weight: "500" },
  { file: "InstrumentSans-SemiBold.ttf", weight: "600" },
  { file: "InstrumentSans-Bold.ttf", weight: "700" },
];
let twitterCardFontLoadPromise = null;

async function ensureTwitterCardFontsLoaded() {
  if (!("FontFace" in window) || !document.fonts) return false;

  if (!twitterCardFontLoadPromise) {
    twitterCardFontLoadPromise = Promise.all(
      TWITTER_CARD_FONT_SOURCES.map(async ({ file, weight }) => {
        const fontFace = new FontFace(
          TWITTER_CARD_FONT_FAMILY,
          `url("${new URL(`./assets/fonts/${file}`, window.location.href).href}") format("truetype")`,
          { style: "normal", weight, display: "swap" }
        );

        const loadedFace = await fontFace.load();
        document.fonts.add(loadedFace);
        return loadedFace;
      })
    )
      .then(async () => {
        await document.fonts.ready;
        return true;
      })
      .catch((error) => {
        console.warn("Twitter card font load failed, falling back to system fonts:", error);
        return false;
      });
  }

  return twitterCardFontLoadPromise;
}

function twitterCanvasFont(size, weight = 400) {
  return `${weight} ${size}px "${TWITTER_CARD_FONT_FAMILY}", "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", "Twemoji Mozilla", system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Arial, sans-serif`;
}

function drawCanvasLineText(ctx, text, x, y, options = {}) {
  const clean = metadataString(text);
  if (!clean) return 0;

  const size = options.size || 18;
  const weight = options.weight || 400;
  const maxWidth = Math.max(0, options.maxWidth || 0);
  const color = options.color || "#e7e9ea";
  const align = options.align || "left";
  const baseline = options.baseline || "top";

  ctx.save();
  ctx.font = twitterCanvasFont(size, weight);
  ctx.fillStyle = color;
  ctx.textAlign = align;
  ctx.textBaseline = baseline;

  const value = maxWidth > 0 ? ellipsizeCanvasText(ctx, clean, maxWidth) : clean;
  ctx.fillText(value, x, y);
  const width = ctx.measureText(value).width;
  ctx.restore();

  return width;
}

function drawCanvasTextLines(ctx, lines, x, y, lineHeight, options = {}) {
  ctx.save();
  ctx.font = twitterCanvasFont(options.size || 27, options.weight || 400);
  ctx.fillStyle = options.color || "#e7e9ea";
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  lines.forEach((line, index) => {
    ctx.fillText(line, x, y + index * lineHeight);
  });

  ctx.restore();
}

function drawTwitterPostTextLines(ctx, lines, x, y, lineHeight, options = {}) {
  const textColor = options.color || "#e7e9ea";
  const accentColor = options.accentColor || "#1d9bf0";

  ctx.save();
  ctx.font = twitterCanvasFont(options.size || 26, options.weight || 400);
  ctx.textAlign = "left";
  ctx.textBaseline = "top";

  lines.forEach((line, lineIndex) => {
    let segmentX = x;
    for (const segment of line.match(/\S+|\s+/g) || []) {
      const isAccent = /^[@#][\p{L}\p{N}_]+/u.test(segment);
      ctx.fillStyle = isAccent ? accentColor : textColor;
      ctx.fillText(segment, segmentX, y + lineIndex * lineHeight);
      segmentX += ctx.measureText(segment).width;
    }
  });

  ctx.restore();
}

function twitterAvatarFallbackColor(seed) {
  const colors = [
    "#333639",
    "#1d4ed8",
    "#0f766e",
    "#7c3aed",
    "#be123c",
    "#a16207",
  ];
  const clean = metadataString(seed);
  if (!clean) return colors[0];

  let hash = 0;
  for (let index = 0; index < clean.length; index += 1) {
    hash = (hash * 31 + clean.charCodeAt(index)) >>> 0;
  }

  return colors[hash % colors.length];
}

function drawTwitterAvatarFallback(ctx, rect, initial, seed = "") {
  const cx = rect.x + rect.width / 2;
  const cy = rect.y + rect.height / 2;
  const radius = Math.min(rect.width, rect.height) / 2;

  ctx.save();
  ctx.fillStyle = twitterAvatarFallbackColor(seed || initial);
  ctx.beginPath();
  ctx.arc(cx, cy, radius, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#2f3336";
  ctx.lineWidth = 2;
  ctx.stroke();

  ctx.font = twitterCanvasFont(24, 700);
  ctx.fillStyle = "#ffffff";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(metadataString(initial) || "X", cx, cy + 1);
  ctx.restore();
}

function drawTwitterPostAvatar(ctx, image, rect, initial, seed = "") {
  if (drawCircularImage(ctx, image, rect, 0)) {
    ctx.save();
    ctx.strokeStyle = "#2f3336";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(rect.x + rect.width / 2, rect.y + rect.height / 2, rect.width / 2, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    return;
  }

  drawTwitterAvatarFallback(ctx, rect, initial, seed);
}

function drawTwitterAction(ctx, action, x, y, options = {}) {
  const iconSize = options.iconSize || 30;
  const countSize = options.countSize || 20;
  const rowHeight = options.rowHeight || 36;
  const countGap = Number.isFinite(options.countGap) ? options.countGap : 12;
  const iconRect = {
    x,
    y: y + Math.max(0, rowHeight - iconSize) / 2,
    width: iconSize,
    height: iconSize,
  };
  const color = options.color || "#71767b";
  const count = formatTwitterActionCount(action.count);

  ctx.save();
  ctx.globalAlpha *= count ? 0.9 : 0.74;

  if (action.icon === "reply") {
    drawReplyIcon(ctx, iconRect, color);
  } else if (action.icon === "retweet") {
    drawRetweetIcon(ctx, iconRect, color);
  } else if (action.icon === "like") {
    drawHeartIcon(ctx, iconRect, color);
  } else if (action.icon === "bookmark") {
    drawBookmarkIcon(ctx, iconRect, color);
  } else if (action.icon === "share") {
    drawShareIcon(ctx, iconRect, color);
  }

  if (count) {
    drawCanvasLineText(
      ctx,
      count,
      x + iconSize + countGap,
      y + Math.max(0, rowHeight - countSize) / 2 + 1,
      {
        size: countSize,
        color,
        maxWidth: options.countMaxWidth || 118,
      }
    );
  }

  ctx.restore();
}

function twitterCardSourceText(metadata = {}, values = {}) {
  const viewCount = formatTwitterActionCount(metadata.viewCount);
  const source = metadataString(values.source_label || metadata.sourceLabel || "x.com");

  if (viewCount && source) return `${viewCount} görüntülenme · ${source}`;
  if (viewCount) return `${viewCount} görüntülenme`;

  return "";
}

function twitterViewCountText(metadata = {}) {
  const viewCount = formatTwitterActionCount(metadata.viewCount);
  return viewCount ? `${viewCount} görüntülenme` : "";
}

function twitterScreenshotMetaText(metadata = {}, values = {}) {
  const segments = [];
  const date = metadataString(values.date || metadata.displayDate);
  const viewText = twitterViewCountText(metadata);
  const source = metadataString(metadata.sourceLabel);

  if (date) segments.push(date);
  if (viewText) segments.push(viewText);
  if (source && source.toLowerCase() !== "x.com") segments.push(source);

  return segments.join(" · ");
}

async function renderTwitterDarkPostCard(metadata = {}, values = twitterPostTemplateValues(metadata)) {
  const outputWidth = 1080;
  const outerBg = "#0b0f14";
  const cardBg = "#111820";
  const headerBg = "#151f2a";
  const bodyBg = "#10161d";
  const footerBg = "#10161d";
  const subtlePanel = "#17222e";
  const mediaBg = "#05070a";
  const borderColor = "#2b3947";
  const dividerColor = "#26323f";
  const textColor = "#eef3f8";
  const secondaryColor = "#8b98a5";
  const mutedColor = "#66727f";
  const accentColor = "#1d9bf0";
  const cardX = 28;
  const cardY = 28;
  const cardWidth = 1024;
  const cardRadius = 26;
  const cardInset = 20;
  const contentInset = 40;
  const headerPanelX = cardX + cardInset;
  const headerPanelY = cardY + cardInset;
  const headerPanelWidth = cardWidth - cardInset * 2;
  const headerPanelHeight = 92;
  const headerPanelRadius = 20;
  const avatarSize = 56;
  const avatarRect = {
    x: headerPanelX + 18,
    y: headerPanelY + 18,
    width: avatarSize,
    height: avatarSize,
  };
  const headerTextX = avatarRect.x + avatarSize + 16;
  const headerTextWidth = headerPanelX + headerPanelWidth - headerTextX - 58;
  const textPanelX = headerPanelX;
  const textPanelY = headerPanelY + headerPanelHeight + 14;
  const textPanelWidth = headerPanelWidth;
  const textPanelPadX = 20;
  const textPanelPadY = 18;
  const tweetX = textPanelX + textPanelPadX;
  const tweetWidth = evenFloor(textPanelWidth - textPanelPadX * 2);
  const mediaFrameX = cardX + contentInset;
  const mediaFrameWidth = evenFloor(cardWidth - contentInset * 2);
  const footerPanelX = headerPanelX;
  const footerPanelWidth = headerPanelWidth;
  const footerPanelHeight = 64;

  const measureCanvas = document.createElement("canvas");
  const measureCtx = measureCanvas.getContext("2d");
  if (!measureCtx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  const tweetText = metadataString(values.tweet_text);
  measureCtx.font = twitterCanvasFont(29, 400);
  const tweetLines = wrapCanvasText(measureCtx, tweetText, tweetWidth, 4);
  const tweetLineHeight = 41;
  const hasTweetText = tweetLines.length > 0;
  const textPanelHeight = hasTweetText
    ? evenCeil(textPanelPadY * 2 + tweetLines.length * tweetLineHeight)
    : 0;
  const mediaFrameY = hasTweetText
    ? evenFloor(textPanelY + textPanelHeight + 16)
    : evenFloor(headerPanelY + headerPanelHeight + 18);
  const mediaFrameHeight = evenCeil(mediaFrameWidth * 9 / 16);
  const videoX = evenFloor(mediaFrameX + 4);
  const videoY = evenFloor(mediaFrameY + 4);
  const videoWidth = evenFloor(mediaFrameWidth - 8);
  const videoHeight = evenFloor(mediaFrameHeight - 8);
  const footerPanelY = evenFloor(mediaFrameY + mediaFrameHeight + 14);
  const actionY = footerPanelY + 8;
  const actionHeight = 48;
  const viewText = twitterViewCountText(metadata);
  const cardHeight = evenCeil(footerPanelY + footerPanelHeight + cardInset - cardY);
  const outputHeight = evenCeil(cardY + cardHeight + 28);

  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.fillStyle = outerBg;
  ctx.fillRect(0, 0, outputWidth, outputHeight);

  fillRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, cardRadius, cardBg);
  strokeRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, cardRadius, borderColor, 1.5);

  fillRoundedRect(ctx, headerPanelX, headerPanelY, headerPanelWidth, headerPanelHeight, headerPanelRadius, headerBg);
  strokeRoundedRect(ctx, headerPanelX, headerPanelY, headerPanelWidth, headerPanelHeight, headerPanelRadius, dividerColor, 1);

  const displayName = metadataString(values.display_name) || "X/Twitter";
  const avatarImage = await loadRasterImageSource(values.avatar_data_url);
  drawTwitterPostAvatar(ctx, avatarImage, avatarRect, values.avatar_initial, displayName || values.handle);

  ctx.font = twitterCanvasFont(26, 700);
  const nameMaxWidth = Math.max(120, headerTextWidth - 36);
  const nameText = ellipsizeCanvasText(ctx, displayName, nameMaxWidth);
  drawCanvasLineText(ctx, nameText, headerTextX, headerPanelY + 18, {
    size: 26,
    weight: 700,
    color: textColor,
    maxWidth: nameMaxWidth,
  });

  const nameWidth = Math.min(ctx.measureText(nameText).width, nameMaxWidth);
  if (metadata?.isVerified) {
    drawVerifiedIcon(ctx, {
      x: headerTextX + nameWidth + 8,
      y: headerPanelY + 22,
      width: 18,
      height: 18,
    }, accentColor);
  }

  const handle = metadataString(values.handle);
  const date = metadataString(values.date);
  const metaY = headerPanelY + 51;
  let metaX = headerTextX;

  if (handle) {
    const handleWidth = drawCanvasLineText(ctx, handle, metaX, metaY, {
      size: 20,
      color: secondaryColor,
      maxWidth: Math.min(430, headerTextWidth * 0.58),
    });
    metaX += handleWidth + 9;
  }

  if (handle && date) {
    ctx.fillStyle = mutedColor;
    ctx.beginPath();
    ctx.arc(metaX + 2, metaY + 12, 2, 0, Math.PI * 2);
    ctx.fill();
    metaX += 12;
  }

  if (date) {
    drawCanvasLineText(ctx, date, metaX, metaY, {
      size: 20,
      color: secondaryColor,
      maxWidth: Math.max(0, headerPanelX + headerPanelWidth - metaX - 58),
    });
  }

  ctx.save();
  ctx.globalAlpha = 0.44;
  drawXIcon(ctx, {
    x: headerPanelX + headerPanelWidth - 42,
    y: headerPanelY + 20,
    width: 22,
    height: 22,
  }, textColor);
  ctx.restore();

  if (hasTweetText) {
    fillRoundedRect(ctx, textPanelX, textPanelY, textPanelWidth, textPanelHeight, 18, bodyBg);
    strokeRoundedRect(ctx, textPanelX, textPanelY, textPanelWidth, textPanelHeight, 18, dividerColor, 1);
    drawCanvasTextLines(ctx, tweetLines, tweetX, textPanelY + textPanelPadY, tweetLineHeight, {
      size: 29,
      weight: 400,
      color: textColor,
    });
  }

  fillRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, 22, mediaBg);
  strokeRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, 22, borderColor, 1.5);
  strokeRoundedRect(ctx, videoX, videoY, videoWidth, videoHeight, 18, "#111820", 1);

  fillRoundedRect(ctx, footerPanelX, footerPanelY, footerPanelWidth, footerPanelHeight, 18, footerBg);
  strokeRoundedRect(ctx, footerPanelX, footerPanelY, footerPanelWidth, footerPanelHeight, 18, dividerColor, 1);

  const leftActions = [
    { icon: "reply", count: metadata.replyCount },
    { icon: "retweet", count: metadata.retweetCount },
    { icon: "like", count: metadata.likeCount },
  ];
  const actionStartX = footerPanelX + 24;
  const actionStep = 150;

  leftActions.forEach((action, index) => {
    drawTwitterAction(ctx, action, actionStartX + index * actionStep, actionY, {
      rowHeight: actionHeight,
      iconSize: 26,
      countSize: 18,
      countMaxWidth: 96,
      color: secondaryColor,
    });
  });

  const shareX = footerPanelX + footerPanelWidth - 50;
  const bookmarkX = shareX - 46;
  if (viewText) {
    drawCanvasLineText(ctx, viewText, bookmarkX - 28, actionY + 15, {
      size: 18,
      color: secondaryColor,
      align: "right",
      maxWidth: 280,
    });
  }
  drawTwitterAction(ctx, { icon: "bookmark", count: "" }, bookmarkX, actionY, {
    rowHeight: actionHeight,
    iconSize: 25,
    color: secondaryColor,
  });
  drawTwitterAction(ctx, { icon: "share", count: "" }, shareX, actionY, {
    rowHeight: actionHeight,
    iconSize: 25,
    color: secondaryColor,
  });

  let dataUrl = "";
  try {
    dataUrl = canvas.toDataURL("image/png");
  } catch (error) {
    const stage = isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed";
    throw ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter özel renderer PNG çıktısı alınamadı"
    );
  }

  return {
    dataUrl,
    layout: {
      outputWidth,
      outputHeight,
      videoX,
      videoY,
      videoWidth,
      videoHeight,
    },
  };
}

async function renderTwitterScreenshotStylePostCard(metadata = {}, values = twitterPostTemplateValues(metadata)) {
  const outputWidth = 1080;
  const outerBg = "#06090d";
  const surfaceBg = "#05070a";
  const mediaBg = "#000000";
  const borderColor = "#2f3336";
  const dividerColor = "#2f3336";
  const textColor = "#e7e9ea";
  const secondaryColor = "#71767b";
  const actionColor = "#8b98a5";
  const accentColor = "#1d9bf0";
  const surfaceX = 48;
  const surfaceY = 32;
  const surfaceWidth = 984;
  const surfacePadX = 24;
  const avatarSize = 56;
  const avatarRect = {
    x: surfaceX + surfacePadX,
    y: surfaceY + 26,
    width: avatarSize,
    height: avatarSize,
  };
  const contentX = evenFloor(avatarRect.x + avatarSize + 14);
  const contentRight = evenFloor(surfaceX + surfaceWidth - surfacePadX);
  const contentWidth = evenFloor(contentRight - contentX);
  const nameY = surfaceY + 22;
  const handleY = surfaceY + 52;
  const bodyStartY = surfaceY + 94;
  const tweetText = metadataString(values.tweet_text);
  const tweetLineHeight = 35;
  const tweetMaxLines = 5;
  const tweetFontSize = 26;

  const measureCanvas = document.createElement("canvas");
  const measureCtx = measureCanvas.getContext("2d");
  if (!measureCtx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  measureCtx.font = twitterCanvasFont(tweetFontSize, 400);
  const tweetLines = wrapCanvasText(measureCtx, tweetText, contentWidth, tweetMaxLines);
  const hasTweetText = tweetLines.length > 0;
  const tweetTextHeight = hasTweetText ? tweetLines.length * tweetLineHeight : 0;
  const mediaFrameX = contentX;
  const mediaFrameY = evenFloor(
    bodyStartY + (hasTweetText ? tweetTextHeight + 18 : 6)
  );
  const mediaFrameWidth = contentWidth;
  const mediaFrameHeight = evenCeil(mediaFrameWidth * 9 / 16);
  const videoInset = 4;
  const videoX = evenFloor(mediaFrameX + videoInset);
  const videoY = evenFloor(mediaFrameY + videoInset);
  const videoWidth = evenFloor(mediaFrameWidth - videoInset * 2);
  const videoHeight = evenFloor(mediaFrameHeight - videoInset * 2);
  const metaText = twitterScreenshotMetaText(metadata, values);
  const metaY = evenFloor(mediaFrameY + mediaFrameHeight + 16);
  const firstDividerY = evenFloor(metaY + (metaText ? 30 : 6));
  const actionY = firstDividerY + 8;
  const actionHeight = 44;
  const bottomDividerY = evenFloor(actionY + actionHeight + 8);
  const surfaceHeight = evenCeil(bottomDividerY - surfaceY + 14);
  const outputHeight = evenCeil(surfaceY + surfaceHeight + 32);

  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.fillStyle = outerBg;
  ctx.fillRect(0, 0, outputWidth, outputHeight);

  ctx.fillStyle = surfaceBg;
  ctx.fillRect(surfaceX, surfaceY, surfaceWidth, surfaceHeight);
  ctx.strokeStyle = borderColor;
  ctx.lineWidth = 1;
  ctx.strokeRect(surfaceX + 0.5, surfaceY + 0.5, surfaceWidth - 1, surfaceHeight - 1);

  const displayName = metadataString(values.display_name) || "X/Twitter";
  const avatarImage = await loadRasterImageSource(values.avatar_data_url);
  drawTwitterPostAvatar(ctx, avatarImage, avatarRect, values.avatar_initial, displayName || values.handle);

  ctx.font = twitterCanvasFont(23, 700);
  const nameMaxWidth = Math.max(120, contentWidth - 76);
  const nameText = ellipsizeCanvasText(ctx, displayName, nameMaxWidth);
  drawCanvasLineText(ctx, nameText, contentX, nameY, {
    size: 23,
    weight: 700,
    color: textColor,
    maxWidth: nameMaxWidth,
  });

  const nameWidth = Math.min(ctx.measureText(nameText).width, nameMaxWidth);
  if (metadata?.isVerified) {
    drawVerifiedIcon(ctx, {
      x: contentX + nameWidth + 7,
      y: nameY + 4,
      width: 17,
      height: 17,
    }, accentColor);
  }

  drawTwitterMoreIcon(ctx, {
    x: contentRight - 28,
    y: nameY + 3,
    width: 24,
    height: 24,
  }, secondaryColor);

  const handle = metadataString(values.handle);
  if (handle) {
    drawCanvasLineText(ctx, handle, contentX, handleY, {
      size: 19,
      color: secondaryColor,
      maxWidth: Math.max(0, contentWidth - 38),
    });
  }

  if (hasTweetText) {
    drawTwitterPostTextLines(ctx, tweetLines, contentX, bodyStartY, tweetLineHeight, {
      size: tweetFontSize,
      weight: 400,
      color: textColor,
      accentColor,
    });
  }

  fillRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, 18, mediaBg);
  strokeRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, 18, borderColor, 1);

  if (metaText) {
    drawCanvasLineText(ctx, metaText, contentX, metaY, {
      size: 18,
      color: secondaryColor,
      maxWidth: contentWidth,
    });
  }

  ctx.strokeStyle = dividerColor;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(surfaceX, firstDividerY + 0.5);
  ctx.lineTo(surfaceX + surfaceWidth, firstDividerY + 0.5);
  ctx.stroke();

  const actionOptions = {
    rowHeight: actionHeight,
    iconSize: 24,
    countSize: 17,
    countGap: 8,
    countMaxWidth: 86,
    color: actionColor,
  };
  drawTwitterAction(ctx, { icon: "reply", count: metadata.replyCount }, contentX, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "retweet", count: metadata.retweetCount }, contentX + 178, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "like", count: metadata.likeCount }, contentX + 356, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "bookmark", count: "" }, contentRight - 88, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "share", count: "" }, contentRight - 30, actionY, actionOptions);

  ctx.strokeStyle = dividerColor;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(surfaceX, bottomDividerY + 0.5);
  ctx.lineTo(surfaceX + surfaceWidth, bottomDividerY + 0.5);
  ctx.stroke();

  let dataUrl = "";
  try {
    dataUrl = canvas.toDataURL("image/png");
  } catch (error) {
    const stage = isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed";
    throw ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter özel renderer PNG çıktısı alınamadı"
    );
  }

  return {
    dataUrl,
    layout: {
      outputWidth,
      outputHeight,
      videoX,
      videoY,
      videoWidth,
      videoHeight,
    },
  };
}

async function renderTwitterQuotedPreviewPostCard(metadata = {}, values = twitterPostTemplateValues(metadata)) {
  await ensureTwitterCardFontsLoaded();

  const quotedValues = values.quoted_post || {};
  const textOnly = Boolean(metadata?.textOnly);
  const activeRole = metadata?.activeMediaRole === "quoted" ? "quoted" : "outer";
  const hasOuterMedia = !textOnly && Boolean(metadata?.hasOuterMedia);
  const hasQuotedMedia = !textOnly && Boolean(metadata?.hasQuotedMedia);
  const secondaryMedia = metadata?.secondaryMedia || null;
  const outputWidth = 1080;
  const cardWidth = 760;
  const cardX = evenFloor((outputWidth - cardWidth) / 2);
  const cardY = 36;
  const contentPadX = 32;
  const contentX = cardX + contentPadX;
  const contentRight = cardX + cardWidth - contentPadX;
  const contentWidth = contentRight - contentX;
  const headerY = cardY + 28;
  const avatarSize = 64;
  const tweetY = headerY + avatarSize + 42;
  const tweetLineHeight = 39;

  const measureCanvas = document.createElement("canvas");
  const measureCtx = measureCanvas.getContext("2d");
  if (!measureCtx) {
    throw twitterPostTemplateError("card_png_render_failed", "X/Twitter alıntı renderer canvas oluşturulamadı.");
  }

  measureCtx.font = twitterCanvasFont(27, 400);
  const tweetLines = wrapFullCanvasText(measureCtx, metadataString(values.tweet_text), contentWidth);
  const tweetHeight = tweetLines.length * tweetLineHeight;
  let cursorY = tweetY + (tweetHeight ? tweetHeight + 28 : 8);

  const outerMediaWidth = evenFloor(contentWidth);
  const outerAspect = activeRole === "outer"
    ? normalizedPostCardMediaAspectRatio(metadata.mediaAspectRatio)
    : normalizedPostCardMediaAspectRatio(secondaryMedia?.aspectRatio);
  const outerMediaHeight = hasOuterMedia
    ? evenCeil(Math.min(460, outerMediaWidth / outerAspect))
    : 0;
  const outerMediaRect = hasOuterMedia
    ? { x: contentX, y: evenFloor(cursorY), width: outerMediaWidth, height: outerMediaHeight }
    : null;
  if (outerMediaRect) cursorY = outerMediaRect.y + outerMediaRect.height + 24;

  const quoteX = contentX;
  const quoteY = evenFloor(cursorY);
  const quoteWidth = contentWidth;
  const quotePad = 22;
  const quoteContentX = quoteX + quotePad;
  const quoteContentWidth = quoteWidth - quotePad * 2;
  const quoteHeaderY = quoteY + quotePad;
  const quoteAvatarSize = 46;
  const quoteTextY = quoteHeaderY + quoteAvatarSize + 20;
  measureCtx.font = twitterCanvasFont(24, 400);
  const quotedLines = wrapFullCanvasText(
    measureCtx,
    metadataString(quotedValues.tweet_text),
    quoteContentWidth
  );
  const quoteLineHeight = 34;
  const quotedTextHeight = quotedLines.length * quoteLineHeight;
  const quoteMediaY = evenFloor(quoteTextY + (quotedTextHeight ? quotedTextHeight + 22 : 4));
  const quoteMediaWidth = evenFloor(quoteContentWidth);
  const quoteAspect = activeRole === "quoted"
    ? normalizedPostCardMediaAspectRatio(metadata.mediaAspectRatio)
    : normalizedPostCardMediaAspectRatio(secondaryMedia?.aspectRatio);
  const quoteMediaHeight = hasQuotedMedia
    ? evenCeil(Math.min(430, quoteMediaWidth / quoteAspect))
    : 0;
  const quoteMediaRect = hasQuotedMedia
    ? { x: quoteContentX, y: quoteMediaY, width: quoteMediaWidth, height: quoteMediaHeight }
    : null;
  const quoteBottom = quoteMediaRect
    ? quoteMediaRect.y + quoteMediaRect.height + quotePad
    : quoteTextY + quotedTextHeight + quotePad;
  const quoteHeight = evenCeil(Math.max(quoteBottom - quoteY, quoteAvatarSize + quotePad * 2));

  const footerY = quoteY + quoteHeight + 8;
  const footerHeight = 94;
  const metaY = footerY + 15;
  const actionY = footerY + 48;
  const cardHeight = evenCeil(footerY + footerHeight - cardY);
  const outputHeight = evenCeil(cardY + cardHeight + 36);
  const activeRect = textOnly
    ? null
    : activeRole === "quoted"
      ? quoteMediaRect
      : outerMediaRect;
  const layout = activeRect
    ? {
        outputWidth,
        outputHeight,
        videoX: activeRect.x,
        videoY: activeRect.y,
        videoWidth: activeRect.width,
        videoHeight: activeRect.height,
      }
    : null;

  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError("card_png_render_failed", "X/Twitter alıntı kartı canvas oluşturulamadı.");
  }
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";

  const outerGradient = ctx.createLinearGradient(0, 0, 0, outputHeight);
  outerGradient.addColorStop(0, "#07111a");
  outerGradient.addColorStop(1, "#030609");
  ctx.fillStyle = outerGradient;
  ctx.fillRect(0, 0, outputWidth, outputHeight);

  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.46)";
  ctx.shadowBlur = 38;
  ctx.shadowOffsetY = 18;
  fillRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, 34, "#070b10");
  ctx.restore();

  ctx.save();
  roundedRectPath(ctx, cardX, cardY, cardWidth, cardHeight, 34);
  ctx.clip();
  const cardGradient = ctx.createLinearGradient(cardX, cardY, cardX, cardY + cardHeight);
  cardGradient.addColorStop(0, "#0b1118");
  cardGradient.addColorStop(0.58, "#070b10");
  cardGradient.addColorStop(1, "#05080c");
  ctx.fillStyle = cardGradient;
  ctx.fillRect(cardX, cardY, cardWidth, cardHeight);

  const avatarRect = { x: contentX, y: headerY, width: avatarSize, height: avatarSize };
  const avatarImage = await loadRasterImageSource(values.avatar_data_url);
  drawTwitterPostAvatar(ctx, avatarImage, avatarRect, values.avatar_initial, values.display_name || values.handle);
  const headerTextX = avatarRect.x + avatarSize + 16;
  const followWidth = 124;
  const followHeight = 42;
  const followX = contentRight - followWidth;
  const nameMaxWidth = Math.max(120, followX - headerTextX - 24);
  const displayName = metadataString(values.display_name) || "X/Twitter";
  ctx.font = twitterCanvasFont(27, 700);
  const nameText = ellipsizeCanvasText(ctx, displayName, nameMaxWidth);
  const nameWidth = drawCanvasLineText(ctx, nameText, headerTextX, headerY + 5, {
    size: 27,
    weight: 700,
    color: "#f3f7fb",
    maxWidth: nameMaxWidth,
  });
  if (metadata?.isVerified) {
    drawVerifiedIcon(ctx, {
      x: headerTextX + nameWidth + 8,
      y: headerY + 10,
      width: 19,
      height: 19,
    }, "#1d9bf0");
  }
  drawCanvasLineText(ctx, values.handle, headerTextX, headerY + 36, {
    size: 22,
    color: "#87919d",
    maxWidth: Math.max(0, followX - headerTextX - 20),
  });
  fillRoundedRect(ctx, followX, headerY + 10, followWidth, followHeight, followHeight / 2, "#f5f8fb");
  drawCanvasLineText(ctx, "Takip et", followX + followWidth / 2, headerY + 31, {
    size: 20,
    weight: 700,
    color: "#05070a",
    align: "center",
    baseline: "middle",
    maxWidth: followWidth - 24,
  });
  drawTwitterPostTextLines(ctx, tweetLines, contentX, tweetY, tweetLineHeight, {
    size: 27,
    color: "#f3f7fb",
    accentColor: "#1d9bf0",
  });

  const secondaryImage = secondaryMedia?.source
    ? await loadRasterImageSource(secondaryMedia.source)
    : null;
  const drawMediaSlot = (rect, role) => {
    if (!rect) return;
    fillRoundedRect(ctx, rect.x, rect.y, rect.width, rect.height, 20, "#000000");
    if (secondaryImage && secondaryMedia?.role === role) {
      ctx.save();
      roundedRectPath(ctx, rect.x, rect.y, rect.width, rect.height, 20);
      ctx.clip();
      drawContainedImage(ctx, secondaryImage, rect);
      ctx.restore();
    }
    strokeRoundedRect(ctx, rect.x, rect.y, rect.width, rect.height, 20, "#24313d", 1.3);
  };
  drawMediaSlot(outerMediaRect, "outer");

  fillRoundedRect(ctx, quoteX, quoteY, quoteWidth, quoteHeight, 22, "#05090e");
  strokeRoundedRect(ctx, quoteX, quoteY, quoteWidth, quoteHeight, 22, "#2b3642", 1.4);
  const quotedAvatarRect = {
    x: quoteContentX,
    y: quoteHeaderY,
    width: quoteAvatarSize,
    height: quoteAvatarSize,
  };
  const quotedAvatar = await loadRasterImageSource(quotedValues.avatar_data_url);
  drawTwitterPostAvatar(
    ctx,
    quotedAvatar,
    quotedAvatarRect,
    quotedValues.avatar_initial,
    quotedValues.display_name || quotedValues.handle
  );
  const quotedHeaderX = quotedAvatarRect.x + quoteAvatarSize + 13;
  const quotedNameWidth = drawCanvasLineText(
    ctx,
    quotedValues.display_name || "X/Twitter",
    quotedHeaderX,
    quoteHeaderY + 1,
    { size: 23, weight: 700, color: "#f3f7fb", maxWidth: quoteContentWidth - 100 }
  );
  if (metadata?.quotedPost?.isVerified) {
    drawVerifiedIcon(ctx, {
      x: quotedHeaderX + quotedNameWidth + 7,
      y: quoteHeaderY + 5,
      width: 17,
      height: 17,
    }, "#1d9bf0");
  }
  const quotedSubline = [quotedValues.handle, quotedValues.date].filter(Boolean).join(" · ");
  drawCanvasLineText(ctx, quotedSubline, quotedHeaderX, quoteHeaderY + 27, {
    size: 18,
    color: "#87919d",
    maxWidth: quoteContentWidth - quoteAvatarSize - 13,
  });
  drawTwitterPostTextLines(ctx, quotedLines, quoteContentX, quoteTextY, quoteLineHeight, {
    size: 24,
    color: "#e8eef5",
    accentColor: "#1d9bf0",
  });
  drawMediaSlot(quoteMediaRect, "quoted");

  const actionOptions = {
    rowHeight: 44,
    iconSize: 25,
    countSize: 19,
    countGap: 8,
    countMaxWidth: 88,
    color: "#87919d",
  };
  drawTwitterAction(ctx, { icon: "reply", count: metadata.replyCount }, contentX, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "retweet", count: metadata.retweetCount }, contentX + 138, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "like", count: metadata.likeCount }, contentX + 276, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "bookmark", count: "" }, contentRight - 92, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "share", count: "" }, contentRight - 34, actionY, actionOptions);
  const metaText = twitterScreenshotMetaText(metadata, values);
  if (metaText) {
    drawCanvasLineText(ctx, metaText, contentX, metaY, {
      size: 18,
      color: "#65717d",
      maxWidth: contentWidth,
    });
  }

  ctx.restore();
  strokeRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, 34, "#28323d", 1.4);

  try {
    return {
      dataUrl: canvas.toDataURL("image/png"),
      overlayDataUrl: layout ? await renderTwitterModernPreviewOverlay(layout) : "",
      layout,
    };
  } catch (error) {
    throw ensureTwitterPostTemplateError(
      error,
      isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed",
      "X/Twitter alıntı kartı PNG çıktısı alınamadı"
    );
  }
}

async function renderTwitterModernPreviewPostCard(metadata = {}, values = twitterPostTemplateValues(metadata)) {
  if (metadata?.quotedPost) {
    return renderTwitterQuotedPreviewPostCard(metadata, values);
  }
  await ensureTwitterCardFontsLoaded();

  const textOnly = Boolean(metadata?.textOnly);
  const outputWidth = 1080;
  const outerBgTop = "#07111a";
  const outerBgBottom = "#030609";
  const cardBg = "#070b10";
  const cardBorder = "#28323d";
  const textColor = "#f3f7fb";
  const secondaryColor = "#87919d";
  const mutedColor = "#65717d";
  const mediaBg = "#000000";
  const accentColor = "#1d9bf0";
  const cardWidth = 760;
  const cardX = evenFloor((outputWidth - cardWidth) / 2);
  const cardY = 36;
  const cardRadius = 34;
  const contentPadX = 32;
  const contentX = cardX + contentPadX;
  const contentRight = cardX + cardWidth - contentPadX;
  const contentWidth = contentRight - contentX;
  const headerY = cardY + 28;
  const avatarSize = 64;
  const avatarRect = {
    x: contentX,
    y: headerY,
    width: avatarSize,
    height: avatarSize,
  };
  const headerTextX = avatarRect.x + avatarSize + 16;
  const followWidth = 124;
  const followHeight = 42;
  const followX = contentRight - followWidth;
  const followY = headerY + 10;
  const nameY = headerY + 5;
  const handleY = headerY + 36;
  const tweetY = headerY + avatarSize + 42;
  const tweetFontSize = 27;
  const tweetLineHeight = 39;
  const mediaFrameX = contentX;
  const mediaFrameWidth = evenFloor(contentWidth);
  const mediaFrameHeight = textOnly
    ? 0
    : evenCeil(mediaFrameWidth / normalizedPostCardMediaAspectRatio(metadata.mediaAspectRatio));
  const mediaFrameRadius = 20;

  const measureCanvas = document.createElement("canvas");
  const measureCtx = measureCanvas.getContext("2d");
  if (!measureCtx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  const tweetText = metadataString(values.tweet_text);
  measureCtx.font = twitterCanvasFont(tweetFontSize, 400);
  const tweetLines = wrapFullCanvasText(measureCtx, tweetText, contentWidth);
  const hasTweetText = tweetLines.length > 0;
  const tweetHeight = hasTweetText ? tweetLines.length * tweetLineHeight : 0;
  const mediaFrameY = evenFloor(
    tweetY + (hasTweetText ? tweetHeight + (textOnly ? 22 : 28) : 8)
  );
  const videoInset = 0;
  const videoX = evenFloor(mediaFrameX + videoInset);
  const videoY = evenFloor(mediaFrameY + videoInset);
  const videoWidth = evenFloor(mediaFrameWidth - videoInset * 2);
  const videoHeight = evenFloor(mediaFrameHeight - videoInset * 2);
  const footerY = evenFloor(mediaFrameY + mediaFrameHeight + (textOnly ? 4 : 0));
  const footerHeight = 94;
  const metaY = footerY + (textOnly ? 12 : 15);
  const actionY = footerY + 48;
  const actionHeight = 44;
  const cardHeight = evenCeil(footerY + footerHeight - cardY);
  const outputHeight = evenCeil(cardY + cardHeight + 36);
  const layout = textOnly
    ? null
    : {
        outputWidth,
        outputHeight,
        videoX,
        videoY,
        videoWidth,
        videoHeight,
      };

  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter özel renderer canvas oluşturulamadı."
    );
  }

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";

  const outerGradient = ctx.createLinearGradient(0, 0, 0, outputHeight);
  outerGradient.addColorStop(0, outerBgTop);
  outerGradient.addColorStop(1, outerBgBottom);
  ctx.fillStyle = outerGradient;
  ctx.fillRect(0, 0, outputWidth, outputHeight);

  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.46)";
  ctx.shadowBlur = 38;
  ctx.shadowOffsetY = 18;
  fillRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, cardRadius, cardBg);
  ctx.restore();

  ctx.save();
  roundedRectPath(ctx, cardX, cardY, cardWidth, cardHeight, cardRadius);
  ctx.clip();

  const cardGradient = ctx.createLinearGradient(cardX, cardY, cardX, cardY + cardHeight);
  cardGradient.addColorStop(0, "#0b1118");
  cardGradient.addColorStop(0.58, cardBg);
  cardGradient.addColorStop(1, "#05080c");
  ctx.fillStyle = cardGradient;
  ctx.fillRect(cardX, cardY, cardWidth, cardHeight);

  const avatarImage = await loadRasterImageSource(values.avatar_data_url);
  drawTwitterPostAvatar(ctx, avatarImage, avatarRect, values.avatar_initial, values.display_name || values.handle);

  const displayName = metadataString(values.display_name) || "X/Twitter";
  ctx.font = twitterCanvasFont(27, 700);
  const nameMaxWidth = Math.max(120, followX - headerTextX - 24);
  const nameText = ellipsizeCanvasText(ctx, displayName, nameMaxWidth);
  drawCanvasLineText(ctx, nameText, headerTextX, nameY, {
    size: 27,
    weight: 700,
    color: textColor,
    maxWidth: nameMaxWidth,
  });

  const nameWidth = Math.min(ctx.measureText(nameText).width, nameMaxWidth);
  if (metadata?.isVerified) {
    drawVerifiedIcon(ctx, {
      x: headerTextX + nameWidth + 8,
      y: nameY + 5,
      width: 19,
      height: 19,
    }, accentColor);
  }

  const handle = metadataString(values.handle);
  if (handle) {
    drawCanvasLineText(ctx, handle, headerTextX, handleY, {
      size: 22,
      color: secondaryColor,
      maxWidth: Math.max(0, followX - headerTextX - 20),
    });
  }

  fillRoundedRect(ctx, followX, followY, followWidth, followHeight, followHeight / 2, "#f5f8fb");
  drawCanvasLineText(ctx, "Takip et", followX + followWidth / 2, followY + followHeight / 2 + 0.5, {
    size: 20,
    weight: 700,
    color: "#05070a",
    align: "center",
    baseline: "middle",
    maxWidth: followWidth - 24,
  });

  if (hasTweetText) {
    drawTwitterPostTextLines(ctx, tweetLines, contentX, tweetY, tweetLineHeight, {
      size: tweetFontSize,
      weight: 400,
      color: textColor,
      accentColor,
    });
  }

  if (!textOnly) {
    fillRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, mediaFrameRadius, mediaBg);
    strokeRoundedRect(ctx, mediaFrameX, mediaFrameY, mediaFrameWidth, mediaFrameHeight, mediaFrameRadius, "#24313d", 1.3);
  }

  const actionOptions = {
    rowHeight: actionHeight,
    iconSize: 25,
    countSize: 19,
    countGap: 8,
    countMaxWidth: 88,
    color: secondaryColor,
  };
  drawTwitterAction(ctx, { icon: "reply", count: metadata.replyCount }, contentX, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "retweet", count: metadata.retweetCount }, contentX + 138, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "like", count: metadata.likeCount }, contentX + 276, actionY, {
    ...actionOptions,
    color: "#9aa4af",
  });
  drawTwitterAction(ctx, { icon: "bookmark", count: "" }, contentRight - 92, actionY, actionOptions);
  drawTwitterAction(ctx, { icon: "share", count: "" }, contentRight - 34, actionY, actionOptions);

  const metaText = twitterScreenshotMetaText(metadata, values);
  if (metaText) {
    drawCanvasLineText(ctx, metaText, contentX, metaY, {
      size: 18,
      color: mutedColor,
      maxWidth: contentWidth,
    });
  }

  ctx.restore();
  strokeRoundedRect(ctx, cardX, cardY, cardWidth, cardHeight, cardRadius, cardBorder, 1.4);

  let dataUrl = "";
  let overlayDataUrl = "";
  try {
    dataUrl = canvas.toDataURL("image/png");
    overlayDataUrl = layout ? await renderTwitterModernPreviewOverlay(layout) : "";
  } catch (error) {
    const stage = isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed";
    throw ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter modern renderer PNG çıktısı alınamadı"
    );
  }

  return {
    dataUrl,
    overlayDataUrl,
    layout,
  };
}

async function renderTwitterModernPreviewOverlay(layout) {
  const outputWidth = layout.outputWidth;
  const outputHeight = layout.outputHeight;
  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter modern overlay canvas oluşturulamadı."
    );
  }

  const slot = {
    x: layout.videoX,
    y: layout.videoY,
    width: layout.videoWidth,
    height: layout.videoHeight,
  };
  const slotRadius = 20;
  const cardBg = "#070b10";
  const borderColor = "#24313d";

  ctx.save();
  ctx.fillStyle = cardBg;
  ctx.fillRect(slot.x, slot.y, slot.width, slot.height);
  ctx.globalCompositeOperation = "destination-out";
  fillRoundedRect(ctx, slot.x, slot.y, slot.width, slot.height, slotRadius, "#000000");
  ctx.restore();

  strokeRoundedRect(ctx, slot.x, slot.y, slot.width, slot.height, slotRadius, borderColor, 1.4);

  try {
    return canvas.toDataURL("image/png");
  } catch (error) {
    console.warn("Twitter rounded video overlay export failed:", error);
    return "";
  }
}

async function renderTwitterPostCardCanvas(root, rootRect, layout, values = {}) {
  const outputWidth = layout.outputWidth;
  const outputHeight = layout.outputHeight;
  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw twitterPostTemplateError(
      "card_png_render_failed",
      "X/Twitter MP4 template canvas oluşturulamadı."
    );
  }

  ctx.textBaseline = "top";
  const elements = queryTwitterPostRenderElements(root);
  const rootStyle = getComputedStyle(root);
  ctx.fillStyle = cssColor(rootStyle.backgroundColor, "#000000");
  ctx.fillRect(0, 0, outputWidth, outputHeight);

  const cardBox = fillElementBackground(ctx, elements.card, rootRect, "#000000");
  if (cardBox) {
    ctx.save();
    roundedRectPath(
      ctx,
      cardBox.rect.x,
      cardBox.rect.y,
      cardBox.rect.width,
      cardBox.rect.height,
      cardBox.radius
    );
    ctx.clip();
    drawElementPseudoBackground(ctx, elements.card, rootRect, "::before");
    drawElementPseudoBackground(ctx, elements.card, rootRect, "::after");
    ctx.restore();

    const borderWidth = cssBorderWidth(cardBox.style);
    const borderColor = cssColor(cardBox.style.borderTopColor, "transparent");
    if (borderWidth > 0 && !isTransparentCssColor(borderColor)) {
      strokeRoundedRect(
        ctx,
        cardBox.rect.x,
        cardBox.rect.y,
        cardBox.rect.width,
        cardBox.rect.height,
        cardBox.radius,
        borderColor,
        borderWidth
      );
    }
  }

  drawElementBox(ctx, elements.header, rootRect, "transparent");
  const avatarBox = drawElementBox(ctx, elements.avatar, rootRect, "#1d2832");
  const avatarImage = await loadRasterImageSource(values.avatar_data_url);
  const hasAvatarImage = drawCircularImage(ctx, avatarImage, avatarBox?.rect, 2);

  if (!hasAvatarImage) {
    drawSingleLineElementText(ctx, elements.avatar, rootRect, values.avatar_initial, {
      align: "center",
      valign: "center",
    });
  }

  drawSingleLineElementText(ctx, elements.displayName, rootRect, values.display_name);
  drawSingleLineElementText(ctx, elements.handle, rootRect, values.handle);
  if (metadataString(values.handle) && metadataString(values.date)) {
    drawElementBox(ctx, elements.metaDot, rootRect, "#71767b");
  }
  drawSingleLineElementText(ctx, elements.date, rootRect, values.date);

  drawElementBox(ctx, elements.platformBadge, rootRect, "transparent");
  drawSingleLineElementText(ctx, elements.platformMark, rootRect, "", {
    align: "center",
    valign: "center",
  });

  drawElementBox(ctx, elements.divider, rootRect, "transparent");
  drawWrappedElementText(ctx, elements.tweetText, rootRect, values.tweet_text);

  const mediaFrameBox = drawElementBox(ctx, elements.mediaFrame, rootRect, "#000000");
  if (mediaFrameBox) {
    ctx.save();
    roundedRectPath(
      ctx,
      mediaFrameBox.rect.x,
      mediaFrameBox.rect.y,
      mediaFrameBox.rect.width,
      mediaFrameBox.rect.height,
      mediaFrameBox.radius
    );
    ctx.clip();
  }

  drawElementBox(ctx, elements.videoSlot, rootRect, "#000000");

  if (mediaFrameBox) ctx.restore();

  for (const boxElement of elements.renderBoxes || []) {
    drawElementBox(ctx, boxElement, rootRect, "transparent");
  }
  drawElementBox(ctx, elements.footerRule, rootRect, "#2f3336");
  for (const countElement of elements.actionCounts || []) {
    drawSingleLineElementText(ctx, countElement, rootRect);
  }
  drawSingleLineElementText(ctx, elements.sourceLabel, rootRect, values.source_label);
  drawSingleLineElementText(ctx, elements.brandMark, rootRect, values.platform_label);
  drawIconElements(ctx, elements.icons, rootRect);

  let dataUrl = "";
  try {
    dataUrl = canvas.toDataURL("image/png");
  } catch (error) {
    const stage = isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed";
    throw ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter MP4 template canvas PNG çıktısı alınamadı"
    );
  }

  return dataUrl;
}

async function renderTwitterPurposeBuiltPostCardPng(metadata = {}) {
  let stage = "template_load_failed";
  const debugSnapshot = {
    stage,
    renderer: "twitter-modern-preview-canvas2d",
    templateLength: 0,
    renderedLength: 0,
    layout: null,
    dataUrlLength: 0,
    overlayDataUrlLength: 0,
  };

  try {
    const template = await loadTwitterPostMp4Template();
    debugSnapshot.templateLength = template.length;

    stage = "template_contains_external_resource";
    debugSnapshot.stage = stage;
    assertTwitterPostTemplateExportSafe(template, "Template");

    stage = "placeholder_render_failed";
    debugSnapshot.stage = stage;
    const templateValues = twitterPostTemplateValues(metadata);
    const html = fillTwitterPostMp4Template(template, templateValues);
    debugSnapshot.renderedLength = html.length;

    stage = "template_contains_external_resource";
    debugSnapshot.stage = stage;
    assertTwitterPostTemplateExportSafe(html, "Rendered template");

    if (!html.includes("data-video-slot")) {
      throw twitterPostTemplateError(
        "template_missing_data_video_slot",
        "Rendered template içinde data-video-slot bulunamadı."
      );
    }

    stage = "card_png_render_failed";
    debugSnapshot.stage = stage;
    const renderedCard = await renderTwitterModernPreviewPostCard(metadata, templateValues);
    const dataUrl = renderedCard?.dataUrl || "";
    const overlayDataUrl = renderedCard?.overlayDataUrl || "";
    const layout = renderedCard?.layout || null;
    debugSnapshot.dataUrlLength = dataUrl.length;
    debugSnapshot.overlayDataUrlLength = overlayDataUrl.length;
    debugSnapshot.layout = layout;

    stage = "card_layout_invalid";
    debugSnapshot.stage = stage;
    if (!metadata?.textOnly && !isValidTwitterPostCardLayout(layout)) {
      throw twitterPostTemplateError(
        "card_layout_invalid",
        `X/Twitter özel renderer layout geçersiz: ${JSON.stringify(layout)}`
      );
    }

    if (
      !/^data:image\/png;base64,/i.test(dataUrl) ||
      dataUrl.length <= "data:image/png;base64,".length
    ) {
      throw twitterPostTemplateError(
        "card_png_render_failed",
        "X/Twitter özel renderer PNG çıktısı geçersiz."
      );
    }

    if (
      overlayDataUrl &&
      (!/^data:image\/png;base64,/i.test(overlayDataUrl) ||
        overlayDataUrl.length <= "data:image/png;base64,".length)
    ) {
      throw twitterPostTemplateError(
        "card_png_render_failed",
        "X/Twitter özel renderer overlay PNG çıktısı geçersiz."
      );
    }

    return {
      dataUrl,
      overlayDataUrl,
      layout,
    };
  } catch (error) {
    const finalError = ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter özel renderer başarısız",
      {
        layoutSnapshot: debugSnapshot.layout,
        renderDebug: debugSnapshot,
      }
    );
    debugSnapshot.stage = finalError.stage || finalError.debugCode || stage;

    console.error("Twitter post purpose-built MP4 card render failed:", {
      stage: debugSnapshot.stage,
      debugCode: finalError.debugCode || debugSnapshot.stage,
      message: finalError.message,
      cause: finalError.cause,
      error: finalError,
      snapshot: debugSnapshot,
    });

    throw finalError;
  }
}

async function renderTwitterPostCardPng(metadata = {}) {
  return renderTwitterPurposeBuiltPostCardPng(metadata);
}

async function renderTwitterPhotoPostCardPng(metadata = {}, imageSource = "", item = {}) {
  const photoImage = await loadRasterImageSource(imageSource);
  if (!photoImage) throw new Error("Gönderi kartı için fotoğraf önizlemesi hazırlanamadı.");

  const renderedCard = await renderTwitterPostCardPng({
    ...metadata,
    mediaAspectRatio: imageAspectRatioFromItem(photoImage, item),
  });
  const baseCardDataUrl = safeRasterImageDataUrl(renderedCard?.dataUrl);
  const overlayDataUrl = safeRasterImageDataUrl(renderedCard?.overlayDataUrl);
  const layout = renderedCard?.layout || null;
  if (!baseCardDataUrl || !isValidTwitterPostCardLayout(layout)) {
    throw new Error("X/Twitter gönderi kartı şablonu hazırlanamadı.");
  }

  const [baseCardImage, overlayImage] = await Promise.all([
    loadRasterImageSource(baseCardDataUrl),
    loadRasterImageSource(overlayDataUrl),
  ]);
  if (!baseCardImage) throw new Error("X/Twitter gönderi kartı görseli oluşturulamadı.");

  const { outputWidth, outputHeight, videoX, videoY, videoWidth, videoHeight } = layout;
  const canvas = document.createElement("canvas");
  canvas.width = outputWidth;
  canvas.height = outputHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Gönderi kartı canvas oluşturulamadı.");

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(baseCardImage, 0, 0, outputWidth, outputHeight);
  const mediaSlot = { x: videoX, y: videoY, width: videoWidth, height: videoHeight };
  ctx.save();
  roundedRectPath(ctx, mediaSlot.x, mediaSlot.y, mediaSlot.width, mediaSlot.height, 20);
  ctx.clip();
  ctx.fillStyle = "#000000";
  ctx.fillRect(mediaSlot.x, mediaSlot.y, mediaSlot.width, mediaSlot.height);
  const didDrawPhoto = drawContainedImage(ctx, photoImage, mediaSlot);
  ctx.restore();
  if (!didDrawPhoto) throw new Error("Gönderi kartına fotoğraf yerleştirilemedi.");

  if (overlayImage) {
    ctx.drawImage(overlayImage, 0, 0, outputWidth, outputHeight);
  } else {
    strokeRoundedRect(
      ctx,
      mediaSlot.x,
      mediaSlot.y,
      mediaSlot.width,
      mediaSlot.height,
      20,
      "#24313d",
      1.4,
    );
  }

  try {
    return {
      dataUrl: canvas.toDataURL("image/png"),
      title: metadata.text || metadata.authorName || item.title || "X/Twitter gönderisi",
    };
  } catch (error) {
    throw ensureTwitterPostTemplateError(
      error,
      isTaintedCanvasError(error) ? "tainted_canvas_detected" : "card_png_render_failed",
      "X/Twitter fotoğraf gönderi kartı PNG çıktısı alınamadı",
    );
  }
}

async function renderTwitterTextPostCardPng(metadata = {}) {
  const renderedCard = await renderTwitterPostCardPng({
    ...metadata,
    textOnly: true,
    quality: "Metin",
  });
  const dataUrl = safeRasterImageDataUrl(renderedCard?.dataUrl);
  if (!dataUrl) throw new Error("X/Twitter metin gönderisi kartı oluşturulamadı.");
  return {
    dataUrl,
    title: metadata.text || metadata.authorName || "X/Twitter gönderisi",
  };
}

async function renderTwitterTemplateFallbackPostCardPng(metadata = {}) {
  let stage = "template_load_failed";
  let host = null;
  const debugSnapshot = {
    stage,
    renderer: "template-computed-canvas2d",
    templateLength: 0,
    renderedLength: 0,
    hostConnected: false,
    rects: null,
    layout: null,
    svgLength: 0,
    dataUrlLength: 0,
  };

  try {
    stage = "template_load_failed";
    debugSnapshot.stage = stage;
    const template = await loadTwitterPostMp4Template();
    debugSnapshot.templateLength = template.length;

    stage = "template_contains_external_resource";
    debugSnapshot.stage = stage;
    assertTwitterPostTemplateExportSafe(template, "Template");

    stage = "placeholder_render_failed";
    debugSnapshot.stage = stage;
    const templateValues = twitterPostTemplateValues(metadata);
    const html = fillTwitterPostMp4Template(template, templateValues);
    debugSnapshot.renderedLength = html.length;

    stage = "template_contains_external_resource";
    debugSnapshot.stage = stage;
    assertTwitterPostTemplateExportSafe(html, "Rendered template");

    if (!html.includes("data-video-slot")) {
      stage = "template_missing_data_video_slot";
      debugSnapshot.stage = stage;
      throw twitterPostTemplateError(
        stage,
        "Rendered template içinde data-video-slot bulunamadı."
      );
    }

    const templateDoc = parseTwitterPostTemplateHtml(html);
    host = document.createElement("div");
    const shadow = host.attachShadow({ mode: "open" });

    host.setAttribute("aria-hidden", "true");
    Object.assign(host.style, {
      position: "fixed",
      left: "-10000px",
      top: "0",
      width: "1080px",
      minHeight: "1px",
      opacity: "0",
      pointerEvents: "none",
      zIndex: "-1",
    });

    shadow.innerHTML = twitterPostShadowHtml(templateDoc);
    document.body.appendChild(host);
    debugSnapshot.hostConnected = host.isConnected;

    stage = "video_slot_missing";
    debugSnapshot.stage = stage;
    const root = shadow.querySelector("[data-twitter-post-template]");
    const videoSlot = shadow.querySelector("[data-video-slot]");

    if (!root) {
      stage = "placeholder_render_failed";
      debugSnapshot.stage = stage;
      throw twitterPostTemplateError(
        "placeholder_render_failed",
        "[data-twitter-post-template] bulunamadı."
      );
    }

    if (!videoSlot) {
      throw twitterPostTemplateError("video_slot_missing", "[data-video-slot] bulunamadı.");
    }

    stage = "video_slot_invalid_rect";
    debugSnapshot.stage = stage;
    if (document.fonts?.ready) {
      await document.fonts.ready.catch(() => {});
    }
    await waitAnimationFrame();
    await waitAnimationFrame();

    const rootRect = root.getBoundingClientRect();
    const slotRect = videoSlot.getBoundingClientRect();
    const outputWidth = evenCeil(rootRect.width);
    const outputHeight = evenCeil(rootRect.height);
    const videoX = evenFloor(slotRect.left - rootRect.left);
    const videoY = evenFloor(slotRect.top - rootRect.top);
    const videoWidth = evenFloor(slotRect.width);
    const videoHeight = evenFloor(slotRect.height);
    const layout = {
      outputWidth,
      outputHeight,
      videoX,
      videoY,
      videoWidth,
      videoHeight,
    };
    debugSnapshot.rects = {
      rootWidth: rootRect.width,
      rootHeight: rootRect.height,
      slotLeft: slotRect.left,
      slotTop: slotRect.top,
      slotWidth: slotRect.width,
      slotHeight: slotRect.height,
    };
    debugSnapshot.layout = layout;

    console.debug("Twitter post MP4 template layout", layout);

    if (
      !Number.isFinite(rootRect.width) ||
      !Number.isFinite(rootRect.height) ||
      !Number.isFinite(slotRect.left) ||
      !Number.isFinite(slotRect.top) ||
      !Number.isFinite(slotRect.width) ||
      !Number.isFinite(slotRect.height) ||
      rootRect.width <= 0 ||
      rootRect.height <= 0 ||
      slotRect.width <= 0 ||
      slotRect.height <= 0
    ) {
      throw twitterPostTemplateError(
        "video_slot_invalid_rect",
        `[data-video-slot] ölçüleri geçersiz: ${JSON.stringify(debugSnapshot.rects)}`
      );
    }

    stage = "card_layout_invalid";
    debugSnapshot.stage = stage;
    if (!isValidTwitterPostCardLayout(layout)) {
      throw twitterPostTemplateError(
        "card_layout_invalid",
        `X/Twitter MP4 template layout geçersiz: ${JSON.stringify(layout)}`
      );
    }

    stage = "card_png_render_failed";
    debugSnapshot.stage = stage;
    const dataUrl = await renderTwitterPostCardCanvas(root, rootRect, layout, templateValues);
    debugSnapshot.dataUrlLength = dataUrl.length;

    if (
      !/^data:image\/png;base64,/i.test(dataUrl) ||
      dataUrl.length <= "data:image/png;base64,".length
    ) {
      throw twitterPostTemplateError(
        "card_png_render_failed",
        "X/Twitter MP4 template PNG çıktısı geçersiz."
      );
    }

    return {
      dataUrl,
      layout,
    };
  } catch (error) {
    const finalError = ensureTwitterPostTemplateError(
      error,
      stage,
      "X/Twitter MP4 template render başarısız",
      {
        layoutSnapshot: debugSnapshot.layout,
        renderDebug: debugSnapshot,
      }
    );
    debugSnapshot.stage = finalError.stage || finalError.debugCode || stage;

    console.error("Twitter post MP4 card render failed:", {
      stage: debugSnapshot.stage,
      debugCode: finalError.debugCode || debugSnapshot.stage,
      message: finalError.message,
      cause: finalError.cause,
      error: finalError,
      snapshot: debugSnapshot,
    });

    throw finalError;
  } finally {
    host?.remove();
  }
}

function pngDataUrlToBase64(dataUrl) {
  const clean = metadataString(dataUrl);
  const prefix = "data:image/png;base64,";

  if (clean.toLowerCase().startsWith(prefix)) {
    return clean.slice(prefix.length);
  }

  return clean;
}

async function hydrateTwitterAvatarDataUrl(metadata = {}) {
  const avatarDataUrl =
    safeTwitterAvatarDataUrl(metadata.avatarDataUrl) ||
    safeTwitterAvatarDataUrl(metadata.avatarUrl);
  if (avatarDataUrl) return { ...metadata, avatarDataUrl };

  const avatarUrl = normalizeTwitterImageUrl(metadata.avatarUrl);
  if (typeof invoke !== "function") {
    return metadata;
  }

  const handle =
    normalizeTwitterHandle(metadata.authorHandle).replace(/^@/, "") ||
    normalizeTwitterHandle(twitterHandleFromUrl(metadata.webpageUrl)).replace(/^@/, "");

  if (avatarUrl && isRemoteImageCandidate(avatarUrl)) {
    console.debug("Twitter avatar candidate selected", {
      host: urlHost(avatarUrl),
    });

    try {
      const dataUrl = await invoke("cache_twitter_avatar", { url: avatarUrl });
      const safeDataUrl = safeTwitterAvatarDataUrl(dataUrl);

      if (safeDataUrl) {
        return { ...metadata, avatarUrl, avatarDataUrl: safeDataUrl };
      }
    } catch (error) {
      console.debug("Twitter avatar fallback skipped:", {
        stage: "cache_twitter_avatar",
        host: urlHost(avatarUrl),
        error,
      });
    }
  } else {
    console.debug("Twitter avatar metadata candidate missing", {
      handle: handle || "",
    });
  }

  if (handle) {
    try {
      const dataUrl = await invoke("resolve_twitter_avatar_by_handle", { handle });
      const safeDataUrl = safeTwitterAvatarDataUrl(dataUrl);

      if (safeDataUrl) {
        return { ...metadata, avatarDataUrl: safeDataUrl };
      }
    } catch (error) {
      console.debug("Twitter avatar handle resolution skipped:", {
        stage: "resolve_twitter_avatar_by_handle",
        handle,
        error,
      });
    }
  }

  return metadata;
}


export {
  drawContainedImage,
  ensureTwitterPostTemplateError,
  hydrateTwitterAvatarDataUrl,
  imageAspectRatioFromItem,
  isTaintedCanvasError,
  isValidTwitterPostCardLayout,
  pngDataUrlToBase64,
  renderTwitterPostCardPng,
  renderTwitterPhotoPostCardPng,
  renderTwitterTextPostCardPng,
  roundedRectPath,
  safeRasterImageDataUrl,
  strokeRoundedRect,
  twitterPostErrorCode,
  twitterScreenshotMetaText,
  twitterPostTemplateValues,
  twitterPostTemplateError,
  wrapFullCanvasText,
};
