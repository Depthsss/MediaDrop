import { convertFileSrc, invoke, listen } from "./tauri.js";

import {
  hydrateTwitterAvatarDataUrl,
  isValidTwitterPostCardLayout,
  pngDataUrlToBase64,
  renderTwitterPhotoPostCardPng as renderTwitterPhotoCardPng,
  renderTwitterPostCardPng,
  renderTwitterTextPostCardPng as renderTwitterTextCardPng,
  safeRasterImageDataUrl,
  twitterPostErrorCode,
  twitterPostTemplateError,
} from "../features/twitter-card/renderer.js";
import { startCompanionRenderer } from "../features/twitter-card/companion-renderer.js";
import {
  checkForUpdate,
  openDialog,
  readClipboardText,
  relaunch,
  writeClipboardText,
} from "./tauri.js";
import { createAppStore } from "./store.js";

import { parseBackendError } from "./errors.js";
import {
  clipPreviewStreamSources,
  clipPreviewAttemptBudgetMs,
  isClipPreviewBuildActive,
  mediaItemKindLabel,
  mediaItemHasPreview,
  mediaItemType,
  mediaAnalysisAuthorIdentity,
  mediaCardDescription,
  instagramAnalysisNeedsAvatarAuth,
  mediaAnalysisWarningMessages,
  mediaPreviewPolicy,
  nativeClipAudioSyncTarget,
  nativeClipPlayerState,
  isMediaAnalysisExpired,
  normalizeMediaAnalysis,
  normalizeMediaAnalysisItems,
  normalizeMediaPreviewResponse,
  normalizeRasterImageSource,
  reusableMediaPreviewValue,
  selectMediaPreviewPrefetchItems,
  shouldTryMediaInventory,
  supportsLegacyVideoFallback,
  twitterMediaPostDownloadKind,
} from "../features/preview/media-model.js";
import { loadRasterImageSource } from "../features/preview/raster-loader.js";
import {
  consumeInstagramAuthPromptBudget,
  executeInstagramDownloadWithRecovery,
  instagramInitialAuthMode,
  nextInstagramAuthRecoveryStep,
} from "../features/instagram/auth-policy.js";
import {
  createInstagramAuthController,
  PUBLIC_MEDIA_AUTH_MODE,
  SAVED_INSTAGRAM_AUTH_MODE,
} from "../features/instagram/auth-controller.js";
import { createModalController } from "../features/accessibility/modal-controller.js";
import {
  extensionGuideForBrowser,
  extensionSetupStepStates,
  orderExtensionBrowsers,
} from "../features/extension-setup/setup-model.js";
import {
  asNumber,
  clipDownloadStatusText,
  displayProgressPercent,
  downloadCancellationCompletion,
  parseFallbackProgressLine,
  progressJobId,
} from "../features/downloads/progress-model.js";
import {
  mediaDownloadOutcome,
  mediaDownloadTarget,
  normalizeMediaDownloadResult,
} from "../features/downloads/media-result.js";
import {
  buildMediaBatchDownloadRequest,
  buildMediaItemDownloadRequest,
  buildOptionalMediaRegistryTarget,
} from "../features/downloads/media-request.js";
import {
  basename,
  historyTimeMs,
  normalizePath,
  prependDownloadHistoryItem,
  readDownloadHistory,
  removeDownloadHistoryItem,
  writeDownloadHistory,
} from "../features/downloads/history-store.js";
import {
  cleanTwitterPostText,
  formatMediaDuration,
  formatTwitterDisplayDate,
  formatTwitterActionCount,
  formatTwitterCompactCount,
  getThumbnailCandidates,
  isRemoteImageCandidate,
  isTwitterPostDownloadIntent,
  metadataString,
  normalizeTwitterHandle,
  normalizeTwitterImageUrl,
  normalizeTwitterPostMetadata as buildTwitterPostMetadata,
  normalizeTwitterQuoteContext,
  safeTwitterAvatarDataUrl,
  twitterAvatarInitial,
  twitterAvatarMetadataDebugInfo,
  twitterHandleFromUrl,
  twitterTextPostAvailable,
} from "../features/twitter-card/metadata.js";
import {
  createWindowLayoutCoordinator,
  measureWindowContentHeight,
  WINDOW_HEIGHT_PRESETS,
} from "../features/window-layout.js";
import {
  buildFormatCards,
  qualityHeightFromLabel,
  SOCIAL_COMPATIBLE_FORMAT_ID,
} from "../features/quality/format-model.js";
import { clipboardAutofillValue } from "../features/clipboard-autofill.js";

const CLIP_PREVIEW_TOTAL_BUDGET_MS = 20_000;
const CLIP_PREVIEW_NATIVE_BUDGET_MS = 4_000;

const urlInput = document.querySelector("#urlInput");
const urlClearBtn = document.querySelector("#urlClearBtn");
const analyzeBtn = document.querySelector("#analyzeBtn");
const updateBtn = document.querySelector("#updateBtn");
const folderBtn = document.querySelector("#folderBtn");
const folderLabel = document.querySelector("#folderLabel");
const cloudReportsToggle = document.querySelector("#cloudReportsToggle");
const appVersionBadge = document.querySelector("#appVersionBadge");
const clipBtn = document.querySelector("#clipBtn");
const postDownloadBtn = document.querySelector("#postDownloadBtn");
const clipStatusBar = document.querySelector("#clipStatusBar");
const clipSummary = document.querySelector("#clipSummary");
const clipEditInlineBtn = document.querySelector("#clipEditInlineBtn");
const clipCancelInlineBtn = document.querySelector("#clipCancelInlineBtn");
const message = document.querySelector("#message");
const formatList = document.querySelector("#formatList");
const selectedFormat = document.querySelector("#selectedFormat");
const downloadBtn = document.querySelector("#downloadBtn");
const cancelBtn = document.querySelector("#cancelBtn");
const historyBtn = document.querySelector("#historyBtn");
const platformBadge = document.querySelector("#platformBadge");
const revealLastBtn = document.querySelector("#revealLastBtn");
const historyPanel = document.querySelector("#historyPanel");
const historyList = document.querySelector("#historyList");
const clearHistoryBtn = document.querySelector("#clearHistoryBtn");
const closeHistoryBtn = document.querySelector("#closeHistoryBtn");
const extensionSetupBtn = document.querySelector("#extensionSetupBtn");
const extensionSetupOverlay = document.querySelector("#extensionSetupOverlay");
const extensionSetupCloseBtn = document.querySelector("#extensionSetupCloseBtn");
const extensionSetupLaterBtn = document.querySelector("#extensionSetupLaterBtn");
const extensionSetupOpenBtn = document.querySelector("#extensionSetupOpenBtn");
const extensionCopyPathBtn = document.querySelector("#extensionCopyPathBtn");
const extensionRevealPathBtn = document.querySelector("#extensionRevealPathBtn");
const extensionSetupPath = document.querySelector("#extensionSetupPath");
const extensionSetupMessage = document.querySelector("#extensionSetupMessage");
const extensionBrowserList = document.querySelector("#extensionBrowserList");
const extensionBrowserHint = document.querySelector("#extensionBrowserHint");
const extensionConnectionStatus = document.querySelector("#extensionConnectionStatus");
const extensionSetupSteps = document.querySelector("#extensionSetupSteps");
const toolsOverlay = document.querySelector("#toolsOverlay");
const toolsStatus = document.querySelector("#toolsStatus");
const cookieAuthOverlay = document.querySelector("#cookieAuthOverlay");
const cookieAuthTitle = document.querySelector("#cookieAuthTitle");
const cookieAuthStatus = document.querySelector("#cookieAuthStatus");
const cookieBrowserSelect = document.querySelector("#cookieBrowserSelect");
const cookieBrowserSelectedName = document.querySelector("#cookieBrowserSelectedName");
const cookieBrowserSelectedDetail = document.querySelector("#cookieBrowserSelectedDetail");
const cookieBrowserList = document.querySelector("#cookieBrowserList");
const cookieRememberCheck = document.querySelector("#cookieRememberCheck");
const cookieAllowBtn = document.querySelector("#cookieAllowBtn");
const cookieDenyBtn = document.querySelector("#cookieDenyBtn");
const browserRestartOverlay = document.querySelector("#browserRestartOverlay");
const browserRestartTitle = document.querySelector("#browserRestartTitle");
const browserRestartStatus = document.querySelector("#browserRestartStatus");
const browserRestartAllowBtn = document.querySelector("#browserRestartAllowBtn");
const browserRestartDenyBtn = document.querySelector("#browserRestartDenyBtn");

const progressWrap = document.querySelector("#progressWrap");
const progressFill = document.querySelector("#progressFill");
const progressText = document.querySelector("#progressText");
const progressLine = document.querySelector("#progressLine");

const titlebar = document.querySelector(".titlebar");
const minimizeBtn = document.querySelector("#minimizeBtn");
const closeBtn = document.querySelector("#closeBtn");

const videoPreview = document.querySelector("#videoPreview");
const videoThumb = document.querySelector("#videoThumb");
const videoTitle = document.querySelector("#videoTitle");
const videoMeta = document.querySelector("#videoMeta");
const mediaPreview = document.querySelector("#mediaPreview");
const mediaPreviewLabel = document.querySelector("#mediaPreviewLabel");
const mediaPreviewTitle = document.querySelector("#mediaPreviewTitle");
const mediaPreviewMeta = document.querySelector("#mediaPreviewMeta");
const mediaPreviewBadge = document.querySelector("#mediaPreviewBadge");
const mediaPreviewImage = document.querySelector("#mediaPreviewImage");
const mediaPreviewVideo = document.querySelector("#mediaPreviewVideo");
const mediaStage = document.querySelector(".media-stage");
const mediaFrame = document.querySelector(".media-frame");
const mediaTweetCard = document.querySelector("#mediaTweetCard");
const mediaTweetAvatar = document.querySelector("#mediaTweetAvatar");
const mediaTweetAvatarInitial = document.querySelector("#mediaTweetAvatarInitial");
const mediaTweetName = document.querySelector("#mediaTweetName");
const mediaTweetHandle = document.querySelector("#mediaTweetHandle");
const mediaTweetMeta = document.querySelector("#mediaTweetMeta");
const mediaTweetText = document.querySelector("#mediaTweetText");
const mediaQuotedTweetCard = document.querySelector("#mediaQuotedTweetCard");
const mediaQuotedTweetAvatar = document.querySelector("#mediaQuotedTweetAvatar");
const mediaQuotedTweetAvatarInitial = document.querySelector("#mediaQuotedTweetAvatarInitial");
const mediaQuotedTweetName = document.querySelector("#mediaQuotedTweetName");
const mediaQuotedTweetHandle = document.querySelector("#mediaQuotedTweetHandle");
const mediaQuotedTweetMeta = document.querySelector("#mediaQuotedTweetMeta");
const mediaQuotedTweetText = document.querySelector("#mediaQuotedTweetText");
const mediaQuotedFrame = document.querySelector("#mediaQuotedFrame");
const mediaQuotedPreviewImage = document.querySelector("#mediaQuotedPreviewImage");
const mediaQuotedPreviewVideo = document.querySelector("#mediaQuotedPreviewVideo");
const mediaPrevBtn = document.querySelector("#mediaPrevBtn");
const mediaNextBtn = document.querySelector("#mediaNextBtn");
const mediaQuotePrevBtn = document.querySelector("#mediaQuotePrevBtn");
const mediaQuoteNextBtn = document.querySelector("#mediaQuoteNextBtn");
const downloadMediaPostBtn = document.querySelector("#downloadMediaPostBtn");
const downloadMediaItemBtn = document.querySelector("#downloadMediaItemBtn");
const downloadMediaBatchBtn = document.querySelector("#downloadMediaBatchBtn");
const qualityCard = document.querySelector("#qualityCard");
const qualityCardValue = document.querySelector("#qualityCardValue");
const qualityCardDetail = document.querySelector("#qualityCardDetail");
const qualityPicker = document.querySelector("#qualityPicker");
const qualityCloseBtn = document.querySelector("#qualityCloseBtn");
const qualityPrevBtn = document.querySelector("#qualityPrevBtn");
const qualityNextBtn = document.querySelector("#qualityNextBtn");
const qualitySelectBtn = document.querySelector("#qualitySelectBtn");
const qualityFocusIndex = document.querySelector("#qualityFocusIndex");
const qualityFocusValue = document.querySelector("#qualityFocusValue");
const qualityFocusDetail = document.querySelector("#qualityFocusDetail");
const qualityFocusSize = document.querySelector("#qualityFocusSize");
const downloadPanel = document.querySelector(".download-panel");
const clipEditor = document.querySelector("#clipEditor");
const clipBackBtn = document.querySelector("#clipBackBtn");
const clipClearBtn = document.querySelector("#clipClearBtn");
const clipPlayerMount = document.querySelector("#clipPlayerMount");
const clipSeek = document.querySelector("#clipSeek");
const clipCurrentTime = document.querySelector("#clipCurrentTime");
const clipDurationLabel = document.querySelector("#clipDurationLabel");
const clipSelectedRange = document.querySelector("#clipSelectedRange");
const clipStartMarker = document.querySelector("#clipStartMarker");
const clipEndMarker = document.querySelector("#clipEndMarker");
const clipStartMin = document.querySelector("#clipStartMin");
const clipStartSec = document.querySelector("#clipStartSec");
const clipEndMin = document.querySelector("#clipEndMin");
const clipEndSec = document.querySelector("#clipEndSec");
const clipMessage = document.querySelector("#clipMessage");
const clipSetStartBtn = document.querySelector("#clipSetStartBtn");
const clipSetEndBtn = document.querySelector("#clipSetEndBtn");
const clipPreviewBtn = document.querySelector("#clipPreviewBtn");
const clipDoneBtn = document.querySelector("#clipDoneBtn");
const clipPlayBtn = document.querySelector("#clipPlayBtn");
const clipVolumeSlider = document.querySelector("#clipVolumeSlider");
const clipVolumeValue = document.querySelector("#clipVolumeValue");

const DOWNLOAD_DIR_KEY = "mediadrop_download_dir";
const HISTORY_KEY = "mediadrop_download_history";
const TOOLS_UPDATE_CHECK_KEY = "mediadrop_tools_update_last_check";
const TOOLS_UPDATE_INTERVAL_MS = 12 * 60 * 60 * 1000;
const CLIP_VOLUME_KEY = "mediadrop_clip_volume";
const WINDOW_POSITIONS_KEY = "mediadrop_window_positions";

function formatAppVersionLabel(version) {
  const clean = String(version || "").trim().replace(/^v/i, "");
  return clean ? `v${clean}` : "";
}

async function updateAppVersionBadge() {
  if (!appVersionBadge) return;

  const fallback =
    formatAppVersionLabel(appVersionBadge.dataset.fallback) ||
    formatAppVersionLabel(appVersionBadge.textContent);

  if (fallback) {
    appVersionBadge.textContent = fallback;
  }

  try {
    const version = await invoke("get_app_version");
    const label = formatAppVersionLabel(version);

    if (label) {
      appVersionBadge.textContent = label;
    }
  } catch (error) {
    console.warn("App version could not be read:", error);
  }
}

// ── Hata mesajı yardımcıları ──────────────────────────────────────────────────

/**
 * Hata mesajını message div'inde gösterir.
 * Backend bir rapor dosyası oluşturduysa, ham hata yerine
 * tıklanabilir "Hata alındı — Hata raporu için tıklayın" linki gösterir.
 * Tıklanınca Rust tarafı en son rapor dosyasını Explorer'da seçili açar.
 */
function showErrorMessage(error) {
  const parsed = parseBackendError(error);
  const text = parsed.message;
  const hasReport = Boolean(parsed.reportId);
  const summary = text.split("\n")[0].slice(0, 140);

  // Önceki içeriği temizle (önceki tıklama listener'ları dahil)
  message.replaceChildren();

  if (hasReport) {
    if (summary) {
      const summaryNode = document.createElement("span");
      summaryNode.textContent = `${summary} `;
      message.appendChild(summaryNode);
    }

    const link = document.createElement("span");
    link.className = "message-error-link";
    link.textContent = "⚠ Hata alındı — Hata raporu için tıklayın";
    link.title = "Hata Raporları klasörünü açmak için tıklayın";
    link.addEventListener("click", () => {
      invoke("reveal_last_error_report").catch(console.error);
    });
    message.appendChild(link);
  } else {
    // Rapor dosyası yoksa hatanın ilk satırını kısaltılmış göster
    message.textContent = summary;
  }

  message.className = "message is-error";
  return parsed;
}

// ─────────────────────────────────────────────────────────────────────────────

function buildFallbackDownloadArgs(args, fallbackOffer) {
  if (!args || !fallbackOffer || fallbackOffer.kind !== "hls_1080") return null;

  if (args.clipStartSeconds === null || args.clipEndSeconds === null) return null;

  return {
    ...args,
    quality: fallbackOffer.quality || "1080p",
  };
}

let activeFormat = null;
let currentUrl = "";
let currentInfo = null;
let currentMediaAnalysis = null;
let currentMediaItems = [];
let currentMediaIndex = 0;
let currentMediaAuthMode = null;
let currentPlatform = "generic";
let currentTwitterPostMetadata = null;
let currentTwitterTextOnly = false;
let mediaPreviewCache = new Map();
let selectedOutputDir = localStorage.getItem(DOWNLOAD_DIR_KEY) || "";
let previewToken = 0;
let lastCompletedFilePath = "";
let lastCompletedItem = null;
let clipSelection = null;
let clipDraft = null;
let clipPlayer = null;
let clipPlayerReady = false;
let clipTicker = null;
let cancelPendingClipPlayer = null;
let clipPreviewMode = false;
let youtubeApiPromise = null;
let clipPlayerState = "idle"; // idle | playing | paused | buffering
let clipPlayerMode = "none"; // none | native | iframe
let clipFallbackVideoId = "";
let clipPreviewCache = {
  key: "",
  videoId: "",
  formatSignature: "",
  ready: false,
  loading: false,
  mode: "none",
};
let clipPlayerBuildToken = 0;
let availableFormats = [];
let qualityPickerIndex = 0;
let qualityPickerDraftIndex = 0;
let twitterPostProgressFloor = 0;
let windowPositionTimer = null;
let currentWindowLayoutMode = "main";
let windowLayoutCoordinator = null;
let extensionSetupInfo = null;
let selectedExtensionBrowserId = "";
let extensionSetupPollTimer = null;
const openedExtensionBrowserIds = new Set();

let downloadState = "idle"; // idle | downloading | pausing | paused | canceling
let activeDownloadJobId = "";
let lastDownloadArgs = null;
let lastMediaDownloadArgs = null;
let analysisToken = 0;
let lastClipboardText = "";
const appStore = createAppStore();
appStore.subscribe((state) => {
  currentMediaAnalysis = state.analysis.mediaAnalysis;
  currentMediaItems = state.analysis.items;
  currentMediaIndex = state.analysis.index;
  currentPlatform = state.analysis.platform;
  currentWindowLayoutMode = state.window.mode;
  downloadState = state.download.status;
  activeDownloadJobId = state.download.jobId;
});

const SUPPORTED_LINK_MESSAGE =
  "Sadece YouTube, Instagram, X/Twitter ve TikTok linkleri destekleniyor.";

const modalController = createModalController({ documentRef: document });
void startCompanionRenderer().catch(() => {});

const instagramAuthController = createInstagramAuthController({
  invoke,
  storage: localStorage,
  documentRef: document,
  elements: {
    cookieAuthOverlay,
    cookieAuthTitle,
    cookieAuthStatus,
    cookieBrowserSelect,
    cookieBrowserSelectedName,
    cookieBrowserSelectedDetail,
    cookieBrowserList,
    cookieRememberCheck,
    cookieAllowBtn,
    cookieDenyBtn,
    browserRestartOverlay,
    browserRestartTitle,
    browserRestartStatus,
    browserRestartAllowBtn,
    browserRestartDenyBtn,
  },
  parseBackendError,
  logger: console,
  publicMode: PUBLIC_MEDIA_AUTH_MODE,
  savedMode: SAVED_INSTAGRAM_AUTH_MODE,
  modalController,
});

const {
  browserIdFromInstagramAuthMode,
  clearInstagramCookieConsent,
  confirmPendingInstagramCookieConsent,
  instagramCookieAuthMode,
  isInstagramAuthRecoverableError,
  isInstagramStoryUrl,
  mediaAuthModeAfterSuccessfulAnalysis,
  mediaAuthModeForUrl,
  prepareInstagramCookieAuthFromPermission,
  savedInstagramCookieConsent,
} = instagramAuthController;

currentMediaAuthMode = PUBLIC_MEDIA_AUTH_MODE;

function urlHost(value) {
  let clean = String(value || "").trim();
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

function hostMatches(host, domain) {
  return host === domain || host.endsWith(`.${domain}`);
}

function detectPlatform(url, info = {}) {
  const safeInfo = info && typeof info === "object" ? info : {};
  const hosts = [url, safeInfo.webpage_url, safeInfo.original_url]
    .filter(Boolean)
    .map(urlHost)
    .filter(Boolean);
  const extractor = [safeInfo.extractor, safeInfo.extractor_key]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();

  if (
    hosts.some((host) => hostMatches(host, "youtube.com") || host === "youtu.be") ||
    extractor.includes("youtube")
  ) {
    return "youtube";
  }

  if (
    hosts.some((host) => hostMatches(host, "twitter.com") || hostMatches(host, "x.com") || host === "t.co") ||
    extractor.includes("twitter")
  ) {
    return "twitter";
  }

  if (
    hosts.some((host) => hostMatches(host, "instagram.com") || hostMatches(host, "instagr.am")) ||
    extractor.includes("instagram")
  ) {
    return "instagram";
  }

  if (
    hosts.some((host) => hostMatches(host, "tiktok.com")) ||
    extractor.includes("tiktok")
  ) {
    return "tiktok";
  }

  return "generic";
}

function isSupportedMediaLink(value) {
  return detectPlatform(value) !== "generic";
}

function updateUrlClearButton() {
  if (urlClearBtn) urlClearBtn.hidden = !urlInput.value.trim();
}

async function autofillUrlFromClipboard() {
  try {
    const clipboardText = await readClipboardText();
    const value = clipboardAutofillValue({
      clipboardText,
      inputValue: urlInput.value,
      lastClipboardText,
      isSupported: isSupportedMediaLink,
    });
    lastClipboardText = String(clipboardText || "").trim();
    if (!value) return;

    urlInput.value = value;
    updateUrlClearButton();
  } catch (error) {
    console.warn("Clipboard could not be read:", error);
  }
}

function normalizeTwitterPostMetadata(info = {}, fallbackUrl = "") {
  const metadata = buildTwitterPostMetadata(info, fallbackUrl);

  console.debug(
    "Twitter avatar metadata scan",
    twitterAvatarMetadataDebugInfo(info, metadata.avatarUrl)
  );

  return metadata;
}

function bytesToMb(bytes) {
  const number = Number(bytes);
  if (!Number.isFinite(number) || number <= 0) return null;
  return number / 1024 / 1024;
}

function shortPath(path) {
  if (!path) return "Varsayılan: Downloads\\MediaDrop";
  if (path.length <= 74) return path;
  return `...${path.slice(-71)}`;
}

function updateFolderLabel() {
  if (!folderLabel) return;

  folderLabel.textContent = `Kayıt klasörü: ${shortPath(selectedOutputDir)}`;
  folderLabel.title = selectedOutputDir || "Varsayılan: Downloads\\MediaDrop";
  folderLabel.classList.toggle("is-custom", Boolean(selectedOutputDir));
}

async function updateCloudReportsFromBackend() {
  if (!cloudReportsToggle) return;

  try {
    const enabled = await invoke("get_cloud_reports_enabled");
    cloudReportsToggle.checked = enabled === true;
  } catch (error) {
    console.warn("Cloud report setting could not be read:", error);
    cloudReportsToggle.checked = false;
  }
}

async function setCloudReportsEnabled(enabled) {
  if (!cloudReportsToggle) return;

  cloudReportsToggle.checked = Boolean(enabled);

  try {
    await invoke("set_cloud_reports_enabled", { enabled: Boolean(enabled) });

    if (enabled) {
      const sent = await invoke("flush_pending_cloud_reports").catch(() => 0);
      message.textContent = sent > 0
        ? `Tanılama raporları açıldı. ${sent} bekleyen rapor gönderildi.`
        : "Tanılama raporları açıldı.";
    } else {
      message.textContent = "Tanılama raporları kapatıldı. Hatalar sadece bu bilgisayarda saklanacak.";
    }

    message.className = "message";
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
    await updateCloudReportsFromBackend();
  }
}

async function flushPendingCloudReportsOnStartup() {
  try {
    await invoke("flush_pending_cloud_reports");
  } catch (error) {
    console.warn("Pending cloud reports could not be flushed:", error);
  }
}

// Presets are fallbacks only. The final height is measured from the visible
// content so previews, progress details and the clip editor cannot be clipped.
const MAIN_WINDOW_HEIGHT = WINDOW_HEIGHT_PRESETS.main;
const MEDIA_WINDOW_HEIGHT = WINDOW_HEIGHT_PRESETS.media;
const CLIP_WINDOW_HEIGHT = WINDOW_HEIGHT_PRESETS.clip;

function readWindowPositions() {
  try {
    const positions = JSON.parse(localStorage.getItem(WINDOW_POSITIONS_KEY) || "{}");
    return positions && typeof positions === "object" ? positions : {};
  } catch {
    return {};
  }
}

function writeWindowPositions(positions) {
  localStorage.setItem(WINDOW_POSITIONS_KEY, JSON.stringify(positions || {}));
}

function normalizeWindowPosition(position) {
  const x = Number(position?.x);
  const y = Number(position?.y);

  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;

  return {
    x: Math.round(x),
    y: Math.round(y),
  };
}

async function saveWindowPosition(mode = currentWindowLayoutMode) {
  if (!mode) return;

  try {
    const position = normalizeWindowPosition(await invoke("get_window_position"));
    if (!position) return;

    const positions = readWindowPositions();
    positions[mode] = position;
    writeWindowPositions(positions);
  } catch (error) {
    console.warn("Window position save failed:", error);
  }
}

function saveWindowPositionSoon(mode = currentWindowLayoutMode, delay = 160) {
  clearTimeout(windowPositionTimer);
  windowPositionTimer = setTimeout(() => {
    saveWindowPosition(mode);
  }, delay);
}

async function restoreWindowPosition(mode) {
  const position = normalizeWindowPosition(readWindowPositions()[mode]);
  if (!position) return;

  try {
    const safePosition = normalizeWindowPosition(await invoke("set_window_position", position));

    if (safePosition) {
      const positions = readWindowPositions();
      positions[mode] = safePosition;
      writeWindowPositions(positions);
    }
  } catch (error) {
    console.warn("Window position restore failed:", error);
  }
}

function sampleWindowPositionAfterDrag(mode = currentWindowLayoutMode) {
  saveWindowPositionSoon(mode, 180);
  [700, 1500, 2600].forEach((delay) => {
    setTimeout(() => saveWindowPosition(mode), delay);
  });
}

function visibleWindowContent() {
  if (document.body.classList.contains("clip-editor-open")) {
    return clipEditor?.querySelector(".clip-editor-card") || clipEditor;
  }

  return document.querySelector(".console");
}

function measureDesiredWindowHeight(preset = currentWindowLayoutMode) {
  const content = visibleWindowContent();
  const fallback = preset === "clip"
    ? CLIP_WINDOW_HEIGHT
    : preset === "media"
      ? MEDIA_WINDOW_HEIGHT
      : MAIN_WINDOW_HEIGHT;

  if (!content) return fallback;

  const rect = content.getBoundingClientRect();
  const outerPadding = preset === "clip" ? 28 : 20;
  return measureWindowContentHeight({
    scrollHeight: content.scrollHeight,
    rectHeight: rect.height,
    outerPadding,
    fallbackHeight: fallback,
  });
}

function ensureWindowLayoutCoordinator() {
  if (windowLayoutCoordinator) return windowLayoutCoordinator;

  windowLayoutCoordinator = createWindowLayoutCoordinator({
    initialMode: currentWindowLayoutMode,
    debounceMs: 55,
    manualResizeMode: "suspend",
    measureHeight: (mode, fallbackHeight) =>
      measureDesiredWindowHeight(mode) || fallbackHeight || MAIN_WINDOW_HEIGHT,
    requestHeight: (height) => invoke("resize_window_height", { height }),
    onError: (error) => console.warn("Window resize failed:", error),
  });
  return windowLayoutCoordinator;
}

function setWindowHeightPreset(preset = currentWindowLayoutMode, fallbackHeight = null) {
  return ensureWindowLayoutCoordinator().schedule({
    mode: preset,
    fallbackHeight,
  });
}

function setWindowLayoutMode(mode, fallbackHeight) {
  const previousMode = currentWindowLayoutMode;
  const modeChanged = previousMode !== mode;
  appStore.dispatch({ type: "window/mode", mode });

  if (modeChanged) {
    saveWindowPosition(previousMode).finally(() => {
      ensureWindowLayoutCoordinator().setMode(mode, fallbackHeight);
      setTimeout(() => restoreWindowPosition(mode), 140);
    });
    return;
  }

  ensureWindowLayoutCoordinator().setMode(mode, fallbackHeight);
}

function setMainWindowSize() {
  setWindowLayoutMode("main", MAIN_WINDOW_HEIGHT);
}

function setMediaWindowSize() {
  setWindowLayoutMode("media", MEDIA_WINDOW_HEIGHT);
}

function setClipWindowSize() {
  setWindowLayoutMode("clip", CLIP_WINDOW_HEIGHT);
}

function initializeWindowPlacement() {
  appStore.dispatch({ type: "window/mode", mode: "main" });
  const coordinator = ensureWindowLayoutCoordinator();
  coordinator.resumeAutoResize();
  coordinator.schedule({ mode: "main", fallbackHeight: MAIN_WINDOW_HEIGHT, force: true });
  setTimeout(() => restoreWindowPosition("main"), 160);
}

function fallbackWindowResize(_width, _height) {
  return ensureWindowLayoutCoordinator().resizeNow(_height, { force: true });
}

function refreshWindowLayout() {
  return ensureWindowLayoutCoordinator().refresh();
}

function initializeDynamicWindowLayout() {
  ensureWindowLayoutCoordinator().attach({
    windowTarget: window,
    ResizeObserverClass: window.ResizeObserver,
    observedElements: [
      document.querySelector(".console"),
      clipEditor?.querySelector(".clip-editor-card"),
    ],
    fontsReady: document.fonts?.ready,
  });
}



function getVideoDurationSeconds() {
  const duration = Number(currentInfo?.duration || 0);
  return Number.isFinite(duration) && duration > 0 ? duration : 0;
}

function clampNumber(value, min, max) {
  const number = Number(value);
  if (!Number.isFinite(number)) return min;
  return Math.max(min, Math.min(max, number));
}

function formatClipTime(totalSeconds) {
  const safe = Math.max(0, Number(totalSeconds) || 0);
  const rounded = Math.floor(safe);
  const hours = Math.floor(rounded / 3600);
  const minutes = Math.floor((rounded % 3600) / 60);
  const seconds = Math.floor(rounded % 60);

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }

  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatClipFilePart(totalSeconds) {
  return formatClipTime(totalSeconds).replace(/:/g, "-");
}

function clipDurationLabelText(clip) {
  if (!clip) return "";
  return `${formatClipTime(clip.start)} → ${formatClipTime(clip.end)}`;
}

function isClipCompatibleFormat(format = activeFormat) {
  return Boolean(format && currentPlatform === "youtube" && format.type === "video");
}

function isClipFeatureAvailable() {
  return Boolean(isClipCompatibleFormat() && currentInfo && getVideoDurationSeconds() > 0);
}

function isDownloadableVideoFormat(format) {
  return Boolean(
    format &&
      format.vcodec &&
      format.vcodec !== "none" &&
      (format.ext === "mp4" || format.protocol || format.url)
  );
}

function hasSocialDownloadTarget(info = currentInfo, format = activeFormat) {
  if (!info || !format) return false;

  const formats = Array.isArray(info.formats) ? info.formats : [];
  return formats.some(isDownloadableVideoFormat) || isDownloadableVideoFormat(format.raw);
}

function hasTwitterPostDownloadTarget(info = currentInfo, format = activeFormat) {
  if (twitterMediaPostDownloadKind(currentPlatform, selectedMediaItem()) === "video") return true;
  if (currentPlatform !== "twitter" || !info || !format) return false;
  if (format.type !== "twitter" && format.type !== "video") return false;

  return hasSocialDownloadTarget(info, format);
}

function canDownloadActiveFormat() {
  if (isMediaPhotoMode()) {
    return Boolean(selectedMediaItem());
  }

  if (!activeFormat) return false;

  if (currentPlatform === "twitter") {
    return hasTwitterPostDownloadTarget(currentInfo, activeFormat);
  }

  if (currentPlatform === "instagram" || currentPlatform === "tiktok") {
    return hasSocialDownloadTarget(currentInfo, activeFormat);
  }

  return currentPlatform === "youtube";
}

function updatePrimaryDownloadButtonLabel() {
  if (!downloadBtn || downloadState !== "idle") return;
  if (isMediaPhotoMode()) {
    downloadBtn.textContent = selectedMediaItem()?.isStory ? "Hikayeyi İndir" : "Fotoğrafı İndir";
    return;
  }
  downloadBtn.textContent = currentPlatform === "twitter" ? "Videoyu İndir" : "İndir";
}

function updateTwitterPostControls() {
  if (!postDownloadBtn) return;

  const show = currentPlatform === "twitter" && !isMediaPhotoMode() && !isTwitterTextPostMode();
  postDownloadBtn.classList.toggle("is-hidden", !show);

  if (!show) {
    postDownloadBtn.disabled = true;
    postDownloadBtn.title = "";
    return;
  }

  const available = hasTwitterPostDownloadTarget();
  postDownloadBtn.disabled = downloadState !== "idle" || !available;
  postDownloadBtn.title = available
    ? "Gönderiyi MP4 video olarak indir"
    : "Bu X/Twitter gönderisinde indirilebilir video bulunamadı.";
}

function twitterPostDownloadArgs(cardPngBase64 = "", cardLayout = null, cardOverlayPngBase64 = "") {
  const metadata =
    currentTwitterPostMetadata || normalizeTwitterPostMetadata(currentInfo || {}, currentUrl);
  const quoteContext = normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote);
  const registryTarget = buildOptionalMediaRegistryTarget({
    analysisId: quoteContext ? mediaAnalysisId() : null,
    itemId: quoteContext ? selectedMediaItem()?.id : null,
  });

  return {
    url: currentUrl,
    formatId: activeFormat?.id || SOCIAL_COMPATIBLE_FORMAT_ID,
    quality: activeFormat?.quality || "Otomatik",
    outputDir: selectedOutputDir || null,
    title: currentInfo?.title || metadata.text || null,
    postText: metadata.text || "Gönderi metni alınamadı.",
    authorName: metadata.authorName || "X/Twitter",
    authorHandle: metadata.authorHandle || "",
    displayDate: metadata.displayDate || "",
    webpageUrl: metadata.webpageUrl || currentUrl,
    cardPngBase64,
    cardOverlayPngBase64,
    cardLayout,
    ...registryTarget,
  };
}

function showTwitterPostSuccessMessage(result) {
  const targetPath = normalizePath(result?.filePath || result?.outputDir || "");

  message.replaceChildren();

  if (!targetPath) {
    message.textContent = "X/Twitter gönderi videosu hazırlandı.";
    message.className = "message is-success";
    return;
  }

  const link = document.createElement("span");
  link.className = "message-action-link";
  link.textContent = "tıklayın";
  link.title = "Gönderi videosunu klasörde göster";
  link.addEventListener("click", () => revealPath(targetPath));

  message.append(
    document.createTextNode("X/Twitter gönderi videosu hazırlandı. Klasörde göstermek için "),
    link,
    document.createTextNode(".")
  );
  message.className = "message is-success";
}

function showTwitterPhotoPostCardSuccessMessage(result) {
  const targetPath = normalizePath(result?.filePath || result?.outputDir || "");

  message.replaceChildren();

  if (!targetPath) {
    message.textContent = "X/Twitter gönderi kartı indirildi.";
    message.className = "message is-success";
    return;
  }

  const link = document.createElement("span");
  link.className = "message-action-link";
  link.textContent = "tıklayın";
  link.title = "Gönderi kartını klasörde göster";
  link.addEventListener("click", () => revealPath(targetPath));

  message.append(
    document.createTextNode("X/Twitter gönderi kartı indirildi. Klasörde göstermek için "),
    link,
    document.createTextNode(".")
  );
  message.className = "message is-success";
}

async function startTwitterPostDownload() {
  if (currentPlatform !== "twitter") return;

  if (!hasTwitterPostDownloadTarget()) {
    message.textContent = "Bu X/Twitter gönderisinde indirilebilir video bulunamadı.";
    message.className = "message is-error";
    return;
  }

  if (downloadState !== "idle") return;

  setDownloadState("downloading");

  if (downloadBtn) {
    downloadBtn.textContent = "Hazırlanıyor";
    downloadBtn.disabled = true;
    downloadBtn.classList.add("is-busy");
  }

  showProgress();
  setProgress({
    percent: 0,
    downloaded_mb: null,
    total_mb: null,
    speed_mb: null,
    phase: "Gönderi kartı hazırlanıyor...",
    line: "",
  });

  let args = null;
  try {
    const baseMetadata = {
      ...(currentTwitterPostMetadata || normalizeTwitterPostMetadata(currentInfo || {}, currentUrl)),
      duration: currentInfo?.duration || Number(selectedMediaItem()?.durationMs || 0) / 1000,
      quality:
        activeFormat?.quality ||
        (selectedMediaItem()?.height ? `${selectedMediaItem().height}p` : "Otomatik"),
    };
    const [metadata, secondaryMedia] = await Promise.all([
      hydrateTwitterCardMetadata(baseMetadata),
      twitterQuoteSecondaryMedia(baseMetadata.activeMediaRole),
    ]);
    currentTwitterPostMetadata = metadata;
    const renderedCard = await renderTwitterPostCardPng({ ...metadata, secondaryMedia });
    const cardPngBase64 = pngDataUrlToBase64(renderedCard?.dataUrl);
    const cardOverlayPngBase64 = pngDataUrlToBase64(renderedCard?.overlayDataUrl);

    if (!cardPngBase64) {
      throw twitterPostTemplateError("card_png_render_failed", "Kart PNG base64 çıktısı boş.");
    }

    if (!isValidTwitterPostCardLayout(renderedCard?.layout)) {
      throw twitterPostTemplateError(
        "card_layout_invalid",
        `Kart layout çıktısı geçersiz: ${JSON.stringify(renderedCard?.layout || null)}`
      );
    }

    args = twitterPostDownloadArgs(cardPngBase64, renderedCard.layout, cardOverlayPngBase64);
  } catch (error) {
    const stage = twitterPostErrorCode(error, "card_png_render_failed");
    const rawDetail = String(error?.message || error || "");
    const detail = rawDetail.replace(new RegExp(`^${stage}:\\s*`), "").split("\n")[0].slice(0, 140);

    console.error("Twitter post MP4 card render failed:", {
      stage,
      debugCode: error?.debugCode || stage,
      message: error?.message || String(error),
      cause: error?.cause,
      error,
      layoutSnapshot: error?.layoutSnapshot || error?.renderDebug?.layout || null,
      renderDebug: error?.renderDebug || null,
    });
    message.textContent = detail
      ? `Gönderi kartı oluşturulamadı: ${stage} - ${detail}`
      : `Gönderi kartı oluşturulamadı: ${stage}`;
    message.className = "message is-error";
    resetProgress();
    setDownloadState("idle");
    return;
  }

  setProgress({
    percent: 0,
    downloaded_mb: null,
    total_mb: null,
    speed_mb: null,
    phase: "Gönderi videosu indiriliyor...",
    line: "",
  });

  message.textContent = "X/Twitter gönderi videosu hazırlanıyor...";
  message.className = "message";

  try {
    const result = await invoke("download_twitter_post", args);
    const normalized = normalizeDownloadResult(result);

    setProgress({
      percent: 100,
      downloaded_mb: null,
      total_mb: null,
      speed_mb: null,
      phase: "X/Twitter gönderi videosu hazırlandı.",
      line: "",
    });

    if (normalized.filePath) {
      const completedItem = {
        title: currentInfo?.title || currentTwitterPostMetadata?.text || "X/Twitter gönderisi",
        platform: "twitter",
        quality: `Gönderi videosu · ${args.quality || activeFormat?.quality || "Otomatik"}`,
        url: args.webpageUrl || args.url || currentUrl,
        filePath: normalized.filePath,
        outputDir: normalized.outputDir,
        fileSize: normalized.fileSize,
        downloadedAtMs: Date.now(),
        downloadedAt: new Date().toISOString(),
      };
      showLastFileActions(completedItem);
      addHistoryItem(completedItem);
      showTwitterPostSuccessMessage(completedItem);
    } else {
      showTwitterPostSuccessMessage(normalized);
    }

    notifyDownloadComplete(normalized.filePath || normalized.outputDir, 1);

    setDownloadState("idle");
  } catch (error) {
    console.error(error);

    if (isDownloadControlError(error, "__MEDIADROP_CANCELLED__")) {
      message.textContent = "Gönderi videosu iptal edildi. Geçici dosyalar temizlendi.";
      message.className = "message";
      resetProgress();
      setDownloadState("idle");
      return;
    }

    if (isDownloadControlError(error, "__MEDIADROP_PAUSED__")) {
      message.textContent = "Gönderi indirme duraklatıldı. Tekrar Gönderiyi İndir ile baştan deneyebilirsin.";
      message.className = "message";
      setDownloadState("idle");
      return;
    }

    showErrorMessage(error);
    setDownloadState("idle");
  }
}

function updateSelectedFormatLabel() {
  if (!selectedFormat) return;

  if (isMediaPhotoMode()) {
    const item = selectedMediaItem();
    selectedFormat.textContent = item
      ? `${mediaItemKindLabel(item)} · ${mediaDimensionsLabel(item)}`
      : "Fotoğraf";
    return;
  }

  if (!activeFormat) {
    selectedFormat.textContent = "Henüz seçim yok";
    return;
  }

  const base = `${activeFormat.title} · ${activeFormat.quality}`;
  selectedFormat.textContent = clipSelection
    ? `${base} · Klip ${clipDurationLabelText(clipSelection)}`
    : base;
}

function updateClipControls() {
  const available = isClipFeatureAvailable();
  const idle = downloadState === "idle";

  if (clipBtn) {
    clipBtn.classList.toggle("is-hidden", !available);
    clipBtn.disabled = !available || !idle;
    clipBtn.textContent = clipSelection ? "Klip Düzenle" : "Klip İndir";
  }

  if (clipStatusBar && clipSummary) {
    const show = Boolean(clipSelection && available && idle);
    clipStatusBar.classList.toggle("is-hidden", !show);

    if (show) {
      clipSummary.textContent = `Klip modu aktif: ${clipDurationLabelText(clipSelection)} (${formatClipTime(clipSelection.end - clipSelection.start)})`;
    }
  }

  updateSelectedFormatLabel();
  updatePrimaryDownloadButtonLabel();
  updateTwitterPostControls();
  refreshWindowLayout();
}

function clearClipSelection({ silent = false } = {}) {
  clipSelection = null;
  clipDraft = null;
  clipPreviewMode = false;
  updateClipControls();

  if (!silent) {
    message.textContent = "Klip modu iptal edildi. Tam video indirilecek.";
    message.className = "message";
  }
}

function extractYouTubeVideoId(url, info = {}) {
  const fromInfo = String(info?.id || "").trim();
  if (/^[a-zA-Z0-9_-]{6,}$/.test(fromInfo)) return fromInfo;

  const text = String(url || "").trim();

  const patterns = [
    /youtu\.be\/([a-zA-Z0-9_-]{6,})/i,
    /youtube\.com\/shorts\/([a-zA-Z0-9_-]{6,})/i,
    /youtube\.com\/embed\/([a-zA-Z0-9_-]{6,})/i,
    /[?&]v=([a-zA-Z0-9_-]{6,})/i,
  ];

  for (const pattern of patterns) {
    const match = text.match(pattern);
    if (match?.[1]) return match[1];
  }

  return "";
}

function loadYouTubeIframeApi(timeoutMs = 10_000) {
  if (window.YT?.Player) return Promise.resolve(window.YT);
  if (!youtubeApiPromise) {
    youtubeApiPromise = new Promise((resolve, reject) => {
      const previousReady = window.onYouTubeIframeAPIReady;

      window.onYouTubeIframeAPIReady = () => {
        if (typeof previousReady === "function") previousReady();
        resolve(window.YT);
      };

      const script = document.createElement("script");
      script.src = "https://www.youtube.com/iframe_api";
      script.async = true;
      script.onerror = () => reject(new Error("YouTube ön izleme API yüklenemedi."));
      document.head.appendChild(script);
    });
    youtubeApiPromise.catch(() => {
      youtubeApiPromise = null;
    });
  }

  const waitMs = Math.floor(Number(timeoutMs));
  if (!Number.isFinite(waitMs) || waitMs <= 0) {
    return Promise.reject(new Error("YouTube ön izleme zaman aşımına uğradı."));
  }

  return Promise.race([
    youtubeApiPromise,
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error("YouTube ön izleme zaman aşımına uğradı.")), waitMs);
    }),
  ]);
}


function getSavedClipVolume() {
  const value = Number(localStorage.getItem(CLIP_VOLUME_KEY));
  if (!Number.isFinite(value)) return 80;
  return Math.max(0, Math.min(100, value));
}

function updateClipVolumeUI(value = getSavedClipVolume()) {
  const safe = Math.max(0, Math.min(100, Number(value) || 0));

  if (clipVolumeSlider) clipVolumeSlider.value = String(safe);
  if (clipVolumeValue) clipVolumeValue.textContent = `${safe}%`;
}

function applyClipVolume(value = getSavedClipVolume()) {
  const safe = Math.max(0, Math.min(100, Number(value) || 0));
  localStorage.setItem(CLIP_VOLUME_KEY, String(safe));
  updateClipVolumeUI(safe);

  try {
    clipPlayer?.setVolume?.(safe);
    if (safe === 0) {
      clipPlayer?.mute?.();
    } else {
      clipPlayer?.unMute?.();
    }
  } catch {}
}

function updateClipPlayButton(state = clipPlayerState) {
  if (!clipPlayBtn) return;

  const ready = Boolean(clipPlayerReady && clipPlayer);
  clipPlayBtn.disabled = !ready;

  if (!ready) {
    clipPlayBtn.textContent = "Oynat";
    return;
  }

  if (state === "playing") {
    clipPlayBtn.textContent = "Duraklat";
  } else if (state === "buffering") {
    clipPlayBtn.textContent = "Yükleniyor";
  } else {
    clipPlayBtn.textContent = "Oynat";
  }
}

function syncClipPlaybackState() {
  let state = "idle";

  try {
    const raw = clipPlayer?.getPlayerState?.();

    if (raw === 1) {
      state = "playing";
    } else if (raw === 3) {
      state = "buffering";
    } else if (raw === 2 || raw === 0 || raw === 5) {
      state = "paused";
    }
  } catch {}

  clipPlayerState = state;
  updateClipPlayButton(state);
}

function toggleClipPlayback() {
  if (!clipPlayerReady || !clipPlayer) return;

  try {
    syncClipPlaybackState();

    if (clipPlayerState === "playing" || clipPlayerState === "buffering") {
      clipPreviewMode = false;
      clipPlayer.pauseVideo();
      clipPlayerState = "paused";
    } else {
      clipPlayer.playVideo();
      clipPlayerState = "playing";
    }

    updateClipPlayButton(clipPlayerState);
  } catch (error) {
    console.warn("Clip play toggle failed:", error);
  }
}

function getFormatSignature(format = activeFormat) {
  if (!format) return "";

  return [
    format.id || "",
    format.quality || "",
    format.type || "",
  ].join("::");
}

function getClipPreviewCacheKey(videoId = extractYouTubeVideoId(currentUrl, currentInfo)) {
  const cleanUrl = String(currentUrl || "").trim();
  const cleanVideoId = String(videoId || "").trim();
  const formatSignature = getFormatSignature();

  if (!cleanUrl || !cleanVideoId || !formatSignature) return "";

  return `${cleanVideoId}::${formatSignature}::${cleanUrl}`;
}

function isClipPreviewCacheUsable(videoId) {
  const key = getClipPreviewCacheKey(videoId);

  return Boolean(
    key &&
    clipPreviewCache.ready &&
    clipPreviewCache.key === key &&
    clipPreviewCache.videoId === String(videoId || "") &&
    clipPreviewCache.formatSignature === getFormatSignature() &&
    clipPlayer &&
    clipPlayerReady
  );
}

function markClipPreviewCacheReady(videoId) {
  clipPreviewCache = {
    key: getClipPreviewCacheKey(videoId),
    videoId: String(videoId || ""),
    formatSignature: getFormatSignature(),
    ready: true,
    loading: false,
    mode: clipPlayerMode,
  };
}

function resetClipPreviewCacheState() {
  clipPreviewCache = {
    key: "",
    videoId: "",
    formatSignature: "",
    ready: false,
    loading: false,
    mode: "none",
  };
}

function pauseClipPlayerForHide() {
  clipPreviewMode = false;

  if (clipTicker) {
    clearInterval(clipTicker);
    clipTicker = null;
  }

  try {
    clipPlayer?.pauseVideo?.();
  } catch (error) {
    console.warn("Clip player pause failed:", error);
  }

  syncClipPlaybackState();
  updateClipPlayButton("paused");
}

function destroyClipPlayer() {
  clipPlayerBuildToken += 1;
  const cancelPending = cancelPendingClipPlayer;
  cancelPendingClipPlayer = null;
  try {
    cancelPending?.();
  } catch (error) {
    console.warn("Pending clip player cancel failed:", error);
  }
  resetClipPreviewCacheState();
  clipPreviewMode = false;
  clipPlayerReady = false;

  if (clipTicker) {
    clearInterval(clipTicker);
    clipTicker = null;
  }

  try {
    clipPlayer?.destroy?.();
  } catch (error) {
    console.warn("Clip player destroy failed:", error);
  }

  clipPlayer = null;
  clipPlayerMode = "none";
  clipPlayerState = "idle";
  clipFallbackVideoId = "";
  updateClipPlayButton("idle");

  if (clipPlayerMount) {
    clipPlayerMount.innerHTML = `
      <div class="clip-player-placeholder">
        <strong>Ön izleme hazırlanıyor...</strong>
        <span>Video stream hazırlanıyor.</span>
      </div>
    `;
  }
}

function createNativeClipPlayer(streamUrls, videoId, { buildToken, deadlineMs, audioUrl = "" }) {
  return new Promise((resolve, reject) => {
    if (!clipPlayerMount) {
      reject(new Error("Ön izleme alanı bulunamadı."));
      return;
    }

    const candidates = (Array.isArray(streamUrls) ? streamUrls : [streamUrls])
      .map((url) => String(url || "").trim())
      .filter(Boolean);
    const separateAudioUrl = String(audioUrl || "").trim();

    if (!candidates.length) {
      reject(new Error("Ön izleme stream URL bulunamadı."));
      return;
    }

    const video = document.createElement("video");
    video.className = "clip-native-video";
    video.preload = "metadata";
    video.playsInline = true;
    video.controls = false;
    video.disablePictureInPicture = true;
    video.setAttribute("controlslist", "nodownload noplaybackrate noremoteplayback");
    const audio = separateAudioUrl ? document.createElement("audio") : null;
    if (audio) {
      audio.className = "clip-native-audio";
      audio.preload = "auto";
      audio.hidden = true;
    }

    let settled = false;
    let timeoutId = null;
    let index = 0;
    const errors = [];
    const isActive = () => isClipPreviewBuildActive(
      buildToken,
      clipPlayerBuildToken,
      deadlineMs,
      performance.now()
    );

    const cleanupCandidateListeners = () => {
      video.removeEventListener("loadedmetadata", finishWhenReady);
      video.removeEventListener("canplay", finishWhenReady);
      video.removeEventListener("error", failCandidate);
      audio?.removeEventListener("loadedmetadata", finishWhenReady);
      audio?.removeEventListener("canplay", finishWhenReady);
      audio?.removeEventListener("error", failAudio);
    };

    const clearCandidateTimeout = () => {
      if (timeoutId !== null) {
        clearTimeout(timeoutId);
        timeoutId = null;
      }
    };

    let pendingCancel = null;
    const clearPendingCancel = () => {
      if (cancelPendingClipPlayer === pendingCancel) {
        cancelPendingClipPlayer = null;
      }
    };
    const disposeMedia = () => {
      clearCandidateTimeout();
      cleanupCandidateListeners();
      try {
        video.pause();
        video.removeAttribute("src");
        video.load();
        video.remove();
        if (audio) {
          audio.pause();
          audio.removeAttribute("src");
          audio.load();
          audio.remove();
        }
      } catch {}
    };
    const rejectAttempt = (error) => {
      if (settled) return;
      settled = true;
      clearPendingCancel();
      disposeMedia();
      reject(error instanceof Error ? error : new Error(String(error)));
    };

    const finish = () => {
      if (settled) return;
      if (!isActive()) {
        rejectAttempt(new Error("Ön izleme isteği artık geçerli değil."));
        return;
      }
      settled = true;
      clearCandidateTimeout();
      cleanupCandidateListeners();
      clearPendingCancel();

      clipPlayerMode = "native";
      clipFallbackVideoId = videoId || "";
      clipPlayerReady = true;

      const syncAudioToVideo = () => {
        if (!audio) return;
        const target = nativeClipAudioSyncTarget(video.currentTime, audio.currentTime, {
          videoReadyState: video.readyState,
          audioReadyState: audio.readyState,
          seeking: video.seeking || audio.seeking,
        });
        if (target === null) return;
        try { audio.currentTime = target; } catch {}
      };

      const playAudio = () => {
        audio?.play().catch((error) => {
          console.warn("Native preview audio failed:", error);
        });
      };

      if (audio) {
        video.addEventListener("waiting", () => audio.pause());
        video.addEventListener("playing", () => {
          syncAudioToVideo();
          playAudio();
        });
      }

      clipPlayer = {
        getCurrentTime: () => {
          syncAudioToVideo();
          return Number(video.currentTime || 0);
        },
        seekTo: (time) => {
          const safe = Math.max(0, Number(time) || 0);
          audio?.pause();
          try { video.currentTime = safe; } catch {}
          try { if (audio) audio.currentTime = safe; } catch {}
        },
        playVideo: () => {
          syncAudioToVideo();
          video.play().catch((error) => {
            console.warn("Native preview play failed:", error);
          });
        },
        pauseVideo: () => {
          video.pause();
          audio?.pause();
        },
        setVolume: (value) => {
          const safe = Math.max(0, Math.min(100, Number(value) || 0));
          const volumeTarget = audio || video;
          volumeTarget.volume = safe / 100;
          volumeTarget.muted = safe === 0;
          if (audio) video.muted = true;
        },
        mute: () => { (audio || video).muted = true; },
        unMute: () => { (audio || video).muted = false; },
        getPlayerState: () => nativeClipPlayerState(
          video.paused,
          audio ? Math.min(video.readyState, audio.readyState) : video.readyState
        ),
        destroy: disposeMedia,
      };

      const start = readClipInputs().start;
      clipPlayer.seekTo(start);
      applyClipVolume(getSavedClipVolume());
      updateClipPlayButton("paused");
      startClipTicker();

      if (clipMessage) {
        clipMessage.textContent = "Ham video ön izlemesi hazır. Zamanı seçebilirsin.";
        clipMessage.className = "clip-message";
      }

      resolve(true);
    };

    const finishWhenReady = () => {
      if (video.readyState >= 1 && (!audio || audio.readyState >= 1)) finish();
    };

    const rejectAll = () => {
      if (settled) return;
      rejectAttempt(new Error(
        `Ham video stream oynatılamadı. Denenen kaynak: ${candidates.length}. ${errors.join(" | ")}`
      ));
    };

    const tryCandidate = () => {
      if (settled) return;
      if (!isActive()) {
        rejectAttempt(new Error("Ham video ön izlemesi zaman aşımına uğradı."));
        return;
      }

      if (index >= candidates.length) {
        rejectAll();
        return;
      }

      clearCandidateTimeout();
      cleanupCandidateListeners();

      const timeoutMs = clipPreviewAttemptBudgetMs(
        deadlineMs,
        performance.now(),
        candidates.length - index
      );
      if (timeoutMs <= 0) {
        errors.push("preview deadline");
        rejectAll();
        return;
      }

      const currentUrl = candidates[index];
      index += 1;

      video.addEventListener("loadedmetadata", finishWhenReady, { once: true });
      video.addEventListener("canplay", finishWhenReady, { once: true });
      video.addEventListener("error", failCandidate, { once: true });
      audio?.addEventListener("loadedmetadata", finishWhenReady, { once: true });
      audio?.addEventListener("canplay", finishWhenReady, { once: true });
      audio?.addEventListener("error", failAudio, { once: true });

      if (clipMessage) {
        clipMessage.textContent = `Ham video ön izlemesi deneniyor (${index}/${candidates.length})...`;
        clipMessage.className = "clip-message";
      }

      try {
        video.pause();
        video.src = currentUrl;
        video.load();
      } catch (error) {
        errors.push(String(error));
        setTimeout(tryCandidate, 80);
        return;
      }

      timeoutId = setTimeout(() => {
        errors.push("candidate timeout");
        tryCandidate();
      }, timeoutMs);
    };

    function failCandidate() {
      if (settled) return;
      errors.push(`candidate ${index} failed`);
      tryCandidate();
    }

    function failAudio() {
      rejectAttempt(new Error("Ham video ses stream'i oynatılamadı."));
    }

    pendingCancel = () => rejectAttempt(new Error("Ön izleme isteği iptal edildi."));
    cancelPendingClipPlayer = pendingCancel;
    clipPlayerMount.innerHTML = "";
    clipPlayerMount.appendChild(video);
    if (audio) {
      audio.src = separateAudioUrl;
      clipPlayerMount.appendChild(audio);
      audio.load();
    }
    tryCandidate();
  });
}

async function createIframeClipPlayer(videoId, { buildToken, deadlineMs }) {
  if (!clipPlayerMount) return false;

  const isActive = () => isClipPreviewBuildActive(
    buildToken,
    clipPlayerBuildToken,
    deadlineMs,
    performance.now()
  );
  if (!isActive()) return false;

  const playerId = `clipPlayerInner-${Date.now()}`;
  clipPlayerMount.innerHTML = `<div id="${playerId}"></div>`;

  try {
    const YT = await loadYouTubeIframeApi(
      clipPreviewAttemptBudgetMs(deadlineMs, performance.now())
    );
    if (!isActive()) return false;

    const currentOrigin = window.location?.origin || "";
    const safeOrigin = /^https?:\/\//i.test(currentOrigin) ? currentOrigin : "https://www.youtube.com";

    return await new Promise((resolve, reject) => {
      let iframePlayer = null;
      let settled = false;
      let readyTimeoutId = null;
      let pendingCancel = null;

      const clearPending = () => {
        if (readyTimeoutId !== null) {
          clearTimeout(readyTimeoutId);
          readyTimeoutId = null;
        }
        if (cancelPendingClipPlayer === pendingCancel) {
          cancelPendingClipPlayer = null;
        }
      };
      const fail = (error) => {
        if (settled) return;
        settled = true;
        clearPending();
        if (clipPlayer === iframePlayer) {
          clipPlayer = null;
          clipPlayerReady = false;
          clipPlayerMode = "none";
        }
        try { iframePlayer?.destroy?.(); } catch {}
        reject(error instanceof Error ? error : new Error(String(error)));
      };
      const finish = () => {
        if (settled) return;
        if (!isActive() || clipPlayer !== iframePlayer) {
          fail(new Error("Ön izleme isteği artık geçerli değil."));
          return;
        }

        settled = true;
        clearPending();
        clipPlayerReady = true;
        const start = readClipInputs().start;
        iframePlayer.cueVideoById({ videoId, startSeconds: start });
        applyClipVolume(getSavedClipVolume());
        updateClipPlayButton("paused");
        startClipTicker();

        if (clipMessage) {
          clipMessage.textContent = "YouTube ön izlemesi hazır. Bazı videolarda gömülü oynatıcı sınırlı olabilir.";
          clipMessage.className = "clip-message";
        }
        resolve(true);
      };
      const handleError = (event) => {
        const code = event?.data;
        let detail = "YouTube ön izleme açılamadı.";
        if (code === 101 || code === 150) {
          detail = "Bu video gömülü ön izlemeye izin vermiyor.";
        } else if (code === 2) {
          detail = "YouTube video ID geçersiz görünüyor.";
        } else if (code === 100) {
          detail = "Video bulunamadı veya özel olabilir.";
        }
        if (settled) {
          if (isActive() && clipPlayer === iframePlayer && clipMessage) {
            clipMessage.textContent = `${detail} Süreleri elle girerek klip seçebilirsin.`;
            clipMessage.className = "clip-message is-error";
          }
          return;
        }
        fail(new Error(detail));
      };

      const readyBudgetMs = clipPreviewAttemptBudgetMs(deadlineMs, performance.now());
      if (readyBudgetMs <= 0) {
        fail(new Error("YouTube ön izleme zaman aşımına uğradı."));
        return;
      }
      readyTimeoutId = setTimeout(() => {
        fail(new Error("YouTube ön izleme zaman aşımına uğradı."));
      }, readyBudgetMs);
      pendingCancel = () => fail(new Error("Ön izleme isteği iptal edildi."));
      cancelPendingClipPlayer = pendingCancel;

      try {
        iframePlayer = new YT.Player(playerId, {
          width: "100%",
          height: "100%",
          videoId,
          playerVars: {
            autoplay: 0,
            controls: 0,
            disablekb: 1,
            fs: 0,
            rel: 0,
            modestbranding: 1,
            playsinline: 1,
            enablejsapi: 1,
            iv_load_policy: 3,
            origin: safeOrigin,
            widget_referrer: safeOrigin,
          },
          events: {
            onReady: finish,
            onStateChange: () => {
              if (isActive() && clipPlayer === iframePlayer) syncClipPlaybackState();
            },
            onError: handleError,
          },
        });
        clipPlayerMode = "iframe";
        clipFallbackVideoId = videoId || "";
        clipPlayer = iframePlayer;
      } catch (error) {
        fail(error);
      }
    });
  } catch (error) {
    if (!isActive()) return false;
    console.warn(error);
    clipPlayerMount.innerHTML = `
      <div class="clip-player-placeholder">
        <strong>Ön izleme yüklenemedi.</strong>
        <span>Süreleri elle girerek klip seçebilirsin.</span>
      </div>
    `;
    return false;
  }
}

async function createClipPlayer(videoId) {
  if (!clipPlayerMount) return;

  if (isClipPreviewCacheUsable(videoId)) {
    pauseClipPlayerForHide();
    const start = readClipInputs().start;

    try {
      clipPlayer?.seekTo?.(start, true);
    } catch {}

    if (clipMessage) {
      clipMessage.textContent = "Ön izleme hazır. Zamanı seçebilirsin.";
      clipMessage.className = "clip-message";
    }

    startClipTicker();
    return;
  }

  destroyClipPlayer();
  const buildToken = clipPlayerBuildToken;
  const deadlineMs = performance.now() + CLIP_PREVIEW_TOTAL_BUDGET_MS;
  const isCurrent = () => buildToken === clipPlayerBuildToken;
  const isActive = () => isClipPreviewBuildActive(
    buildToken,
    clipPlayerBuildToken,
    deadlineMs,
    performance.now()
  );
  clipPreviewCache.loading = true;
  clipPreviewCache.key = getClipPreviewCacheKey(videoId);
  clipPreviewCache.videoId = String(videoId || "");
  clipPreviewCache.formatSignature = getFormatSignature();

  if (clipMessage) {
    clipMessage.textContent = "Ham video ön izlemesi hazırlanıyor...";
    clipMessage.className = "clip-message";
  }

  try {
    const preview = await invoke("prepare_clip_preview_stream", {
      url: currentUrl,
      quality: activeFormat?.quality || "720p",
    });

    if (!isActive()) {
      if (isCurrent()) resetClipPreviewCacheState();
      return;
    }

    const { videoUrls: streamUrls, audioUrl } = clipPreviewStreamSources(preview);

    if (streamUrls.length) {
      await createNativeClipPlayer(streamUrls, videoId, {
        buildToken,
        audioUrl,
        deadlineMs: Math.min(
          deadlineMs,
          performance.now() + CLIP_PREVIEW_NATIVE_BUDGET_MS
        ),
      });

      if (isActive()) {
        markClipPreviewCacheReady(videoId);
      }

      return;
    }
  } catch (error) {
    console.warn("Native preview failed, falling back to YouTube iframe:", error);
  }

  if (!isActive()) {
    if (isCurrent()) {
      resetClipPreviewCacheState();
      if (clipMessage) {
        clipMessage.textContent = "Ön izleme zaman aşımına uğradı. Süreleri elle girerek klip seçebilirsin.";
        clipMessage.className = "clip-message is-error";
      }
    }
    return;
  }

  if (clipMessage) {
    clipMessage.textContent = "Ham ön izleme açılamadı. YouTube fallback deneniyor...";
    clipMessage.className = "clip-message";
  }

  await createIframeClipPlayer(videoId, { buildToken, deadlineMs });

  if (isActive() && clipPlayer && clipPlayerReady) {
    markClipPreviewCacheReady(videoId);
  } else if (isCurrent()) {
    resetClipPreviewCacheState();
    if (!isActive() && clipMessage) {
      clipMessage.textContent = "Ön izleme zaman aşımına uğradı. Süreleri elle girerek klip seçebilirsin.";
      clipMessage.className = "clip-message is-error";
    }
  }
}

function getClipPlayerTime() {
  try {
    const current = clipPlayer?.getCurrentTime?.();
    return Number.isFinite(current) ? current : 0;
  } catch {
    return 0;
  }
}

function startClipTicker() {
  if (clipTicker) clearInterval(clipTicker);

  clipTicker = setInterval(() => {
    const duration = getVideoDurationSeconds();
    const current = clampNumber(getClipPlayerTime(), 0, duration || 0);

    if (clipCurrentTime) clipCurrentTime.textContent = formatClipTime(current);

    if (clipSeek && duration > 0 && document.activeElement !== clipSeek) {
      clipSeek.value = String(current);
    }

    syncClipPlaybackState();

    if (clipPreviewMode) {
      const { start, end } = readClipInputs();
      if (current >= end) {
        clipPreviewMode = false;
        try {
          clipPlayer?.pauseVideo?.();
          clipPlayer?.seekTo?.(start, true);
        } catch {}

        if (clipMessage) {
          clipMessage.textContent = "Klip ön izlemesi tamamlandı.";
          clipMessage.className = "clip-message";
        }
      }
    }
  }, 250);
}

function writeClipInputs(start, end) {
  const safeStart = Math.max(0, Math.floor(start || 0));
  const safeEnd = Math.max(0, Math.floor(end || 0));

  if (clipStartMin) clipStartMin.value = String(Math.floor(safeStart / 60));
  if (clipStartSec) clipStartSec.value = String(safeStart % 60);
  if (clipEndMin) clipEndMin.value = String(Math.floor(safeEnd / 60));
  if (clipEndSec) clipEndSec.value = String(safeEnd % 60);

  updateClipMarkers();
}

function readClipInputs() {
  const duration = getVideoDurationSeconds();
  const max = duration || 24 * 60 * 60;

  const startMin = clampNumber(clipStartMin?.value, 0, 9999);
  const startSec = clampNumber(clipStartSec?.value, 0, 59);
  const endMin = clampNumber(clipEndMin?.value, 0, 9999);
  const endSec = clampNumber(clipEndSec?.value, 0, 59);

  const start = clampNumber(Math.floor(startMin) * 60 + Math.floor(startSec), 0, max);
  const end = clampNumber(Math.floor(endMin) * 60 + Math.floor(endSec), 0, max);

  return { start, end };
}

function normalizeClipInputs() {
  const { start, end } = readClipInputs();
  writeClipInputs(start, end);
}

function validateClip(showMessage = true) {
  const duration = getVideoDurationSeconds();
  const { start, end } = readClipInputs();

  let error = "";

  if (!isClipFeatureAvailable()) {
    error = "Klip için önce YouTube video formatı seçmelisin.";
  } else if (end <= start) {
    error = "Bitiş zamanı başlangıçtan büyük olmalı.";
  } else if (end - start < 1) {
    error = "Klip süresi en az 1 saniye olmalı.";
  } else if (duration > 0 && end > duration) {
    error = "Bitiş zamanı video süresini geçemez.";
  }

  if (showMessage && clipMessage) {
    if (error) {
      clipMessage.textContent = error;
      clipMessage.className = "clip-message is-error";
    } else {
      clipMessage.textContent = `Seçili klip: ${formatClipTime(start)} → ${formatClipTime(end)} (${formatClipTime(end - start)})`;
      clipMessage.className = "clip-message is-success";
    }
  }

  return error ? null : { start, end };
}

function updateClipMarkers() {
  const duration = getVideoDurationSeconds();
  const { start, end } = readClipInputs();

  if (clipDurationLabel) clipDurationLabel.textContent = formatClipTime(duration);

  if (clipSeek) {
    clipSeek.max = String(duration || 0);
    clipSeek.disabled = duration <= 0;
  }

  const startPct = duration > 0 ? Math.max(0, Math.min(100, (start / duration) * 100)) : 0;
  const endPct = duration > 0 ? Math.max(0, Math.min(100, (end / duration) * 100)) : 0;
  const widthPct = Math.max(0, endPct - startPct);

  if (clipSelectedRange) {
    clipSelectedRange.style.left = `${startPct}%`;
    clipSelectedRange.style.width = `${widthPct}%`;
  }

  if (clipStartMarker) clipStartMarker.style.left = `${startPct}%`;
  if (clipEndMarker) clipEndMarker.style.left = `${endPct}%`;

  validateClip(false);
}

function openClipEditor() {
  if (!isClipFeatureAvailable()) {
    message.textContent = "Klip için önce YouTube video formatı seç.";
    message.className = "message is-error";
    return;
  }

  const duration = getVideoDurationSeconds();
  const start = clipSelection?.start ?? 0;
  const end = clipSelection?.end ?? Math.min(duration, 15);
  const videoId = extractYouTubeVideoId(currentUrl, currentInfo);

  if (!videoId) {
    message.textContent = "YouTube video ID okunamadı.";
    message.className = "message is-error";
    return;
  }

  clipDraft = { start, end, videoId };
  writeClipInputs(start, end);
  updateClipMarkers();

  if (clipCurrentTime) clipCurrentTime.textContent = formatClipTime(start);
  if (clipMessage) {
    clipMessage.textContent = "Başlangıç ve bitiş zamanını seç.";
    clipMessage.className = "clip-message";
  }

  document.body.classList.add("clip-editor-open");
  modalController.open("clip-editor");
  setClipWindowSize();

  createClipPlayer(videoId);
}

function closeClipEditor() {
  modalController.close("clip-editor");
  document.body.classList.remove("clip-editor-open");
  pauseClipPlayerForHide();
  setMainWindowSize();
}

function setClipInputToCurrent(target) {
  const current = Math.floor(getClipPlayerTime());
  const { start, end } = readClipInputs();

  if (target === "start") {
    writeClipInputs(current, Math.max(end, current + 1));
  } else {
    if (current <= start) {
      if (clipMessage) {
        clipMessage.textContent = "Bitiş zamanı başlangıçtan büyük olmalı. Önce videoyu ileri sar.";
        clipMessage.className = "clip-message is-error";
      }
      return;
    }

    writeClipInputs(start, current);
  }

  validateClip(true);
}

function previewSelectedClip() {
  const clip = validateClip(true);
  if (!clip || !clipPlayerReady) return;

  try {
    clipPreviewMode = true;
    clipPlayer.seekTo(clip.start, true);
    clipPlayer.playVideo();
    if (clipMessage) {
      clipMessage.textContent = `Ön izleme oynatılıyor: ${clipDurationLabelText(clip)}`;
      clipMessage.className = "clip-message";
    }
  } catch (error) {
    console.warn(error);
  }
}

function saveClipSelection() {
  const clip = validateClip(true);
  if (!clip) return;

  clipSelection = {
    enabled: true,
    start: clip.start,
    end: clip.end,
    videoId: extractYouTubeVideoId(currentUrl, currentInfo),
    sourceUrl: currentUrl,
  };

  closeClipEditor();
  updateClipControls();

  message.textContent = `Klip seçildi: ${clipDurationLabelText(clipSelection)}. İndir’e basınca sadece bu aralık indirilecek.`;
  message.className = "message is-success";
}

function clipPayloadForDownload() {
  if (!clipSelection || !isClipFeatureAvailable()) return null;
  return {
    start: clipSelection.start,
    end: clipSelection.end,
  };
}

async function chooseDownloadFolder() {
  if (["downloading", "pausing", "paused", "canceling"].includes(downloadState)) {
    message.textContent = "Devam eden indirme varken kayıt klasörü değiştirilemez.";
    message.className = "message is-error";
    return;
  }

  try {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: "İndirme klasörü seç",
    });

    if (!selected || typeof selected !== "string") return;

    selectedOutputDir = selected;
    localStorage.setItem(DOWNLOAD_DIR_KEY, selectedOutputDir);
    updateFolderLabel();

    message.textContent = `Kayıt klasörü seçildi: ${shortPath(selectedOutputDir)}`;
    message.className = "message is-success";
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
  }
}

function clearPreviewImage() {
  if (!videoThumb) return;

  videoThumb.onerror = null;
  videoThumb.onload = null;
  videoThumb.removeAttribute("src");
  videoThumb.alt = "Video thumbnail";
}

async function loadLocalThumbnailFallback(token, sourceUrl) {
  if (!sourceUrl || typeof invoke !== "function") return false;

  try {
    const dataUrl = await invoke("cache_thumbnail", { url: sourceUrl });

    if (token !== previewToken || !dataUrl) return true;

    videoThumb.onerror = null;
    videoThumb.onload = null;
    videoThumb.alt = "Video thumbnail";
    videoThumb.src = dataUrl;

    return true;
  } catch (error) {
    console.warn("Thumbnail fallback failed:", error);
    return false;
  }
}

function loadPreviewThumbnail(info, platform = "youtube") {
  if (!videoThumb) return;

  const token = ++previewToken;
  const candidates = getThumbnailCandidates(info);
  let index = 0;

  clearPreviewImage();

  videoThumb.referrerPolicy = "no-referrer";

  const tryNextRemote = () => {
    if (token !== previewToken) return;

    const nextUrl = candidates[index];
    index += 1;

    if (nextUrl) {
      videoThumb.src = nextUrl;
      return;
    }

    clearPreviewImage();
  };

  // Instagram WebView içinde remote thumbnail'i çoğu zaman patlatıyor.
  // O yüzden Instagram'da önce yt-dlp ile lokal/base64 thumbnail alıyoruz.
  if (platform === "instagram") {
    loadLocalThumbnailFallback(token, currentUrl).then((handled) => {
      if (token !== previewToken) return;

      if (!handled) {
        tryNextRemote();
      }
    });

    return;
  }

  videoThumb.onerror = tryNextRemote;
  tryNextRemote();
}

function isMediaPhotoMode() {
  return currentMediaItems.length > 0;
}

function isTwitterTextPostMode() {
  return currentPlatform === "twitter" && currentTwitterTextOnly;
}

function selectedMediaItem() {
  if (!currentMediaItems.length) return null;
  const safeIndex = Math.max(0, Math.min(currentMediaIndex, currentMediaItems.length - 1));
  return currentMediaItems[safeIndex] || null;
}

function mediaDimensionsLabel(item) {
  const width = Number(item?.width || 0);
  const height = Number(item?.height || 0);
  if (width > 0 && height > 0) return `${width}×${height}`;
  return "En yüksek çözünürlük";
}

function mediaItemDetailsLabel(item) {
  const parts = [mediaDimensionsLabel(item)];
  const durationMs = Number(item?.durationMs || 0);
  if (mediaItemType(item) === "video") {
    if (durationMs > 0) parts.push(formatMediaDuration(durationMs / 1000));
    if (item?.hasAudio === true) parts.push("Sesli");
    if (item?.hasAudio === false) parts.push("Sessiz");
  }
  return parts.join(" · ");
}

function mediaPreviewPlatform() {
  return currentMediaAnalysis?.platform || currentPlatform;
}

function isTwitterMediaPreview() {
  return mediaPreviewPlatform() === "twitter";
}

function isInstagramMediaPreview() {
  return mediaPreviewPlatform() === "instagram";
}

function isSocialMediaCardPreview() {
  return isTwitterMediaPreview() || isInstagramMediaPreview();
}

function mediaAnalysisAuthor() {
  const author = currentMediaAnalysis?.author;
  if (!author || typeof author !== "object" || Array.isArray(author)) return null;

  return {
    id: metadataString(author.id),
    name: metadataString(author.name),
    handle: metadataString(author.handle),
    avatarDataUrl: safeRasterImageDataUrl(author.avatarDataUrl),
    avatarUrl: metadataString(author.avatarUrl),
  };
}

function mediaItemTitle(item, fallback = "Fotoğraf") {
  const rawText = metadataString(item?.text);
  const text = rawText ? cleanTwitterPostText(rawText) : "";
  if (isSocialMediaCardPreview() && text) return text;
  return metadataString(currentMediaAnalysis?.title || item?.title || fallback);
}

function normalizeMediaDisplayDate(value) {
  return formatTwitterDisplayDate(value);
}

function twitterMediaMetadataFromItem(item = {}) {
  const quoteContext = normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote);
  if (quoteContext) {
    const outer = quoteContext.outer;
    const quoted = quoteContext.quoted;
    const quotedIndexes = new Set(quoteContext.quotedMediaIndexes);
    return {
      ...outer,
      text: outer.text ? cleanTwitterPostText(outer.text) : "",
      authorName: metadataString(outer.authorName) || "X/Twitter",
      authorHandle: normalizeTwitterHandle(outer.authorHandle),
      displayDate: normalizeMediaDisplayDate(outer.displayDate),
      avatarUrl: metadataString(outer.avatarUrl),
      avatarDataUrl: safeTwitterAvatarDataUrl(outer.avatarDataUrl),
      webpageUrl: currentUrl,
      sourceLabel: "x.com",
      quotedPost: {
        ...quoted,
        text: quoted.text ? cleanTwitterPostText(quoted.text) : "",
        authorName: metadataString(quoted.authorName) || "X/Twitter",
        authorHandle: normalizeTwitterHandle(quoted.authorHandle),
        displayDate: normalizeMediaDisplayDate(quoted.displayDate),
        avatarUrl: metadataString(quoted.avatarUrl),
        avatarDataUrl: safeTwitterAvatarDataUrl(quoted.avatarDataUrl),
        sourceLabel: "x.com",
      },
      quotedMediaIndexes: quoteContext.quotedMediaIndexes,
      activeMediaRole: quotedIndexes.has(currentMediaIndex) ? "quoted" : "outer",
      hasOuterMedia: currentMediaItems.some((_, index) => !quotedIndexes.has(index)),
      hasQuotedMedia: currentMediaItems.some((_, index) => quotedIndexes.has(index)),
    };
  }

  const analysisAuthor = mediaAnalysisAuthor();
  const handle = normalizeTwitterHandle(analysisAuthor?.handle || item.authorHandle) || twitterHandleFromUrl(currentUrl);
  const authorName = metadataString(analysisAuthor?.name || item.authorName) || metadataString(currentMediaAnalysis?.uploader) || "X/Twitter";
  const rawText = metadataString(item.text);
  const text = rawText ? cleanTwitterPostText(rawText) : "";

  return {
    text,
    exportText: rawText,
    authorName,
    authorHandle: handle,
    displayDate: normalizeMediaDisplayDate(item.displayDate),
    avatarUrl: metadataString(analysisAuthor?.avatarUrl || item.avatarUrl),
    avatarDataUrl: analysisAuthor?.avatarDataUrl || safeTwitterAvatarDataUrl(item.avatarUrl),
    replyCount: item.replyCount,
    retweetCount: item.retweetCount,
    likeCount: item.likeCount,
    viewCount: item.viewCount,
    webpageUrl: currentUrl,
    sourceLabel: "x.com",
  };
}

function normalizeInstagramHandle(value) {
  const clean = metadataString(value).replace(/^@+/, "");
  const handle = clean.split(/[/?#\s]/)[0]?.replace(/^@+/, "") || "";
  return handle ? `@${handle}` : "";
}

function instagramMediaMetadataFromItem(item = {}) {
  const analysisAuthor = mediaAnalysisAuthorIdentity(currentMediaAnalysis, item);
  const rawText = mediaCardDescription(item, currentMediaAnalysis);
  const text = rawText ? cleanTwitterPostText(rawText) : "";
  const authorName =
    metadataString(analysisAuthor?.name) ||
    normalizeInstagramHandle(analysisAuthor?.handle).replace(/^@/, "") ||
    (!analysisAuthor.registryBacked ? metadataString(currentMediaAnalysis?.uploader) : "") ||
    "Instagram";
  const authorHandle = normalizeInstagramHandle(analysisAuthor?.handle);
  const avatarUrl = metadataString(analysisAuthor?.avatarUrl);
  const avatarDataUrl = safeRasterImageDataUrl(analysisAuthor?.avatarDataUrl);

  return {
    text,
    authorName,
    authorHandle,
    displayDate: normalizeMediaDisplayDate(item.displayDate),
    avatarUrl,
    avatarDataUrl,
    replyCount: null,
    retweetCount: null,
    likeCount: item.likeCount,
    viewCount: item.viewCount,
    webpageUrl: currentUrl,
    sourceLabel: "instagram.com",
  };
}

function mediaCardMetadataFromItem(item = {}) {
  if (isInstagramMediaPreview()) return instagramMediaMetadataFromItem(item);
  return twitterMediaMetadataFromItem(item);
}

async function hydrateMediaCardAvatarDataUrl(metadata = {}) {
  if (isTwitterMediaPreview()) return hydrateTwitterAvatarDataUrl(metadata);

  const avatarDataUrl = safeRasterImageDataUrl(metadata.avatarDataUrl);
  if (avatarDataUrl) return { ...metadata, avatarDataUrl };
  return metadata;
}

async function hydrateTwitterCardMetadata(metadata = {}) {
  if (!metadata?.quotedPost) return hydrateTwitterAvatarDataUrl(metadata);
  const [outer, quotedPost] = await Promise.all([
    hydrateTwitterAvatarDataUrl(metadata),
    hydrateTwitterAvatarDataUrl(metadata.quotedPost),
  ]);
  return { ...outer, quotedPost };
}

function setMediaTweetAvatarData(dataUrl = "") {
  const clean = safeRasterImageDataUrl(dataUrl);

  if (mediaTweetAvatar) {
    mediaTweetAvatar.classList.toggle("is-hidden", !clean);
    if (clean) {
      mediaTweetAvatar.src = clean;
    } else {
      mediaTweetAvatar.removeAttribute("src");
    }
  }

  mediaTweetAvatarInitial?.classList.toggle("is-hidden", Boolean(clean));
}

function setMediaQuotedTweetAvatarData(dataUrl = "") {
  const clean = safeRasterImageDataUrl(dataUrl);
  if (mediaQuotedTweetAvatar) {
    mediaQuotedTweetAvatar.classList.toggle("is-hidden", !clean);
    if (clean) mediaQuotedTweetAvatar.src = clean;
    else mediaQuotedTweetAvatar.removeAttribute("src");
  }
  mediaQuotedTweetAvatarInitial?.classList.toggle("is-hidden", Boolean(clean));
}

function setMediaFrameBackdrop(dataUrl = "") {
  const clean = safeRasterImageDataUrl(dataUrl);
  if (!mediaFrame) return;

  if (clean && isSocialMediaCardPreview()) {
    mediaFrame.style.setProperty("--media-backdrop", `url("${clean}")`);
  } else {
    mediaFrame.style.removeProperty("--media-backdrop");
  }
}

function renderMediaTweetCard(item, { hydrateAvatar = false, token = previewToken } = {}) {
  const show = isSocialMediaCardPreview() && Boolean(item);
  const twitterCard = show && isTwitterMediaPreview();
  const instagramCard = show && isInstagramMediaPreview();
  const quoteContext = twitterCard
    ? normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote)
    : null;
  const quoteCard = Boolean(quoteContext);

  mediaPreview?.classList.toggle("is-twitter-card", twitterCard);
  mediaPreview?.classList.toggle("is-instagram-card", instagramCard);
  mediaStage?.classList.toggle("is-twitter-card", twitterCard);
  mediaStage?.classList.toggle("is-instagram-card", instagramCard);
  mediaStage?.classList.toggle("is-twitter-quote", quoteCard);
  mediaTweetCard?.classList.toggle("is-hidden", !show);
  mediaTweetCard?.setAttribute("aria-hidden", show ? "false" : "true");
  mediaQuotedTweetCard?.classList.toggle("is-hidden", !quoteCard);
  mediaQuotedTweetCard?.setAttribute("aria-hidden", quoteCard ? "false" : "true");

  if (!show) {
    mediaPreview?.classList.remove("is-twitter-card", "is-instagram-card");
    mediaStage?.classList.remove(
      "is-twitter-card",
      "is-instagram-card",
      "is-twitter-quote",
      "has-outer-media",
      "has-quoted-media"
    );
    setMediaTweetAvatarData("");
    setMediaQuotedTweetAvatarData("");
    setMediaFrameBackdrop("");
    return;
  }

  const metadata = mediaCardMetadataFromItem(item);
  const actionParts = isInstagramMediaPreview()
    ? [
        metadata.likeCount ? `${formatTwitterCompactCount(metadata.likeCount)} beğeni` : "",
        metadata.viewCount ? `${formatTwitterCompactCount(metadata.viewCount)} görüntülenme` : "",
      ].filter(Boolean)
    : [
        metadata.replyCount ? `${formatTwitterCompactCount(metadata.replyCount)} yanıt` : "",
        metadata.retweetCount ? `${formatTwitterCompactCount(metadata.retweetCount)} repost` : "",
        metadata.likeCount ? `${formatTwitterCompactCount(metadata.likeCount)} beğeni` : "",
      ].filter(Boolean);
  const metaParts = [
    isTwitterTextPostMode() ? "" : `${currentMediaIndex + 1} / ${currentMediaItems.length}`,
    metadata.displayDate,
    actionParts.join(" · "),
  ].filter(Boolean);

  if (mediaTweetName) mediaTweetName.textContent = metadata.authorName;
  if (mediaTweetHandle) mediaTweetHandle.textContent = metadata.authorHandle || metadata.sourceLabel || "x.com";
  if (mediaTweetMeta) mediaTweetMeta.textContent = metaParts.join(" · ");
  if (mediaTweetText) {
    mediaTweetText.textContent = metadata.text;
    mediaTweetText.classList.toggle("is-empty", !metadata.text);
  }
  if (mediaTweetAvatarInitial) {
    mediaTweetAvatarInitial.textContent = twitterAvatarInitial(metadata.authorName || metadata.authorHandle);
  }
  setMediaTweetAvatarData(metadata.avatarDataUrl);

  const quotedPost = quoteCard ? metadata.quotedPost : null;
  if (quotedPost) {
    if (mediaQuotedTweetName) mediaQuotedTweetName.textContent = quotedPost.authorName;
    if (mediaQuotedTweetHandle) {
      mediaQuotedTweetHandle.textContent = quotedPost.authorHandle || quotedPost.sourceLabel || "x.com";
    }
    if (mediaQuotedTweetMeta) mediaQuotedTweetMeta.textContent = quotedPost.displayDate || "";
    if (mediaQuotedTweetText) {
      mediaQuotedTweetText.textContent = quotedPost.text;
      mediaQuotedTweetText.classList.toggle("is-empty", !quotedPost.text);
    }
    if (mediaQuotedTweetAvatarInitial) {
      mediaQuotedTweetAvatarInitial.textContent = twitterAvatarInitial(
        quotedPost.authorName || quotedPost.authorHandle
      );
    }
    setMediaQuotedTweetAvatarData(quotedPost.avatarDataUrl);
  } else {
    setMediaQuotedTweetAvatarData("");
  }

  if (!hydrateAvatar) return;

  const hydrate = quoteCard
    ? hydrateTwitterCardMetadata(metadata)
    : hydrateMediaCardAvatarDataUrl(metadata);
  hydrate.then((hydrated) => {
    if (token !== previewToken) return;
    setMediaTweetAvatarData(hydrated.avatarDataUrl);
    setMediaQuotedTweetAvatarData(hydrated.quotedPost?.avatarDataUrl);
  }).catch((error) => {
    console.debug("Media preview avatar could not be hydrated:", error);
  });
}

async function mediaPreviewImageSource(item) {
  const imagePreviewUrl = metadataString(mediaPreviewImage?.dataset?.previewUrl);
  const currentSrc = imagePreviewUrl === mediaPreviewCacheKey(item)
    ? normalizeRasterImageSource(mediaPreviewImage?.src)
    : "";
  if (currentSrc) return currentSrc;

  return getCachedMediaPreviewDataUrl(item);
}

function mediaItemAspectRatio(item = {}) {
  const width = Number(item.width || 0);
  const height = Number(item.height || 0);
  return width > 0 && height > 0 ? width / height : 16 / 9;
}

async function captureVideoPosterDataUrl(source, item = {}) {
  const cleanSource = metadataString(source);
  if (!cleanSource) return "";

  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  const loaded = new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => reject(new Error("Video kapak karesi zaman aşımına uğradı.")), 8_000);
    video.onloadeddata = () => {
      window.clearTimeout(timeout);
      resolve();
    };
    video.onerror = () => {
      window.clearTimeout(timeout);
      reject(new Error("Video kapak karesi okunamadı."));
    };
  });

  try {
    video.src = cleanSource;
    video.load();
    await loaded;
    const sourceWidth = Number(video.videoWidth || item.width || 0);
    const sourceHeight = Number(video.videoHeight || item.height || 0);
    if (sourceWidth <= 0 || sourceHeight <= 0) return "";
    const scale = Math.min(1, 1280 / Math.max(sourceWidth, sourceHeight));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(2, evenCanvasDimension(sourceWidth * scale));
    canvas.height = Math.max(2, evenCanvasDimension(sourceHeight * scale));
    const ctx = canvas.getContext("2d");
    if (!ctx) return "";
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
    return safeRasterImageDataUrl(canvas.toDataURL("image/jpeg", 0.88));
  } finally {
    try {
      video.pause();
    } catch {}
    video.removeAttribute("src");
    video.load();
  }
}

function evenCanvasDimension(value) {
  const rounded = Math.max(2, Math.round(Number(value) || 0));
  return rounded % 2 === 0 ? rounded : rounded - 1;
}

async function twitterQuoteSecondaryMedia(activeRole) {
  const quote = normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote);
  if (!quote || !currentMediaItems.length) return null;
  const quotedIndexes = new Set(quote.quotedMediaIndexes);
  const secondaryRole = activeRole === "quoted" ? "outer" : "quoted";
  const secondaryIndex = currentMediaItems.findIndex((_, index) =>
    secondaryRole === "quoted" ? quotedIndexes.has(index) : !quotedIndexes.has(index)
  );
  if (secondaryIndex < 0) return null;

  const item = currentMediaItems[secondaryIndex];
  const source = await getCachedMediaPreviewDataUrl(item);
  const rasterSource = mediaItemType(item) === "video"
    ? await captureVideoPosterDataUrl(source, item)
    : normalizeRasterImageSource(source);
  if (!rasterSource) return null;
  return {
    role: secondaryRole,
    source: rasterSource,
    aspectRatio: mediaItemAspectRatio(item),
  };
}

async function renderTwitterPhotoPostCardPng(item) {
  if (!item || !isTwitterMediaPreview()) {
    throw new Error("X/Twitter gönderi kartı için seçili fotoğraf yok.");
  }

  const baseMetadata = twitterMediaMetadataFromItem(item);
  const [metadata, secondaryMedia] = await Promise.all([
    hydrateTwitterCardMetadata(baseMetadata),
    twitterQuoteSecondaryMedia(baseMetadata.activeMediaRole),
  ]);
  const imageSource = await mediaPreviewImageSource(item);
  return renderTwitterPhotoCardPng({ ...metadata, secondaryMedia }, imageSource, item);
}

async function renderTwitterTextPostCardPng() {
  const metadata = await hydrateTwitterCardMetadata(currentTwitterPostMetadata || {});
  currentTwitterPostMetadata = metadata;
  return renderTwitterTextCardPng(metadata);
}

function resetMediaPreview({ resize = true } = {}) {
  previewToken += 1;
  currentTwitterTextOnly = false;
  lastMediaDownloadArgs = null;
  mediaPreviewCache = new Map();
  appStore.dispatch({ type: "analysis/reset" });

  if (mediaPreview) {
    mediaPreview.classList.add("is-hidden");
    mediaPreview.classList.remove("is-text-only");
    mediaPreview.setAttribute("aria-hidden", "true");
  }

  if (mediaPreviewImage) {
    mediaPreviewImage.onload = null;
    mediaPreviewImage.onerror = null;
    mediaPreviewImage.removeAttribute("src");
    delete mediaPreviewImage.dataset.previewUrl;
    mediaPreviewImage.classList.remove("is-hidden");
  }

  resetMediaPreviewVideo();
  resetQuotedMediaPreview();

  setMediaFrameBackdrop("");
  renderMediaTweetCard(null);
  mediaStage?.classList.remove("is-text-only");
  mediaStage?.classList.remove("is-twitter-quote", "has-outer-media", "has-quoted-media");
  mediaFrame?.classList.remove("is-hidden");
  downloadPanel?.classList.remove("is-media-hidden");
  mediaFrame?.classList.remove("is-loading");
  updateMediaControls();

  if (resize && currentWindowLayoutMode === "media") {
    setMainWindowSize();
  }
}

function resetMediaPreviewVideo() {
  if (!mediaPreviewVideo) return;

  try {
    mediaPreviewVideo.pause();
  } catch {}
  mediaPreviewVideo.onloadedmetadata = null;
  mediaPreviewVideo.oncanplay = null;
  mediaPreviewVideo.onerror = null;
  mediaPreviewVideo.removeAttribute("src");
  mediaPreviewVideo.removeAttribute("poster");
  delete mediaPreviewVideo.dataset.previewKey;
  mediaPreviewVideo.load();
  mediaPreviewVideo.classList.add("is-hidden");
}

function resetQuotedMediaPreview() {
  if (mediaQuotedPreviewImage) {
    mediaQuotedPreviewImage.onload = null;
    mediaQuotedPreviewImage.onerror = null;
    mediaQuotedPreviewImage.removeAttribute("src");
    delete mediaQuotedPreviewImage.dataset.previewUrl;
    mediaQuotedPreviewImage.classList.add("is-hidden");
  }
  if (mediaQuotedPreviewVideo) {
    try {
      mediaQuotedPreviewVideo.pause();
    } catch {}
    mediaQuotedPreviewVideo.onloadedmetadata = null;
    mediaQuotedPreviewVideo.oncanplay = null;
    mediaQuotedPreviewVideo.onerror = null;
    mediaQuotedPreviewVideo.removeAttribute("src");
    delete mediaQuotedPreviewVideo.dataset.previewKey;
    mediaQuotedPreviewVideo.load();
    mediaQuotedPreviewVideo.classList.add("is-hidden");
  }
  mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedFrame?.style.removeProperty("--quoted-media-backdrop");
}

function mediaAnalysisId() {
  return metadataString(currentMediaAnalysis?.analysisId);
}

function ensureCurrentMediaAnalysisFresh() {
  if (!isMediaAnalysisExpired(currentMediaAnalysis)) return true;

  lastMediaDownloadArgs = null;
  message.textContent =
    "Bu medya analizinin süresi doldu. Bağlantıyı yeniden analiz edip indirmeyi tekrar başlat.";
  message.className = "message is-error";
  return false;
}

function currentMediaPreviewPolicy(item) {
  return mediaPreviewPolicy(currentMediaAnalysis, item);
}

function mediaPreviewCacheKey(item) {
  return currentMediaPreviewPolicy(item).cacheKey;
}

async function prepareMediaPreviewSource(item, policy = currentMediaPreviewPolicy(item)) {
  if (!policy.registryBacked) {
    throw new Error("Onizleme icin analysisId ve itemId zorunludur.");
  }

  const prepared = await invoke("prepare_media_preview", {
    analysisId: policy.analysisId,
    itemId: policy.itemId,
  });
  const source = normalizeMediaPreviewResponse(prepared, convertFileSrc);
  if (source) return source;
  throw new Error("Backend onizleme dosyasi hazirlamadi.");
}

async function getCachedMediaPreviewDataUrl(
  item,
  {
    forcePrepare = false,
    policy = currentMediaPreviewPolicy(item),
  } = {}
) {
  const key = policy.cacheKey;
  if (!key) return "";

  const cache = mediaPreviewCache;
  const cached = cache.get(key);
  const reusable = reusableMediaPreviewValue(cached, forcePrepare);
  if (reusable) return reusable;

  const requestMarker = {};
  const promise = prepareMediaPreviewSource(item, policy).then((source) => {
    const clean = metadataString(source);
    if (cache.get(key)?.requestMarker !== requestMarker) return clean;

    if (clean) {
      cache.set(key, {
        source: clean,
        dataUrl: safeRasterImageDataUrl(clean),
      });
    } else {
      cache.delete(key);
    }
    return clean;
  }).catch((error) => {
    if (cache.get(key)?.requestMarker === requestMarker) {
      if (cached?.source || cached?.dataUrl) {
        cache.set(key, {
          source: cached.source || cached.dataUrl,
          dataUrl: cached.dataUrl || safeRasterImageDataUrl(cached.source),
        });
      } else {
        cache.delete(key);
      }
    }
    throw error;
  });

  cache.set(key, {
    ...(cached?.source ? { source: cached.source } : {}),
    ...(cached?.dataUrl ? { dataUrl: cached.dataUrl } : {}),
    promise,
    requestMarker,
  });
  return promise;
}

function preloadMediaPreviewItems(items = [], startIndex = 0) {
  const queue = selectMediaPreviewPrefetchItems(items, startIndex, {
    canPrefetch: (item) => Boolean(mediaPreviewCacheKey(item)),
    isPrepared: (item) => {
      const cached = mediaPreviewCache.get(mediaPreviewCacheKey(item));
      return Boolean(cached?.source || cached?.promise);
    },
  });

  let cursor = 0;
  const workerCount = Math.min(2, queue.length);
  const runNext = () => {
    const next = queue[cursor];
    cursor += 1;
    if (!next) return Promise.resolve();

    return getCachedMediaPreviewDataUrl(next.item)
      .catch((error) => console.debug("Media preview preload skipped:", error))
      .then(runNext);
  };

  for (let index = 0; index < workerCount; index += 1) {
    runNext();
  }
}

function showMediaPreviewImage(source, cacheKey) {
  if (!mediaPreviewImage) return;

  resetMediaPreviewVideo();
  mediaPreviewImage.classList.remove("is-hidden");
  mediaPreviewImage.referrerPolicy = "no-referrer";
  mediaPreviewImage.dataset.previewUrl = cacheKey;
  mediaPreviewImage.src = source;
  setMediaFrameBackdrop(source);
}

function showMediaPreviewVideo(source, cacheKey, item) {
  if (!mediaPreviewVideo) return;

  if (mediaPreviewImage) {
    mediaPreviewImage.onload = null;
    mediaPreviewImage.onerror = null;
    mediaPreviewImage.removeAttribute("src");
    delete mediaPreviewImage.dataset.previewUrl;
    mediaPreviewImage.classList.add("is-hidden");
  }
  resetMediaPreviewVideo();
  setMediaFrameBackdrop("");
  mediaPreviewVideo.classList.remove("is-hidden");
  mediaPreviewVideo.dataset.previewKey = cacheKey;
  mediaPreviewVideo.onloadedmetadata = () => mediaFrame?.classList.remove("is-loading");
  mediaPreviewVideo.oncanplay = () => mediaFrame?.classList.remove("is-loading");
  mediaPreviewVideo.onerror = () => mediaFrame?.classList.remove("is-loading");
  mediaPreviewVideo.setAttribute(
    "aria-label",
    item?.isStory ? "Instagram video hikayesi önizlemesi" : "Video önizleme"
  );
  mediaPreviewVideo.src = source;
  mediaPreviewVideo.load();
}

function showQuotedMediaPreviewImage(source, cacheKey) {
  if (!mediaQuotedPreviewImage) return;
  resetQuotedMediaPreview();
  mediaQuotedPreviewImage.classList.remove("is-hidden");
  mediaQuotedPreviewImage.referrerPolicy = "no-referrer";
  mediaQuotedPreviewImage.dataset.previewUrl = cacheKey;
  mediaQuotedPreviewImage.onload = () => mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedPreviewImage.onerror = () => mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedPreviewImage.src = source;
  mediaQuotedFrame?.style.setProperty("--quoted-media-backdrop", `url("${source}")`);
}

function showQuotedMediaPreviewVideo(source, cacheKey) {
  if (!mediaQuotedPreviewVideo) return;
  resetQuotedMediaPreview();
  mediaQuotedPreviewVideo.classList.remove("is-hidden");
  mediaQuotedPreviewVideo.dataset.previewKey = cacheKey;
  mediaQuotedPreviewVideo.onloadedmetadata = () => mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedPreviewVideo.oncanplay = () => mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedPreviewVideo.onerror = () => mediaQuotedFrame?.classList.remove("is-loading");
  mediaQuotedPreviewVideo.src = source;
  mediaQuotedPreviewVideo.load();
}

function twitterQuotePreviewSlots() {
  const quote = normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote);
  if (!quote) return null;
  const quotedIndexes = new Set(
    quote.quotedMediaIndexes.filter((index) => index < currentMediaItems.length)
  );
  const selectedIsQuoted = quotedIndexes.has(currentMediaIndex);
  const outerIndex = selectedIsQuoted
    ? currentMediaItems.findIndex((_, index) => !quotedIndexes.has(index))
    : currentMediaIndex;
  const quotedIndex = selectedIsQuoted
    ? currentMediaIndex
    : currentMediaItems.findIndex((_, index) => quotedIndexes.has(index));

  return {
    outerItem: outerIndex >= 0 ? currentMediaItems[outerIndex] : null,
    quotedItem: quotedIndex >= 0 ? currentMediaItems[quotedIndex] : null,
  };
}

async function loadTwitterQuoteMediaSlot(item, slot, token) {
  if (!item) return;
  const policy = currentMediaPreviewPolicy(item);
  if (!policy.cacheKey) return;
  const frame = slot === "quoted" ? mediaQuotedFrame : mediaFrame;
  frame?.classList.add("is-loading");

  try {
    const source = await getCachedMediaPreviewDataUrl(item, {
      forcePrepare: policy.refreshAccessOnDisplay,
      policy,
    });
    if (token !== previewToken || !source) return;
    const isVideo = mediaItemType(item) === "video";
    if (slot === "quoted") {
      if (isVideo) showQuotedMediaPreviewVideo(source, policy.cacheKey);
      else showQuotedMediaPreviewImage(source, policy.cacheKey);
    } else if (isVideo) {
      showMediaPreviewVideo(source, policy.cacheKey, item);
    } else {
      if (mediaPreviewImage) {
        mediaPreviewImage.onload = () => mediaFrame?.classList.remove("is-loading");
        mediaPreviewImage.onerror = () => mediaFrame?.classList.remove("is-loading");
      }
      showMediaPreviewImage(source, policy.cacheKey);
    }
  } catch (error) {
    console.warn(`Twitter quote ${slot} preview preparation failed:`, error);
    if (token !== previewToken) return;
    frame?.classList.remove("is-loading");
    if (slot === "quoted") resetQuotedMediaPreview();
    else {
      resetMediaPreviewVideo();
      mediaPreviewImage?.removeAttribute("src");
    }
  }
}

async function loadTwitterQuotePreview(item) {
  const slots = twitterQuotePreviewSlots();
  if (!slots) return;
  const token = ++previewToken;
  renderMediaTweetCard(item || twitterMediaMetadataFromItem(), { hydrateAvatar: true, token });

  mediaStage?.classList.toggle("has-outer-media", Boolean(slots.outerItem));
  mediaStage?.classList.toggle("has-quoted-media", Boolean(slots.quotedItem));
  mediaFrame?.classList.toggle("is-hidden", !slots.outerItem);
  mediaQuotedFrame?.classList.toggle("is-hidden", !slots.quotedItem);
  if (!slots.outerItem) {
    resetMediaPreviewVideo();
    mediaPreviewImage?.removeAttribute("src");
  }
  if (!slots.quotedItem) resetQuotedMediaPreview();

  await Promise.all([
    loadTwitterQuoteMediaSlot(slots.outerItem, "outer", token),
    loadTwitterQuoteMediaSlot(slots.quotedItem, "quoted", token),
  ]);
}

async function loadMediaPreviewItem(item) {
  if (normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote)) {
    await loadTwitterQuotePreview(item);
    return;
  }
  const policy = currentMediaPreviewPolicy(item);
  if (!item || !policy.cacheKey) return;

  const token = ++previewToken;
  const cacheKey = policy.cacheKey;
  const cached = mediaPreviewCache.get(cacheKey)?.source || mediaPreviewCache.get(cacheKey)?.dataUrl || "";
  const isVideo = mediaItemType(item) === "video";
  renderMediaTweetCard(item, { hydrateAvatar: true, token });
  mediaFrame?.classList.toggle("is-loading", policy.refreshAccessOnDisplay || !cached);

  if (!isVideo && mediaPreviewImage) {
    mediaPreviewImage.onload = () => mediaFrame?.classList.remove("is-loading");
    mediaPreviewImage.onerror = () => mediaFrame?.classList.remove("is-loading");
  }

  if (cached && !policy.refreshAccessOnDisplay) {
    if (isVideo) showMediaPreviewVideo(cached, cacheKey, item);
    else showMediaPreviewImage(cached, cacheKey);
    return;
  }

  try {
    const source = await getCachedMediaPreviewDataUrl(item, {
      forcePrepare: policy.refreshAccessOnDisplay,
      policy,
    });
    if (token !== previewToken) return;
    if (!source) throw new Error("Medya önizleme kaynağı hazırlanamadı.");

    if (isVideo) showMediaPreviewVideo(source, cacheKey, item);
    else showMediaPreviewImage(source, cacheKey);
  } catch (error) {
    console.warn("Media preview preparation failed:", error);
    if (token !== previewToken) return;

    const legacySource = policy.allowLegacyFallback ? policy.legacySource : "";
    if (legacySource) {
      if (isVideo) showMediaPreviewVideo(legacySource, cacheKey, item);
      else showMediaPreviewImage(legacySource, cacheKey);
      return;
    }

    resetMediaPreviewVideo();
    mediaPreviewImage?.removeAttribute("src");
    mediaFrame?.classList.remove("is-loading");
  }
}

function updateMediaControls() {
  const item = selectedMediaItem();
  const active = Boolean(item);
  const idle = downloadState === "idle";
  const many = currentMediaItems.length > 1;
  const quoteMode = Boolean(normalizeTwitterQuoteContext(currentMediaAnalysis?.twitterQuote));

  if (mediaPrevBtn) {
    mediaPrevBtn.classList.toggle("is-hidden", !many || quoteMode);
    mediaPrevBtn.disabled = !many || !idle || currentMediaIndex <= 0;
    const previousItem = currentMediaItems[currentMediaIndex - 1];
    const previousLabel = previousItem
      ? `Önceki ${mediaItemKindLabel(previousItem).toLocaleLowerCase("tr-TR")} (${currentMediaIndex} / ${currentMediaItems.length})`
      : "Önceki medya yok";
    mediaPrevBtn.title = previousLabel;
    mediaPrevBtn.setAttribute("aria-label", previousLabel);
    mediaPrevBtn.setAttribute("aria-disabled", String(mediaPrevBtn.disabled));
  }

  if (mediaNextBtn) {
    mediaNextBtn.classList.toggle("is-hidden", !many || quoteMode);
    mediaNextBtn.disabled = !many || !idle || currentMediaIndex >= currentMediaItems.length - 1;
    const nextItem = currentMediaItems[currentMediaIndex + 1];
    const nextLabel = nextItem
      ? `Sonraki ${mediaItemKindLabel(nextItem).toLocaleLowerCase("tr-TR")} (${currentMediaIndex + 2} / ${currentMediaItems.length})`
      : "Sonraki medya yok";
    mediaNextBtn.title = nextLabel;
    mediaNextBtn.setAttribute("aria-label", nextLabel);
    mediaNextBtn.setAttribute("aria-disabled", String(mediaNextBtn.disabled));
  }

  for (const [button, source] of [
    [mediaQuotePrevBtn, mediaPrevBtn],
    [mediaQuoteNextBtn, mediaNextBtn],
  ]) {
    if (!button) continue;
    button.classList.toggle("is-hidden", !many || !quoteMode);
    button.disabled = source?.disabled ?? true;
    button.title = source?.title || "Medya yok";
    button.setAttribute("aria-label", button.title);
    button.setAttribute("aria-disabled", String(button.disabled));
  }

  if (downloadMediaPostBtn) {
    const showPostDownload =
      isTwitterTextPostMode() || Boolean(twitterMediaPostDownloadKind(currentPlatform, item));
    downloadMediaPostBtn.classList.toggle("is-hidden", !showPostDownload);
    downloadMediaPostBtn.disabled = !showPostDownload || !idle;
  }

  if (downloadMediaItemBtn) {
    downloadMediaItemBtn.classList.toggle("is-hidden", isTwitterTextPostMode());
    downloadMediaItemBtn.disabled = isTwitterTextPostMode() || !active || !idle;
    downloadMediaItemBtn.textContent = item?.isStory
      ? "Hikayeyi indir"
      : mediaItemType(item) === "video"
        ? "Videoyu indir"
        : "Fotoğrafı indir";
  }

  if (downloadMediaBatchBtn) {
    downloadMediaBatchBtn.classList.toggle("is-hidden", !many);
    downloadMediaBatchBtn.disabled = !many || !idle;
    const storyBatch = currentMediaItems.some((mediaItem) => mediaItem?.isStory);
    const videoBatch = currentMediaItems.some((mediaItem) => mediaItemType(mediaItem) === "video");
    downloadMediaBatchBtn.textContent = storyBatch
      ? "Tüm hikayeleri indir"
      : videoBatch
        ? "Tüm medyayı indir"
        : "Tüm fotoğrafları indir";
  }
}

function renderTwitterTextPostPreview() {
  resetVideoPreview();
  if (mediaPreviewLabel) mediaPreviewLabel.textContent = "X/Twitter gönderisi";
  if (mediaPreviewTitle) mediaPreviewTitle.textContent = "X/Twitter metin gönderisi";
  if (mediaPreviewMeta) mediaPreviewMeta.textContent = "Metin gönderisi";
  if (mediaPreviewBadge) mediaPreviewBadge.textContent = "X/Twitter · Metin";
  mediaPreview?.classList.remove("is-hidden");
  mediaPreview?.classList.add("is-twitter-card", "is-text-only");
  mediaPreview?.setAttribute("aria-hidden", "false");
  mediaStage?.classList.add("is-twitter-card", "is-text-only");
  mediaPreviewImage?.classList.add("is-hidden");
  resetMediaPreviewVideo();
  resetQuotedMediaPreview();
  mediaFrame?.classList.add("is-hidden");
  mediaQuotedFrame?.classList.add("is-hidden");
  renderMediaTweetCard(currentTwitterPostMetadata, { hydrateAvatar: true });
  downloadPanel?.classList.add("is-media-hidden");
  updateMediaControls();
  setMediaWindowSize();
}

function renderMediaPreview(analysis) {
  const normalizedAnalysis = normalizeMediaAnalysis(analysis);
  const normalized = normalizeMediaAnalysisItems(normalizedAnalysis);
  const items = normalized.items;

  appStore.dispatch({
    type: "analysis/succeeded",
    platform: normalizedAnalysis.platform,
    mediaAnalysis: normalizedAnalysis,
    items,
    index: normalized.initialIndex,
  });

  const quoteContext = normalizeTwitterQuoteContext(normalizedAnalysis.twitterQuote);
  if (quoteContext && !items.length) {
    currentTwitterTextOnly = true;
    currentTwitterPostMetadata = twitterMediaMetadataFromItem();
    renderTwitterTextPostPreview();
    return;
  }

  if (!mediaPreview || !items.length) {
    resetMediaPreview();
    return;
  }

  const item = selectedMediaItem();
  const title = mediaItemTitle(item, mediaItemKindLabel(item));
  const label = mediaItemKindLabel(item);

  if (mediaPreviewLabel) mediaPreviewLabel.textContent = `Seçili ${label.toLocaleLowerCase("tr-TR")}`;
  if (mediaPreviewTitle) {
    mediaPreviewTitle.textContent = title;
    mediaPreviewTitle.title = title;
  }
  if (mediaPreviewBadge) {
    mediaPreviewBadge.textContent = `${platformLabel(analysis?.platform || currentPlatform)} · ${label}`;
  }

  renderMediaPreviewPosition();
  mediaPreview.classList.remove("is-hidden");
  mediaPreview.setAttribute("aria-hidden", "false");
  downloadPanel?.classList.add("is-media-hidden");
  loadMediaPreviewItem(item);
  preloadMediaPreviewItems(items, currentMediaIndex);
  updateMediaControls();
  updateSelectedFormatLabel();
  setMediaWindowSize();
}

function renderMediaPreviewPosition() {
  const item = selectedMediaItem();
  if (!item) return;

  const label = mediaItemKindLabel(item);
  const title = mediaItemTitle(item, label);

  if (mediaPreviewLabel) mediaPreviewLabel.textContent = `Seçili ${label.toLocaleLowerCase("tr-TR")}`;
  if (mediaPreviewTitle) {
    mediaPreviewTitle.textContent = title;
    mediaPreviewTitle.title = title;
  }
  if (mediaPreviewMeta) {
    mediaPreviewMeta.textContent = `${currentMediaIndex + 1} / ${currentMediaItems.length} · ${mediaItemDetailsLabel(item)}`;
  }
  if (mediaPreviewBadge) {
    mediaPreviewBadge.textContent = `${platformLabel(mediaPreviewPlatform())} · ${label}`;
  }
  renderMediaTweetCard(item);
  updateMediaControls();
}

function moveMediaPreview(delta) {
  if (!currentMediaItems.length || downloadState !== "idle") return;

  const nextIndex = clampNumber(
    currentMediaIndex + delta,
    0,
    currentMediaItems.length - 1
  );

  if (nextIndex === currentMediaIndex) return;

  appStore.dispatch({
    type: "preview/selected",
    index: nextIndex,
    itemId: currentMediaItems[nextIndex]?.id,
  });
  const item = selectedMediaItem();
  renderMediaPreviewPosition();
  loadMediaPreviewItem(item);
  preloadMediaPreviewItems(currentMediaItems, currentMediaIndex);
  updateSelectedFormatLabel();
}

function resetVideoPreview() {
  if (!videoPreview || !videoThumb || !videoTitle || !videoMeta) return;

  previewToken += 1;
  videoPreview.classList.add("is-hidden");
  clearPreviewImage();
  videoTitle.textContent = "Video başlığı";
  videoMeta.textContent = "Süre bilgisi";
  resetQualityCard();
  refreshWindowLayout();
}

function renderVideoPreview(info, platform = "youtube") {
  if (!videoPreview || !videoThumb || !videoTitle || !videoMeta) return;

  const title = info.title || "Video";
  const duration = formatMediaDuration(info.duration);
  const channel = info.uploader || info.channel || platformLabel(platform);

  videoTitle.textContent = title;
  videoTitle.title = title;
  videoMeta.textContent = `${duration} · ${channel}`;

  loadPreviewThumbnail(info, platform);

  videoPreview.classList.remove("is-hidden");
  updateQualityCard();
  refreshWindowLayout();
}

function platformLabel(platform) {
  if (platform === "twitter") return "X/Twitter";
  if (platform === "instagram") return "Instagram";
  if (platform === "tiktok") return "TikTok";
  if (platform === "youtube") return "YouTube";
  return "Media";
}

function resetPlatformBadge() {
  if (!platformBadge) return;

  platformBadge.textContent = "Platform";
  platformBadge.className = "platform-badge is-hidden";
}

function setPlatformBadge(platform) {
  if (!platformBadge) return;

  platformBadge.textContent = platformLabel(platform);
  platformBadge.className = `platform-badge is-${platform}`;
}

function formatHistoryDate(value) {
  try {
    return new Intl.DateTimeFormat("tr-TR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));
  } catch {
    return "Tarih bilinmiyor";
  }
}

function readHistory() {
  return readDownloadHistory(localStorage, HISTORY_KEY);
}

function writeHistory(items) {
  return writeDownloadHistory(localStorage, HISTORY_KEY, items);
}

function removeHistoryItem(item) {
  writeHistory(removeDownloadHistoryItem(readHistory(), item));
  renderHistory();
}

function isMissingDownloadedFileError(text) {
  const lower = String(text || "").toLowerCase();
  return lower.includes("video bulunamadı") || lower.includes("dosya bulunamadı");
}

function addHistoryItem(item) {
  writeHistory(prependDownloadHistoryItem(readHistory(), item));
  renderHistory();
}

async function revealPath(path) {
  const clean = normalizePath(path);

  if (!clean) {
    message.textContent = "Gösterilecek dosya yolu bulunamadı.";
    message.className = "message is-error";
    return;
  }

  try {
    await invoke("reveal_path", { path: clean });
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
    window.alert(String(error));
  }
}

function notifyDownloadComplete(path, fileCount = 1) {
  const clean = normalizePath(path);
  if (!clean) return;

  void invoke("show_download_complete_notification", {
    filePath: clean,
    fileCount: Math.max(1, Number(fileCount || 1)),
  }).catch((error) => {
    console.warn("Windows indirme bildirimi gösterilemedi:", error);
  });
}

async function revealHistoryItem(item) {
  try {
    await invoke("reveal_download", {
      filePath: normalizePath(item.filePath),
      outputDir: normalizePath(item.outputDir),
      title: item.title || basename(item.filePath),
      downloadedAtMs: historyTimeMs(item),
      fileSize: Number(item.fileSize || 0) || null,
    });
  } catch (error) {
    console.error(error);

    const text = String(
      error || "Dosya bulunamadı. Çıktı dosyası taşınmış veya silinmiş olabilir."
    );

    message.textContent = text;
    message.className = "message is-error";

    window.alert(text);

    if (isMissingDownloadedFileError(text)) {
      removeHistoryItem(item);

      const sameAsLast =
        lastCompletedItem &&
        normalizePath(lastCompletedItem.filePath) === normalizePath(item.filePath);

      if (sameAsLast) {
        hideLastFileActions();
      }
    }
  }
}

function showLastFileActions(item) {
  const filePath = normalizePath(item?.filePath || item);

  lastCompletedFilePath = filePath;
  lastCompletedItem = item && typeof item === "object"
    ? item
    : {
        title: basename(filePath),
        platform: currentPlatform,
        quality: activeFormat?.quality || "Otomatik",
        url: currentUrl,
        filePath,
        outputDir: "",
        fileSize: 0,
        downloadedAtMs: Date.now(),
        downloadedAt: new Date().toISOString(),
      };

  if (!revealLastBtn || !lastCompletedFilePath) {
    return;
  }

  revealLastBtn.disabled = false;
  revealLastBtn.classList.remove("is-hidden");
}

function hideLastFileActions() {
  lastCompletedFilePath = "";
  lastCompletedItem = null;
  revealLastBtn?.classList.add("is-hidden");
  if (revealLastBtn) revealLastBtn.disabled = true;
}

function normalizeDownloadResult(result) {
  if (typeof result === "string") {
    return {
      message: result,
      filePath: "",
      outputDir: "",
      mode: "",
      fileSize: 0,
    };
  }

  return {
    message: result?.message || "İndirme tamamlandı.",
    filePath: result?.file_path || result?.filePath || "",
    outputDir: result?.output_dir || result?.outputDir || "",
    mode: result?.mode || "",
    fileSize: Number(result?.file_size || result?.fileSize || 0),
  };
}

function downloadSuccessSubject(args) {
  return args?.kind === "audio" ? "Ses" : "Video";
}

function downloadSuccessQuality(args) {
  const quality = String(args?.quality || activeFormat?.quality || "seçilen").trim();
  return quality || "seçilen";
}

function showDownloadSuccessMessage(result, args) {
  const targetPath = normalizePath(result?.filePath || result?.outputDir || "");
  const text = `${downloadSuccessSubject(args)} başarıyla ${downloadSuccessQuality(args)} kalitesinde indirildi.`;

  message.replaceChildren();

  if (!targetPath) {
    message.textContent = text;
    message.className = "message is-success";
    return;
  }

  const link = document.createElement("span");
  link.className = "message-action-link";
  link.textContent = "tıklayın";
  link.title = "Dosyayı klasörde göster";
  link.addEventListener("click", () => revealPath(targetPath));

  message.append(
    document.createTextNode(`${text} Klasörde göstermek için `),
    link,
    document.createTextNode(".")
  );
  message.className = "message is-success";
}

function showMediaDownloadResultMessage(result, args) {
  const outcome = mediaDownloadOutcome(result, args);
  const targetPath = normalizePath(mediaDownloadTarget(result, args));
  const failureDetails = Array.isArray(result?.failures)
    ? result.failures
        .map((failure, index) => `${index + 1}. ${failure?.message || "Medya indirilemedi."}`)
        .join("\n")
    : "";

  message.replaceChildren();
  message.title = failureDetails;

  if (!targetPath) {
    message.textContent = outcome.text;
    message.className = `message is-${outcome.status}`;
    return outcome;
  }

  const link = document.createElement("span");
  link.className = "message-action-link";
  link.textContent = "tıklayın";
  link.title = "İndirilen medyayı klasörde göster";
  link.addEventListener("click", () => revealPath(targetPath));

  message.append(
    document.createTextNode(`${outcome.text} Klasörde göstermek için `),
    link,
    document.createTextNode(".")
  );
  message.className = `message is-${outcome.status}`;
  return outcome;
}

function openHistoryPanel() {
  renderHistory();
  modalController.open("history-panel");
}

function closeHistoryPanel() {
  modalController.close("history-panel");
}

function setExtensionSetupMessage(text = "", error = false) {
  if (!extensionSetupMessage) return;
  extensionSetupMessage.textContent = text;
  extensionSetupMessage.classList.toggle("is-error", error);
}

function stopExtensionSetupPolling() {
  if (extensionSetupPollTimer) clearInterval(extensionSetupPollTimer);
  extensionSetupPollTimer = null;
}

function renderExtensionSetupSteps(browser, connected) {
  if (!extensionSetupSteps) return;
  const guide = extensionGuideForBrowser(browser?.id);
  const opened = Boolean(browser?.id && openedExtensionBrowserIds.has(browser.id));
  const states = extensionSetupStepStates({ selected: Boolean(browser), opened, connected });
  const browserLabel = browser?.label || "Tarayıcı";
  const manualNavigation = guide?.launchesInternalPage === false;
  const steps = [
    {
      title: browser ? `${browserLabel} seçildi` : "Tarayıcını seç",
      detail: browser?.defaultBrowser
        ? "Varsayılan tarayıcın otomatik olarak seçildi."
        : "Kurulu tarayıcılardan birini seç.",
    },
    {
      title: manualNavigation ? "Tarayıcıyı aç ve adresi gir" : "Eklenti sayfasını açmayı dene",
      detail: guide
        ? manualNavigation
          ? `${guide.page} panoya kopyalanır. Adres çubuğuna yapıştır veya ${guide.shortcut} kullan.`
          : `${guide.page} tarayıcıda açılmak üzere gönderilir.`
        : "Devam etmek için desteklenen bir Chromium tarayıcısı seç.",
    },
    {
      title: "Eklentiyi yükle veya yenile",
      detail: guide
        ? `${guide.developerMode} ${guide.loadUnpacked} Yolu kopyala veya Klasörü göster ile doğru dizini seç.`
        : "Seçtiğin tarayıcıya uygun adımlar burada gösterilecek.",
    },
    {
      title: connected ? "Bağlantı doğrulandı" : "Bağlantı otomatik doğrulansın",
      detail: connected
        ? "Tarayıcı companion'ı MediaDrop ile güvenli biçimde bağlandı."
        : "Eklenti yüklenince bu adım kendiliğinden tamamlanır.",
    },
  ];
  const statusLabels = { complete: "Tamam", current: "Şimdi", pending: "Bekliyor" };

  extensionSetupSteps.replaceChildren(...steps.map((step, index) => {
    const item = document.createElement("li");
    const state = states[index] || "pending";
    item.className = `is-${state}`;
    item.dataset.stepState = state;

    const title = document.createElement("strong");
    title.textContent = step.title;
    const detail = document.createElement("span");
    detail.textContent = step.detail;
    const status = document.createElement("em");
    status.textContent = statusLabels[state];
    item.append(title, detail, status);
    return item;
  }));
}

function renderExtensionSetupInfo(info = {}) {
  extensionSetupInfo = info;
  const path = String(info.extensionPath || "").trim();
  const browsers = orderExtensionBrowsers(Array.isArray(info.browsers) ? info.browsers : []);
  const installed = browsers.filter((browser) => browser?.installed === true);
  const selectedStillAvailable = installed.some(
    (browser) => browser.id === selectedExtensionBrowserId
  );
  if (!selectedStillAvailable) {
    selectedExtensionBrowserId =
      installed[0]?.id || "";
  }

  if (extensionSetupPath) {
    extensionSetupPath.textContent = path || "Eklenti klasörü bulunamadı";
    extensionSetupPath.title = path;
  }
  if (extensionCopyPathBtn) extensionCopyPathBtn.disabled = !path;
  if (extensionRevealPathBtn) extensionRevealPathBtn.disabled = !path;
  if (extensionSetupOpenBtn) extensionSetupOpenBtn.disabled = !selectedExtensionBrowserId || !path;
  if (extensionBrowserHint) {
    extensionBrowserHint.textContent = installed.length
      ? `${installed.length} kurulu tarayıcı bulundu`
      : "Desteklenen Chromium tarayıcısı bulunamadı";
  }

  const selectedBrowser = browsers.find((browser) => browser.id === selectedExtensionBrowserId) || null;
  renderExtensionSetupSteps(selectedBrowser, info.connected === true);
  if (extensionSetupOpenBtn) {
    const guide = extensionGuideForBrowser(selectedBrowser?.id);
    extensionSetupOpenBtn.textContent = selectedBrowser
      ? guide?.launchesInternalPage === false
        ? `${selectedBrowser.label} tarayıcısını aç`
        : `${selectedBrowser.label} ile sayfayı aç`
      : "Tarayıcıyı aç";
  }

  extensionBrowserList?.replaceChildren();
  for (const browser of browsers) {
    const option = document.createElement("button");
    option.type = "button";
    option.className = "extension-browser-option";
    option.dataset.browserId = browser.id;
    option.disabled = browser.installed !== true;
    option.setAttribute("role", "radio");
    option.setAttribute("aria-checked", String(browser.id === selectedExtensionBrowserId));
    option.classList.toggle("is-selected", browser.id === selectedExtensionBrowserId);

    const label = document.createElement("strong");
    label.textContent = browser.label || browser.id;
    const detail = document.createElement("small");
    detail.textContent = browser.installed
      ? browser.defaultBrowser
        ? "Varsayılan tarayıcı"
        : browser.recommended
          ? "Önerilen"
          : "Kurulu"
      : "Bulunamadı";
    option.append(label, detail);
    extensionBrowserList?.appendChild(option);
  }

  if (extensionConnectionStatus) {
    const connected = info.connected === true;
    extensionConnectionStatus.classList.toggle("is-connected", connected);
    const title = extensionConnectionStatus.querySelector("strong");
    const detail = extensionConnectionStatus.querySelector("small");
    if (title) title.textContent = connected ? "Eklenti hazır" : "Bağlantı bekleniyor";
    if (detail) {
      detail.textContent = connected
        ? "Tarayıcı companion'ı MediaDrop ile güvenli biçimde bağlandı."
        : "Eklenti yüklendiğinde bağlantı burada otomatik doğrulanır.";
    }
    if (connected) {
      stopExtensionSetupPolling();
      setExtensionSetupMessage("Kurulum tamamlandı. Eklentiyi kullanabilirsin.");
    }
  }
}

async function refreshExtensionSetupInfo({ quiet = false } = {}) {
  try {
    const info = await invoke("get_extension_setup_info");
    renderExtensionSetupInfo(info);
    return info;
  } catch (error) {
    if (!quiet) {
      setExtensionSetupMessage(parseBackendError(error).message, true);
    }
    return null;
  }
}

async function openExtensionSetup() {
  modalController.open("extension-setup");
  setExtensionSetupMessage("Kurulum dosyaları ve tarayıcılar kontrol ediliyor.");
  const info = await refreshExtensionSetupInfo();
  if (!info) return;
  if (!info.connected) {
    stopExtensionSetupPolling();
    extensionSetupPollTimer = setInterval(
      () => void refreshExtensionSetupInfo({ quiet: true }),
      1500
    );
  }
}

function closeExtensionSetup() {
  stopExtensionSetupPolling();
  modalController.close("extension-setup");
}

async function copyExtensionSetupPath() {
  const path = String(extensionSetupInfo?.extensionPath || "").trim();
  if (!path) return false;
  try {
    await writeClipboardText(path);
    setExtensionSetupMessage("Eklenti klasörü panoya kopyalandı.");
    return true;
  } catch (error) {
    setExtensionSetupMessage(parseBackendError(error).message, true);
    return false;
  }
}

async function revealExtensionSetupPath() {
  const path = String(extensionSetupInfo?.extensionPath || "").trim();
  if (!path) return false;
  const manifestPath = `${path.replace(/[\\/]+$/, "")}\\manifest.json`;
  try {
    await invoke("reveal_path", { path: manifestPath });
    setExtensionSetupMessage("Eklenti klasörü açıldı; manifest.json seçili gösteriliyor.");
    return true;
  } catch (error) {
    setExtensionSetupMessage(parseBackendError(error).message, true);
    return false;
  }
}

async function launchExtensionSetup() {
  if (!selectedExtensionBrowserId || extensionSetupOpenBtn?.disabled) return;
  if (extensionSetupOpenBtn) extensionSetupOpenBtn.disabled = true;
  const guide = extensionGuideForBrowser(selectedExtensionBrowserId);
  try {
    if (guide?.launchesInternalPage === false) {
      await writeClipboardText(guide.page);
    }
    const info = await invoke("open_extension_setup", {
      browserId: selectedExtensionBrowserId,
    });
    openedExtensionBrowserIds.add(selectedExtensionBrowserId);
    renderExtensionSetupInfo(info);
    setExtensionSetupMessage(
      guide?.launchesInternalPage === false
        ? `${guide.page} panoya kopyalandı ve tarayıcı açıldı. Adresi yapıştır veya ${guide.shortcut} kullan.`
        : `Tarayıcı açıldı. ${guide?.page || "Eklenti sayfası"} görünmediyse adresi elle gir; bağlantı otomatik doğrulanacak.`
    );
  } catch (error) {
    setExtensionSetupMessage(parseBackendError(error).message, true);
  } finally {
    if (extensionSetupOpenBtn) extensionSetupOpenBtn.disabled = !selectedExtensionBrowserId;
  }
}

function renderHistory() {
  if (!historyList) return;

  const history = readHistory();
  historyList.innerHTML = "";

  if (!history.length) {
    const empty = document.createElement("p");
    empty.className = "history-empty";
    empty.textContent = "Henüz indirme geçmişi yok.";
    historyList.appendChild(empty);
    return;
  }

  for (const item of history) {
    const row = document.createElement("article");
    row.className = "history-item";

    const titleWrap = document.createElement("div");
    titleWrap.className = "history-title";

    const title = document.createElement("strong");
    title.textContent = item.title || basename(item.filePath);
    title.title = item.title || basename(item.filePath);

    const path = document.createElement("small");
    path.textContent = item.filePath || item.outputDir || "Dosya yolu yok";
    path.title = item.filePath || item.outputDir || "";

    const meta = document.createElement("div");
    meta.className = "history-meta";

    const platform = document.createElement("span");
    platform.className = "history-chip";
    platform.textContent = platformLabel(item.platform);

    const quality = document.createElement("span");
    quality.className = "history-chip";
    quality.textContent = item.quality || "Otomatik";

    const date = document.createElement("span");
    date.className = "history-chip";
    date.textContent = formatHistoryDate(item.downloadedAt);

    meta.append(platform, quality, date);
    titleWrap.append(title, path, meta);

    const actions = document.createElement("div");
    actions.className = "history-item-actions";

    const reveal = document.createElement("button");
    reveal.type = "button";
    reveal.className = "mini-action-btn";
    reveal.textContent = "Klasörde Göster";
    reveal.addEventListener("click", () => revealHistoryItem(item));

    actions.appendChild(reveal);
    row.append(titleWrap, actions);
    historyList.appendChild(row);
  }
}

function showToolsOverlay(text = "İndirme araçları kontrol ediliyor.") {
  if (!toolsOverlay) return;
  if (toolsStatus) toolsStatus.textContent = text;
  modalController.open("tools-overlay");
}

function hideToolsOverlay() {
  modalController.close("tools-overlay");
}

async function checkToolsUpdateOnStartup() {
  const last = Number(localStorage.getItem(TOOLS_UPDATE_CHECK_KEY) || "0");
  const now = Date.now();

  if (Number.isFinite(last) && now - last < TOOLS_UPDATE_INTERVAL_MS) {
    return;
  }

  localStorage.setItem(TOOLS_UPDATE_CHECK_KEY, String(now));

  let overlayTimer = null;

  try {
    overlayTimer = setTimeout(() => {
      showToolsOverlay("Eklentiler kontrol ediliyor...");
    }, 700);

    const result = await invoke("update_ytdlp");

    clearTimeout(overlayTimer);
    overlayTimer = null;

    if (result?.updated) {
      showToolsOverlay("İndirme araçları güncellendi. Uygulama yeniden başlatılıyor...");

      setTimeout(async () => {
        try {
          await relaunch();
        } catch (error) {
          console.warn(error);
          hideToolsOverlay();
        }
      }, 1100);

      return;
    }

    hideToolsOverlay();
  } catch (error) {
    if (overlayTimer) clearTimeout(overlayTimer);
    console.warn("Download tools auto update failed:", error);
    hideToolsOverlay();
  }
}


function setDownloadState(state) {
  appStore.dispatch({ type: "download/status", status: state });

  if (!downloadBtn) return;

  downloadBtn.classList.remove("is-pause", "is-resume", "is-busy");

  updateClipControls();
  updateQualityCard();
  updateMediaControls();

  if (state === "idle") {
    downloadBtn.textContent = isMediaPhotoMode()
      ? selectedMediaItem()?.isStory ? "Hikayeyi İndir" : "Fotoğrafı İndir"
      : currentPlatform === "twitter" ? "Videoyu İndir" : "İndir";
    downloadBtn.disabled = !canDownloadActiveFormat();
    cancelBtn?.classList.add("is-hidden");
    if (cancelBtn) cancelBtn.disabled = true;
    analyzeBtn.disabled = false;
    return;
  }

  if (state === "downloading") {
    downloadBtn.textContent = "Duraklat";
    downloadBtn.disabled = false;
    downloadBtn.classList.add("is-pause");
    cancelBtn?.classList.remove("is-hidden");
    if (cancelBtn) cancelBtn.disabled = false;
    analyzeBtn.disabled = true;
    return;
  }

  if (state === "pausing") {
    downloadBtn.textContent = "Duraklatılıyor";
    downloadBtn.disabled = true;
    downloadBtn.classList.add("is-busy");
    cancelBtn?.classList.remove("is-hidden");
    if (cancelBtn) cancelBtn.disabled = false;
    analyzeBtn.disabled = true;
    return;
  }

  if (state === "paused") {
    downloadBtn.textContent = "Devam Et";
    downloadBtn.disabled = false;
    downloadBtn.classList.add("is-resume");
    cancelBtn?.classList.remove("is-hidden");
    if (cancelBtn) cancelBtn.disabled = false;
    analyzeBtn.disabled = true;
    return;
  }

  if (state === "canceling") {
    downloadBtn.textContent = "İptal ediliyor";
    downloadBtn.disabled = true;
    downloadBtn.classList.add("is-busy");
    cancelBtn?.classList.remove("is-hidden");
    if (cancelBtn) cancelBtn.disabled = true;
    analyzeBtn.disabled = true;
  }
}

function isDownloadControlError(error, code) {
  return String(error || "").includes(code);
}

function finishCancelledDownload() {
  message.textContent = "İndirme iptal edildi. Yarım dosyalar temizlendi.";
  message.className = "message";
  resetProgress();
  lastDownloadArgs = null;
  lastMediaDownloadArgs = null;
  setDownloadState("idle");
}

function isClipDownloadActive(args = lastDownloadArgs) {
  return Boolean(args && args.clipStartSeconds !== null && args.clipEndSeconds !== null);
}

function resetProgress() {
  hideLastFileActions();
  twitterPostProgressFloor = 0;

  if (!progressWrap || !progressFill || !progressText || !progressLine) return;

  progressWrap.classList.add("is-hidden");
  progressFill.style.width = "0%";
  progressText.textContent = "0%";
  progressLine.textContent = "Hazır.";
  progressWrap.setAttribute("aria-valuenow", "0");
  progressWrap.setAttribute("aria-valuetext", "Hazır");
  progressWrap.setAttribute("aria-hidden", "true");
  progressWrap.setAttribute("aria-busy", "false");
  refreshWindowLayout();
}

function showProgress() {
  if (!progressWrap) return;
  progressWrap.classList.remove("is-hidden");
  progressWrap.setAttribute("aria-hidden", "false");
  progressWrap.setAttribute("aria-busy", "true");
  refreshWindowLayout();
}

function updateProgressAccessibility(percent, detail = "") {
  if (!progressWrap) return;

  const numeric = Number(percent);
  const hasPercent = percent !== null && percent !== undefined && Number.isFinite(numeric);
  const safePercent = hasPercent ? Math.max(0, Math.min(100, numeric)) : null;
  const cleanDetail = String(detail || "").trim();

  if (safePercent === null) {
    progressWrap.removeAttribute("aria-valuenow");
    progressWrap.setAttribute("aria-valuetext", cleanDetail || "İşleniyor");
    progressWrap.setAttribute("aria-busy", "true");
    return;
  }

  progressWrap.setAttribute("aria-valuenow", safePercent.toFixed(1));
  progressWrap.setAttribute(
    "aria-valuetext",
    cleanDetail ? `${safePercent.toFixed(1)}% · ${cleanDetail}` : `${safePercent.toFixed(1)}%`
  );
  progressWrap.setAttribute("aria-busy", String(safePercent < 100));
}

function updateClipDownloadMessage(payload = {}) {
  if (!isClipDownloadActive() || downloadState !== "downloading") return;

  const quality = downloadSuccessQuality(lastDownloadArgs);
  message.textContent = `${quality} klip: ${clipDownloadStatusText(payload)}`;
  message.className = "message";
}

function isTwitterPostProgressPayload(payload = {}) {
  const text = `${payload.phase || ""} ${payload.line || ""}`.toLowerCase();
  return (
    text.includes("gönderi videosu") ||
    text.includes("gönderi kartı") ||
    text.includes("mp4 oluşturuluyor") ||
    (twitterPostProgressFloor > 0 && text.includes("tamamlandı"))
  );
}

function setProgress(progressOrPercent, line = "") {
  if (!progressWrap || !progressFill || !progressText || !progressLine) return;

  showProgress();

  const payload =
    typeof progressOrPercent === "object" && progressOrPercent !== null
      ? progressOrPercent
      : {
          percent: progressOrPercent,
          line,
          downloaded_mb: null,
          total_mb: null,
          speed_mb: null,
          phase: "",
        };

  const percent = asNumber(payload.percent);
  const downloadedMb = asNumber(payload.downloaded_mb);
  const totalMb = asNumber(payload.total_mb);
  const speedMb = asNumber(payload.speed_mb);
  const phase = payload.phase || "";
  const rawLine = payload.line || line || "";
  const clipActive = isClipDownloadActive();
  let displayedPercent = null;

  if (percent !== null) {
    let safePercent = displayProgressPercent(percent, rawLine, clipActive);

    if (isTwitterPostProgressPayload(payload)) {
      safePercent = Math.max(safePercent, twitterPostProgressFloor);
      twitterPostProgressFloor = safePercent;
    }

    progressFill.style.width = `${safePercent}%`;
    progressText.textContent = `${safePercent.toFixed(1)}%`;
    displayedPercent = safePercent;
  } else if (clipActive && phase) {
    progressText.textContent = "İşleniyor";
  }

  const parts = [];

  if (!clipActive && downloadedMb !== null && totalMb !== null) {
    parts.push(`${downloadedMb.toFixed(1)} / ${totalMb.toFixed(1)} MB`);
  }

  if (!clipActive && speedMb !== null) {
    parts.push(`${speedMb.toFixed(2)} MB/s`);
  }

  if (parts.length) {
    progressLine.textContent = parts.join(" • ");
    updateProgressAccessibility(displayedPercent, progressLine.textContent);
    return;
  }

  if (phase) {
    progressLine.textContent = phase;
    updateProgressAccessibility(displayedPercent, phase);
    return;
  }

  const fallbackText = parseFallbackProgressLine(rawLine, percent);
  if (fallbackText) {
    progressLine.textContent = fallbackText;
  }
  updateProgressAccessibility(displayedPercent, fallbackText || progressLine.textContent);
}

listen("download-progress", (event) => {
  const payload = event.payload || {};
  const jobId = progressJobId(payload);
  if (jobId && downloadState !== "idle") {
    appStore.dispatch({ type: "download/job", jobId });
  }
  setProgress(payload);
  updateClipDownloadMessage(payload);
});

function shouldStartDrag(event) {
  if (event.button !== 0) return false;
  if (event.target.closest("button")) return false;
  if (event.target.closest("input")) return false;
  return true;
}

async function startWindowDrag(event) {
  if (!shouldStartDrag(event)) return;

  const dragMode = currentWindowLayoutMode;

  try {
    await invoke("start_dragging");
    sampleWindowPositionAfterDrag(dragMode);
  } catch (error) {
    console.error(error);
  }
}

titlebar?.addEventListener("pointerdown", startWindowDrag);
document.querySelector(".clip-editor-head")?.addEventListener("pointerdown", startWindowDrag);

window.addEventListener("beforeunload", () => {
  saveWindowPosition(currentWindowLayoutMode);
});

minimizeBtn?.addEventListener("click", async () => {
  try {
    await invoke("minimize_window");
  } catch (error) {
    console.error(error);
  }
});

closeBtn?.addEventListener("click", async () => {
  try {
    await saveWindowPosition(currentWindowLayoutMode);
    await invoke("close_window");
  } catch (error) {
    console.error(error);
  }
});

folderBtn?.addEventListener("click", chooseDownloadFolder);

cloudReportsToggle?.addEventListener("change", async () => {
  await setCloudReportsEnabled(Boolean(cloudReportsToggle.checked));
});

async function checkForAppUpdates() {
  if (!updateBtn) return;

  updateBtn.disabled = true;
  updateBtn.textContent = "Kontrol";

  message.textContent = "Güncelleme kontrol ediliyor...";
  message.className = "message";

  try {
    const update = await checkForUpdate();

    if (!update) {
      message.textContent = "Uygulama güncel.";
      message.className = "message is-success";
      updateBtn.textContent = "Güncel";

      setTimeout(() => {
        updateBtn.textContent = "Güncelle";
      }, 1800);

      return;
    }

    const notes = update.body ? `\n\n${update.body}` : "";
    const shouldInstall = window.confirm(
      `Yeni sürüm bulundu: ${update.version}${notes}\n\nŞimdi indirip kurulsun mu?`
    );

    if (!shouldInstall) {
      message.textContent = "Güncelleme iptal edildi.";
      message.className = "message";
      return;
    }

    let downloadedBytes = 0;
    let totalBytes = 0;

    showProgress();

    setProgress({
      percent: 0,
      downloaded_mb: 0,
      total_mb: null,
      speed_mb: null,
      phase: "Güncelleme indiriliyor...",
      line: "",
    });

    updateBtn.textContent = "İndiriliyor";
    message.textContent = `MediaDrop ${update.version} indiriliyor...`;
    message.className = "message";

    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        downloadedBytes = 0;
        totalBytes = Number(event.data?.contentLength || 0);

        setProgress({
          percent: 0,
          downloaded_mb: 0,
          total_mb: bytesToMb(totalBytes),
          speed_mb: null,
          phase: "Güncelleme başladı...",
          line: "",
        });
      }

      if (event.event === "Progress") {
        downloadedBytes += Number(event.data?.chunkLength || 0);

        const downloadedMb = bytesToMb(downloadedBytes);
        const totalMb = bytesToMb(totalBytes);
        const percent =
          totalBytes > 0 ? (downloadedBytes / totalBytes) * 100 : null;

        setProgress({
          percent,
          downloaded_mb: downloadedMb,
          total_mb: totalMb,
          speed_mb: null,
          phase: "Güncelleme indiriliyor...",
          line: "",
        });
      }

      if (event.event === "Finished") {
        setProgress({
          percent: 100,
          downloaded_mb: null,
          total_mb: null,
          speed_mb: null,
          phase: "Güncelleme kuruldu. Yeniden başlatılıyor...",
          line: "",
        });
      }
    });

    message.textContent = "Güncelleme kuruldu. Uygulama yeniden başlatılıyor...";
    message.className = "message is-success";

    await relaunch();
  } catch (error) {
    console.error(error);
    message.textContent = `Güncelleme hatası: ${String(error)}`;
    message.className = "message is-error";
  } finally {
    updateBtn.disabled = false;
    updateBtn.textContent = "Güncelle";
  }
}

updateBtn?.addEventListener("click", checkForAppUpdates);

function resetQualityCard() {
  availableFormats = [];
  qualityPickerIndex = 0;
  qualityPickerDraftIndex = 0;

  if (qualityCard) {
    qualityCard.classList.add("is-hidden");
    qualityCard.disabled = true;
  }

  if (qualityCardValue) qualityCardValue.textContent = "-";
  if (qualityCardDetail) qualityCardDetail.textContent = "Analiz bekleniyor";

  closeQualityPicker();
}

function formatSummaryText(format) {
  if (!format) return "Analiz bekleniyor";
  return `${format.title} · ${format.detail || "Otomatik"}`;
}

function updateQualityCard() {
  if (!qualityCard || !qualityCardValue || !qualityCardDetail) return;

  if (isMediaPhotoMode()) {
    qualityCard.classList.add("is-hidden");
    qualityCard.disabled = true;
    return;
  }

  if (!activeFormat) {
    qualityCard.classList.add("is-hidden");
    qualityCard.disabled = true;
    qualityCardValue.textContent = "-";
    qualityCardDetail.textContent = "Analiz bekleniyor";
    return;
  }

  qualityCard.classList.remove("is-hidden");
  qualityCard.disabled = availableFormats.length <= 1 || downloadState !== "idle";
  qualityCardValue.textContent = activeFormat.quality || "Best";
  qualityCardDetail.textContent = formatSummaryText(activeFormat);
}

function selectFormat(format) {
  const previousSignature = getFormatSignature();
  activeFormat = format || null;
  const nextSignature = getFormatSignature();

  if (previousSignature && nextSignature && previousSignature !== nextSignature) {
    destroyClipPlayer();
  }

  if (!isClipCompatibleFormat(format) && clipSelection) {
    clearClipSelection({ silent: true });
  }

  updateSelectedFormatLabel();
  updateClipControls();
  updateQualityCard();

  if (downloadState === "idle") {
    downloadBtn.disabled = !canDownloadActiveFormat();
  }

  refreshWindowLayout();
}

function renderFormats(formats) {
  availableFormats = Array.isArray(formats) ? formats : [];

  if (formatList) {
    formatList.innerHTML = "";
    formatList.classList.add("is-hidden");
    formatList.hidden = true;
  }

  if (!availableFormats.length) {
    activeFormat = null;
    updateQualityCard();
    if (selectedFormat) selectedFormat.textContent = "Uygun format bulunamadı";
    return;
  }

  const autoIndex = availableFormats.findIndex((format) => format.autoSelect);
  const selectedIndex = autoIndex >= 0 ? autoIndex : 0;

  qualityPickerIndex = selectedIndex;
  qualityPickerDraftIndex = selectedIndex;
  selectFormat(availableFormats[selectedIndex]);
}

function openQualityPicker() {
  if (!qualityPicker || !availableFormats.length || downloadState !== "idle") return;

  qualityPickerDraftIndex = Math.max(
    0,
    availableFormats.findIndex((format) => format === activeFormat)
  );

  if (qualityPickerDraftIndex < 0) qualityPickerDraftIndex = 0;

  renderQualityPicker();
  modalController.open("quality-picker");
}

function closeQualityPicker() {
  if (!qualityPicker) return;

  modalController.close("quality-picker");
}

function moveQualityPicker(delta) {
  if (!availableFormats.length) return;

  qualityPickerDraftIndex = clampNumber(
    qualityPickerDraftIndex + delta,
    0,
    availableFormats.length - 1
  );

  renderQualityPicker();
}

function renderQualityPicker() {
  const format = availableFormats[qualityPickerDraftIndex];

  if (!format) return;

  if (qualityFocusIndex) {
    qualityFocusIndex.textContent = `${qualityPickerDraftIndex + 1} / ${availableFormats.length}`;
  }

  if (qualityFocusValue) {
    qualityFocusValue.textContent = format.quality || "Best";
  }

  if (qualityFocusDetail) {
    qualityFocusDetail.textContent = formatSummaryText(format);
  }

  if (qualityFocusSize) {
    qualityFocusSize.textContent = format.size || "Boyut bilinmiyor";
  }

  if (qualityPrevBtn) {
    qualityPrevBtn.disabled = qualityPickerDraftIndex <= 0;
  }

  if (qualityNextBtn) {
    qualityNextBtn.disabled = qualityPickerDraftIndex >= availableFormats.length - 1;
  }
}

function applyQualityPickerSelection() {
  const format = availableFormats[qualityPickerDraftIndex];

  if (!format) return;

  qualityPickerIndex = qualityPickerDraftIndex;
  selectFormat(format);
  closeQualityPicker();

  message.textContent = `Kalite seçildi: ${format.quality}`;
  message.className = "message";
}

function isSocialPlatform(platform) {
  return platform === "twitter" || platform === "instagram" || platform === "tiktok";
}

function hasMediaAnalysisItems(analysis) {
  const hasItems = Array.isArray(analysis?.items)
    && analysis.items.some((item) => mediaItemHasPreview(item, analysis));
  return hasItems || Boolean(normalizeTwitterQuoteContext(analysis?.twitterQuote));
}

async function tryAnalyzeMedia(value, authMode = mediaAuthModeForUrl(value)) {
  const analysis = await invoke("analyze_media", {
    url: value,
    authMode,
  });

  return hasMediaAnalysisItems(analysis) ? analysis : null;
}

function applyMediaAnalysis(mediaAnalysis, urlPlatform, authMode) {
  const normalizedAnalysis = normalizeMediaAnalysis(mediaAnalysis);
  confirmPendingInstagramCookieConsent(authMode);
  currentInfo = null;
  currentMediaAuthMode = mediaAuthModeAfterSuccessfulAnalysis(authMode);
  const platform = normalizedAnalysis.platform || urlPlatform;
  appStore.dispatch({
    type: "auth/changed",
    mode: currentMediaAuthMode,
    status: "ready",
  });
  currentTwitterPostMetadata = null;
  setPlatformBadge(platform);
  renderFormats([]);
  resetVideoPreview();
  renderMediaPreview(normalizedAnalysis);
  updateClipControls();
  setDownloadState("idle");
  const analysisMessage = normalizedAnalysis.contentKind === "story"
    ? `${platformLabel(platform)} hikayesi analiz edildi.`
    : `${platformLabel(platform)} medyası analiz edildi.`;
  const warningMessages = mediaAnalysisWarningMessages(normalizedAnalysis);
  const cookiePrepareNotice = instagramAuthController.takePrepareNotice();
  message.textContent = [analysisMessage, ...warningMessages, cookiePrepareNotice]
    .filter(Boolean)
    .join(" ");
  message.className = warningMessages.length ? "message is-warning" : "message is-success";
}

function applyVideoAnalysisInfo(value, info, mediaAnalysisError = null, { silentNoVideo = false } = {}) {
  if (!info || typeof info !== "object" || Array.isArray(info)) {
    if (silentNoVideo) return false;
    throw new Error("Analiz sonucu boş döndü. Bu bağlantıda indirilebilir medya bulunamadı.");
  }

  const platform = detectPlatform(value, info);

  if (platform === "generic") {
    if (silentNoVideo) return false;
    currentTwitterPostMetadata = null;
    message.textContent = SUPPORTED_LINK_MESSAGE;
    message.className = "message is-error";
    formatList.innerHTML = "";
    formatList.classList.add("is-hidden");
    formatList.hidden = true;
    resetPlatformBadge();
    resetVideoPreview();
    resetMediaPreview();
    appStore.dispatch({
      type: "analysis/failed",
      error: shouldShowMediaError ? mediaAnalysisError : error,
    });
    return true;
  }

  resetMediaPreview({ resize: false });
  currentInfo = info;
  appStore.dispatch({
    type: "analysis/succeeded",
    platform,
    info,
    mediaAnalysis: null,
    items: [],
    index: 0,
  });
  currentTwitterPostMetadata =
    platform === "twitter" ? normalizeTwitterPostMetadata(info, value) : null;
  setPlatformBadge(platform);
  const formats = buildFormatCards(info, platform);
  const selectedFormatCandidate = formats.find((format) => format.autoSelect) || formats[0] || null;
  const socialHasVideo =
    platform === "twitter"
      ? hasTwitterPostDownloadTarget(info, selectedFormatCandidate)
      : platform === "instagram" || platform === "tiktok"
        ? hasSocialDownloadTarget(info, selectedFormatCandidate)
        : false;
  const twitterTextOnly =
    platform === "twitter" && twitterTextPostAvailable(currentTwitterPostMetadata, socialHasVideo);

  if (
    (platform === "twitter" || platform === "instagram" || platform === "tiktok") &&
    !socialHasVideo &&
    !twitterTextOnly
  ) {
    if (silentNoVideo) return false;
    renderFormats([]);
    resetVideoPreview();
    resetMediaPreview();
    updateClipControls();
    if (mediaAnalysisError) {
      showErrorMessage(mediaAnalysisError);
    } else {
      message.textContent = `Bu ${platformLabel(platform)} linkinde indirilebilir fotoğraf, hikaye veya video bulunamadı.`;
      message.className = "message is-error";
    }
    return true;
  }

  if (twitterTextOnly) {
    currentTwitterTextOnly = true;
    renderFormats([]);
    renderTwitterTextPostPreview();
    message.textContent = "X/Twitter metin gönderisi analiz edildi.";
    message.className = "message is-success";
    updateClipControls();
    return true;
  }

  if (platform === "twitter") {
    message.textContent = socialHasVideo
      ? `"${info.title || "X/Twitter videosu"}" analiz edildi. En iyi kalite otomatik seçildi.`
      : "Bu X/Twitter gönderisinde indirilebilir video bulunamadı.";
  } else if (platform === "instagram") {
    message.textContent = `"${info.title || "Instagram videosu"}" analiz edildi. En iyi kalite otomatik seçildi.`;
  } else if (platform === "tiktok") {
    message.textContent = `"${info.title || "TikTok videosu"}" analiz edildi. En iyi kalite otomatik seçildi.`;
  } else {
    message.textContent = `"${info.title || "Video"}" analiz edildi.`;
  }

  message.className = platform === "twitter" && !socialHasVideo
    ? "message is-error"
    : "message is-success";
  renderVideoPreview(info, platform);
  renderFormats(formats);
  updateClipControls();
  return true;
}

function applyCompanionHandoff(payload) {
  const sourceUrl = String(payload?.sourceUrl || "").trim();
  const autoDownloadTwitterPost = isTwitterPostDownloadIntent(payload?.intent);
  if (!sourceUrl || !["video", "media", "reanalyze"].includes(payload?.kind)) return false;
  if (payload.kind === "reanalyze") {
    urlInput.value = sourceUrl;
    updateUrlClearButton();
    void analyzeUrl({ forceInstagramAuth: true });
    return true;
  }
  if (payload.kind === "media" && !payload.analysis) return false;
  if (payload.kind === "video" && !payload.info) return false;

  const token = ++analysisToken;
  urlInput.value = sourceUrl;
  currentUrl = sourceUrl;
  currentInfo = null;
  currentMediaAuthMode = PUBLIC_MEDIA_AUTH_MODE;
  currentTwitterPostMetadata = null;
  currentTwitterTextOnly = false;
  lastMediaDownloadArgs = null;
  activeFormat = null;
  clearClipSelection({ silent: true });
  appStore.dispatch({ type: "analysis/started", token, url: sourceUrl });

  if (payload.kind === "media") {
    applyMediaAnalysis(
      payload.analysis,
      detectPlatform(sourceUrl, payload.analysis),
      PUBLIC_MEDIA_AUTH_MODE,
    );
  } else {
    applyVideoAnalysisInfo(sourceUrl, payload.info);
  }

  analyzeBtn.disabled = false;
  updateUrlClearButton();
  if (autoDownloadTwitterPost && ["media", "video"].includes(payload.kind)) {
    setTimeout(() => void startMediaPostCardDownload(), 0);
  }
  return true;
}

let companionHandoffRecovery = null;

async function restoreCompanionHandoff() {
  if (companionHandoffRecovery) return companionHandoffRecovery;
  companionHandoffRecovery = (async () => {
    try {
      const payload = await invoke("take_companion_handoff");
      return applyCompanionHandoff(payload);
    } catch {
      return false;
    }
  })();
  try {
    return await companionHandoffRecovery;
  } finally {
    companionHandoffRecovery = null;
  }
}

void listen("companion-analysis-ready", () => {
  void restoreCompanionHandoff();
});

function isBrowserAuthError(error, platform) {
  const code = String(parseBackendError(error)?.code || "").trim();
  return platform === "twitter"
    ? code === "twitter_auth_required" || code === "twitter_auth_failed"
    : code === "youtube_auth_required" || code === "youtube_auth_failed";
}

async function requestCookieBrowser(platform, { error = null, avoidBrowserId = "" } = {}) {
  const requestPermission = platform === "twitter"
    ? instagramAuthController.requestTwitterCookiePermission
    : instagramAuthController.requestYoutubeCookiePermission;
  const permission = await requestPermission({
    avoidBrowserId,
    error,
  });
  if (!permission.allowed) {
    throw new Error(
      platform === "twitter"
        ? "X/Twitter oturum iznini reddettiniz. Gönderi medyası alınamadı."
        : "YouTube oturum iznini reddettiniz. Yaş kısıtlı video indirilemedi."
    );
  }
  return permission;
}

async function analyzeVideoWithCookieBrowser(value, browserId, platform) {
  let restartBrowser = false;
  let forceClose = false;
  let browserLabel = "Tarayıcı";

  try {
    const state = await invoke("get_cookie_browser_runtime_state", { browserId });
    browserLabel = state?.label || browserLabel;
  } catch (error) {
    console.warn(`${platformLabel(platform)} cookie browser state could not be read:`, error);
  }

  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      return await invoke("analyze_video", {
        url: value,
        cookieBrowser: browserId,
        restartBrowser,
        forceClose,
      });
    } catch (error) {
      const code = String(parseBackendError(error)?.code || "").trim();
      if (code === "browser_restart_required" && !restartBrowser) {
        const allowed = await instagramAuthController.requestBrowserRestartConfirmation({
          browserLabel,
          force: false,
        });
        if (!allowed) {
          throw new Error("Tarayıcı yeniden başlatma iznini reddettiniz. Video analiz edilemedi.");
        }
        restartBrowser = true;
        continue;
      }
      if (code === "browser_still_running" && restartBrowser && !forceClose) {
        const allowed = await instagramAuthController.requestBrowserRestartConfirmation({
          browserLabel,
          force: true,
        });
        if (!allowed) {
          throw new Error("Tarayıcıyı zorla kapatma iznini reddettiniz. Video analiz edilemedi.");
        }
        forceClose = true;
        continue;
      }
      throw error;
    }
  }

  throw new Error(`${platformLabel(platform)} oturum cookie'si hazırlanamadı.`);
}

async function analyzeVideoWithBrowserAuth(value) {
  const platform = detectPlatform(value);
  try {
    const jsonText = await invoke("analyze_video", {
      url: value,
      cookieBrowser: null,
      restartBrowser: false,
      forceClose: false,
    });
    return { jsonText, mediaAuthMode: "" };
  } catch (error) {
    if (
      !["youtube", "twitter"].includes(platform) ||
      !isBrowserAuthError(error, platform)
    ) {
      throw error;
    }
  }

  const saved = platform === "twitter"
    ? instagramAuthController.savedTwitterCookieConsent()
    : instagramAuthController.savedYoutubeCookieConsent();
  let browserId = saved?.browserId || "";
  let remember = Boolean(browserId);

  for (let attempt = 0; attempt < 2; attempt += 1) {
    if (!browserId) {
      const permission = await requestCookieBrowser(platform);
      browserId = permission.browserId;
      remember = permission.remember;
    }

    try {
      const jsonText = await analyzeVideoWithCookieBrowser(value, browserId, platform);
      if (remember) {
        if (platform === "twitter") {
          instagramAuthController.saveTwitterCookieConsent(browserId);
        } else {
          instagramAuthController.saveYoutubeCookieConsent(browserId);
        }
      }
      return {
        jsonText,
        mediaAuthMode: platform === "twitter" ? "registered:twitter" : "",
      };
    } catch (error) {
      if (!isBrowserAuthError(error, platform)) throw error;
      if (platform === "twitter") {
        instagramAuthController.clearTwitterCookieConsent();
      } else {
        instagramAuthController.clearYoutubeCookieConsent();
      }
      if (attempt >= 1) throw error;

      const failedBrowserId = browserId;
      const permission = await requestCookieBrowser(platform, {
        error,
        avoidBrowserId: failedBrowserId,
      });
      browserId = permission.browserId;
      remember = permission.remember;
    }
  }

  throw new Error(`${platformLabel(platform)} oturumu doğrulanamadı.`);
}

async function analyzeUrl(options = {}) {
  const value = urlInput.value.trim();
  const forceInstagramAuth = options?.forceInstagramAuth === true;

  if (["downloading", "pausing", "paused", "canceling"].includes(downloadState)) {
    message.textContent = "Devam eden veya duraklatılmış indirme varken yeni analiz yapılamaz.";
    message.className = "message is-error";
    return;
  }

  if (!value) {
    message.textContent = "Önce bir bağlantı yapıştır.";
    message.className = "message is-error";
    return;
  }

  if (!isSupportedMediaLink(value)) {
    message.textContent = SUPPORTED_LINK_MESSAGE;
    message.className = "message is-error";
    return;
  }

  const token = ++analysisToken;
  appStore.dispatch({ type: "analysis/started", token, url: value });

  currentUrl = value;
  currentInfo = null;
  currentMediaAuthMode = PUBLIC_MEDIA_AUTH_MODE;
  instagramAuthController.clearPrepareNotice();
  currentTwitterPostMetadata = null;
  currentTwitterTextOnly = false;
  lastMediaDownloadArgs = null;
  appStore.dispatch({ type: "auth/changed", mode: PUBLIC_MEDIA_AUTH_MODE, status: "idle" });
  destroyClipPlayer();
  resetPlatformBadge();
  activeFormat = null;
  clearClipSelection({ silent: true });
  selectedFormat.textContent = "Henüz seçim yok";
  downloadBtn.disabled = true;
  setDownloadState("idle");
  analyzeBtn.disabled = true;
  formatList.innerHTML = "";
  formatList.classList.add("is-hidden");
  formatList.hidden = true;
  resetProgress();
  resetVideoPreview();
  resetMediaPreview();

  message.textContent = "Analiz ediliyor...";
  message.className = "message";
  let mediaAnalysisError = null;

  try {
    const urlPlatform = detectPlatform(value);
    if (isSocialPlatform(urlPlatform) && shouldTryMediaInventory(urlPlatform, value)) {
      let initialAuthMode = mediaAuthModeForUrl(value);
      const recoveryState = { refreshAttempts: 0, promptAttempts: 0 };
      try {
        if (urlPlatform === "instagram") {
          const saved = await savedInstagramCookieConsent();
          const policyMode = instagramInitialAuthMode({
            isStory: isInstagramStoryUrl(value),
            hasSavedCookies: Boolean(saved?.hasSavedCookies),
            forcePrompt: forceInstagramAuth,
            publicMode: PUBLIC_MEDIA_AUTH_MODE,
            savedMode: SAVED_INSTAGRAM_AUTH_MODE,
          });
          if (policyMode) {
            initialAuthMode = policyMode;
          } else {
            const promptBudget = consumeInstagramAuthPromptBudget(recoveryState);
            recoveryState.promptAttempts = promptBudget.promptAttempts;
            if (!promptBudget.allowed) {
              throw new Error("Instagram izin penceresi bu analiz için daha önce gösterildi.");
            }
            initialAuthMode = await instagramCookieAuthMode({
              forcePrompt: forceInstagramAuth,
              error: forceInstagramAuth
                ? { code: "instagram_auth_required", message: "Instagram oturumu yenilenmeli." }
                : null,
            });
          }
        } else {
          initialAuthMode = mediaAuthModeForUrl(value);
        }
        if (token !== analysisToken) return;

        let mediaAnalysis = await tryAnalyzeMedia(value, initialAuthMode);
        if (token !== analysisToken) return;

        if (mediaAnalysis) {
          let successfulAuthMode = initialAuthMode;
          if (
            urlPlatform === "instagram" &&
            initialAuthMode === PUBLIC_MEDIA_AUTH_MODE &&
            instagramAnalysisNeedsAvatarAuth(mediaAnalysis)
          ) {
            const promptBudget = consumeInstagramAuthPromptBudget(recoveryState);
            recoveryState.promptAttempts = promptBudget.promptAttempts;
            if (promptBudget.allowed) {
              try {
                const avatarAuthMode = await instagramCookieAuthMode({ forcePrompt: true });
                if (token !== analysisToken) return;
                const authenticatedAnalysis = await tryAnalyzeMedia(value, avatarAuthMode);
                if (authenticatedAnalysis) {
                  mediaAnalysis = authenticatedAnalysis;
                  successfulAuthMode = avatarAuthMode;
                }
              } catch (avatarError) {
                console.warn("Instagram avatar auth retry failed; keeping public preview:", avatarError);
              }
            }
          }

          applyMediaAnalysis(mediaAnalysis, urlPlatform, successfulAuthMode);
          return;
        }
      } catch (error) {
        if (urlPlatform === "instagram") {
          if (token !== analysisToken) return;

          let recoveryError = error;
          let recoveryAuthMode = initialAuthMode;
          if (
            initialAuthMode === SAVED_INSTAGRAM_AUTH_MODE
            && isInstagramAuthRecoverableError(error)
          ) {
            const saved = await savedInstagramCookieConsent();
            const refreshBrowserId = saved?.browserId || "";
            const recoveryStep = nextInstagramAuthRecoveryStep({
              isAuthError: true,
              authMode: recoveryAuthMode,
              savedMode: SAVED_INSTAGRAM_AUTH_MODE,
              hasRefreshBrowser: Boolean(refreshBrowserId),
              refreshAttempts: recoveryState.refreshAttempts,
              promptAttempts: recoveryState.promptAttempts,
            });
            if (recoveryStep === "refresh") {
              recoveryState.refreshAttempts += 1;
              try {
                const preparedRefresh = await prepareInstagramCookieAuthFromPermission({
                  allowed: true,
                  browserId: refreshBrowserId,
                  remember: true,
                });
                const refreshAuthMode = preparedRefresh.authMode;
                recoveryAuthMode = refreshAuthMode;
                const mediaAnalysis = await tryAnalyzeMedia(value, refreshAuthMode);
                if (token !== analysisToken) return;

                if (mediaAnalysis) {
                  applyMediaAnalysis(mediaAnalysis, urlPlatform, refreshAuthMode);
                  return;
                }
              } catch (refreshError) {
                recoveryError = refreshError;
                mediaAnalysisError = refreshError;
                console.warn("Saved Instagram cookies could not be refreshed:", refreshError);
                if (isInstagramAuthRecoverableError(refreshError)) {
                  clearInstagramCookieConsent({ clearBackend: false });
                }
              }
            }
          }

          const parsed = parseBackendError(error);
          if (String(parsed.message || "").includes("Çerez izinlerini reddettiniz")) {
            showErrorMessage(error);
            formatList.innerHTML = "";
            formatList.classList.add("is-hidden");
            formatList.hidden = true;
            resetVideoPreview();
            resetMediaPreview();
            updateClipControls();
            return;
          }

          const promptStep = nextInstagramAuthRecoveryStep({
            isAuthError: isInstagramAuthRecoverableError(recoveryError),
            authMode: recoveryAuthMode,
            savedMode: SAVED_INSTAGRAM_AUTH_MODE,
            hasRefreshBrowser: false,
            refreshAttempts: recoveryState.refreshAttempts,
            promptAttempts: recoveryState.promptAttempts,
          });
          if (promptStep === "prompt") {
            const promptBudget = consumeInstagramAuthPromptBudget(recoveryState);
            recoveryState.promptAttempts = promptBudget.promptAttempts;
            if (!promptBudget.allowed) {
              mediaAnalysisError = recoveryError;
              console.warn("Instagram auth prompt budget exhausted for this analysis.");
              throw recoveryError;
            }
            clearInstagramCookieConsent({ clearBackend: false });
            const failedBrowserId = browserIdFromInstagramAuthMode(recoveryAuthMode);
            try {
              const retryAuthMode = await instagramCookieAuthMode({
                forcePrompt: true,
                error: recoveryError,
                avoidBrowserId: failedBrowserId,
              });
              if (token !== analysisToken) return;

              const mediaAnalysis = await tryAnalyzeMedia(value, retryAuthMode);
              if (token !== analysisToken) return;

              if (mediaAnalysis) {
                applyMediaAnalysis(mediaAnalysis, urlPlatform, retryAuthMode);
                return;
              }
            } catch (retryError) {
              if (token !== analysisToken) return;
              const retryParsed = parseBackendError(retryError);
              if (String(retryParsed.message || "").includes("Çerez izinlerini reddettiniz")) {
                showErrorMessage(retryError);
                formatList.innerHTML = "";
                formatList.classList.add("is-hidden");
                formatList.hidden = true;
                resetVideoPreview();
                resetMediaPreview();
                updateClipControls();
                return;
              }

              if (isInstagramAuthRecoverableError(retryError)) {
                clearInstagramCookieConsent({ clearBackend: false });
              }
              mediaAnalysisError = retryError;
              console.warn("Instagram cookie media analysis retry failed:", retryError);
            }
          }
          if (!mediaAnalysisError) {
            mediaAnalysisError = recoveryError;
          }
          console.warn("Instagram cookie media analysis failed:", error);
        } else {
          mediaAnalysisError = error;
          console.warn("Photo media analysis failed, falling back to video:", error);
        }
      }
    }

    if (!supportsLegacyVideoFallback(urlPlatform)) {
      const instagramAnalysisError = mediaAnalysisError || {
        code: "instagram_media_analysis_empty",
        message: "Instagram analizi indirilebilir bir gönderi, Reel, video veya hikaye döndürmedi.",
      };
      showErrorMessage(instagramAnalysisError);
      formatList.innerHTML = "";
      formatList.classList.add("is-hidden");
      formatList.hidden = true;
      resetVideoPreview();
      resetMediaPreview();
      updateClipControls();
      return;
    }

    const videoAnalysis = await analyzeVideoWithBrowserAuth(value);
    if (token !== analysisToken) return;

    if (urlPlatform === "twitter" && videoAnalysis.mediaAuthMode) {
      try {
        const authenticatedMedia = await tryAnalyzeMedia(value, videoAnalysis.mediaAuthMode);
        if (token !== analysisToken) return;
        if (authenticatedMedia) {
          applyMediaAnalysis(authenticatedMedia, urlPlatform, videoAnalysis.mediaAuthMode);
          return;
        }
      } catch (error) {
        mediaAnalysisError = error;
        console.warn("Authenticated X/Twitter media analysis failed; using video metadata:", error);
      }
    }

    const info = JSON.parse(videoAnalysis.jsonText);
    applyVideoAnalysisInfo(value, info, mediaAnalysisError);
  } catch (error) {
    if (token !== analysisToken) return;

    console.error(error);
    currentTwitterPostMetadata = null;
    const urlPlatform = detectPlatform(value);
    const videoErrorText = String(error || "").toLowerCase();
    const shouldShowMediaError =
      isSocialPlatform(urlPlatform) &&
      mediaAnalysisError &&
      (videoErrorText.includes("no video") ||
        videoErrorText.includes("format") ||
        videoErrorText.includes("uygun format") ||
        videoErrorText.includes("indirilebilir video"));

    showErrorMessage(shouldShowMediaError ? mediaAnalysisError : error);
    formatList.innerHTML = "";
    formatList.classList.add("is-hidden");
    formatList.hidden = true;
    resetPlatformBadge();
    resetVideoPreview();
    resetMediaPreview();
  } finally {
    if (token === analysisToken) {
      analyzeBtn.disabled = false;
    }
  }
}

analyzeBtn.addEventListener("click", analyzeUrl);

urlInput.addEventListener("input", updateUrlClearButton);

urlInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    analyzeUrl();
  }
});

urlClearBtn?.addEventListener("click", () => {
  urlInput.value = "";
  updateUrlClearButton();
  urlInput.focus();
});

window.addEventListener("focus", autofillUrlFromClipboard);

async function startOrResumeDownload() {
  if (!activeFormat && !lastDownloadArgs) return;

  const wasPaused = downloadState === "paused";
  const clipPayload = clipPayloadForDownload();

  const args =
    lastDownloadArgs || {
      url: currentUrl,
      formatId: activeFormat.id,
      kind: activeFormat.type,
      quality: activeFormat.quality,
      outputDir: selectedOutputDir || null,
      fastMode: false,
      title: currentInfo?.title || null,
      clipStartSeconds: clipPayload?.start ?? null,
      clipEndSeconds: clipPayload?.end ?? null,
    };

  lastDownloadArgs = args;
  appStore.dispatch({
    type: "download/arguments",
    args,
    mediaArgs: lastMediaDownloadArgs,
  });

  setDownloadState("downloading");

  showProgress();

  setProgress({
    percent: wasPaused ? null : 0,
    downloaded_mb: null,
    total_mb: null,
    speed_mb: null,
    phase: wasPaused ? "İndirme devam ettiriliyor..." : "İndirme başlatılıyor...",
    line: "",
  });

  const label = activeFormat
    ? `${activeFormat.title} ${args.quality || activeFormat.quality}`
    : "Seçilen medya";
  const clipLabel = args.clipStartSeconds !== null && args.clipEndSeconds !== null
    ? ` · Klip ${formatClipTime(args.clipStartSeconds)}-${formatClipTime(args.clipEndSeconds)}`
    : "";

  message.textContent = isClipDownloadActive(args)
    ? `${downloadSuccessQuality(args)} klip: Klip hazırlanıyor...`
    : `${label}${clipLabel} indiriliyor...`;
  message.className = "message";

  try {
    const result = await invoke("download_video", args);
    if (downloadState === "canceling") {
      finishCancelledDownload();
      return;
    }
    const normalized = normalizeDownloadResult(result);

    setProgress({
      percent: 100,
      downloaded_mb: null,
      total_mb: null,
      speed_mb: null,
      phase: "Tamamlandı.",
      line: "",
    });

    let completedItem = null;
    if (normalized.filePath) {
      completedItem = {
        title: currentInfo?.title || basename(normalized.filePath),
        platform: currentPlatform,
        quality: args.clipStartSeconds !== null && args.clipEndSeconds !== null
          ? `${args.quality || activeFormat?.quality || "Otomatik"} · Klip ${formatClipTime(args.clipStartSeconds)}-${formatClipTime(args.clipEndSeconds)}`
          : args.quality || activeFormat?.quality || "Otomatik",
        url: args.url || currentUrl,
        filePath: normalized.filePath,
        outputDir: normalized.outputDir,
        fileSize: normalized.fileSize,
        downloadedAtMs: Date.now(),
        downloadedAt: new Date().toISOString(),
      };

      showLastFileActions(completedItem);
      addHistoryItem(completedItem);
    }

    showDownloadSuccessMessage(completedItem || normalized, args);
    notifyDownloadComplete(normalized.filePath || normalized.outputDir, 1);

    lastDownloadArgs = null;
    setDownloadState("idle");
  } catch (error) {
    console.error(error);

    if (downloadState === "canceling" || isDownloadControlError(error, "__MEDIADROP_CANCELLED__")) {
      finishCancelledDownload();
      return;
    }

    if (isDownloadControlError(error, "__MEDIADROP_PAUSED__")) {
      message.textContent = "İndirme duraklatıldı. Devam Et ile kaldığı yerden sürdürebilirsin.";
      message.className = "message";
      setDownloadState("paused");
      return;
    }

    const parsedError = showErrorMessage(error);
    const fallbackArgs = buildFallbackDownloadArgs(args, parsedError?.fallbackOffer);

    if (fallbackArgs) {
      const fallbackLabel = parsedError.fallbackOffer.label || "1080p HLS klip indir";
      const wantsFallback = window.confirm(
        `4K/2K klip bu yöntemle indirilemedi.\n\n${fallbackLabel} denensin mi?`
      );

      if (wantsFallback) {
        lastDownloadArgs = fallbackArgs;
        setDownloadState("idle");
        await startOrResumeDownload();
        return;
      }
    }

    setDownloadState("idle");
  }
}

async function startOrResumeMediaDownload() {
  if (!ensureCurrentMediaAnalysisFresh()) return;
  const item = selectedMediaItem();
  if (!item && !lastMediaDownloadArgs) return;

  const wasPaused = downloadState === "paused";
  const args =
    lastMediaDownloadArgs || {
      mode: "item",
      analysisId: mediaAnalysisId(),
      itemId: metadataString(item?.id),
      itemType: mediaItemType(item),
      isStory: Boolean(item?.isStory),
      outputDir: selectedOutputDir || null,
    };

  lastMediaDownloadArgs = args;
  appStore.dispatch({
    type: "download/arguments",
    args: lastDownloadArgs,
    mediaArgs: args,
  });
  setDownloadState("downloading");
  showProgress();
  setProgress({
    percent: wasPaused ? null : 0,
    downloaded_mb: null,
    total_mb: null,
    speed_mb: null,
    phase: wasPaused ? "İndirme devam ettiriliyor..." : "İndirme başlatılıyor...",
    line: "",
  });

  const isBatch = args.mode === "batch";
  const batchIsStory = Boolean(args.scope === "all-stories");
  const batchHasVideo = Boolean(args.hasVideo);
  message.textContent = isBatch
    ? batchIsStory
      ? "Tüm hikayeler indiriliyor..."
      : batchHasVideo
        ? "Tüm medya indiriliyor..."
        : "Tüm fotoğraflar indiriliyor..."
    : item?.isStory
      ? "Hikaye indiriliyor..."
      : mediaItemType(item) === "video"
        ? "Video indiriliyor..."
        : "Fotoğraf indiriliyor...";
  message.className = "message";

  try {
    const completion = await executeInstagramDownloadWithRecovery({
      executeDownload: () => isBatch
        ? invoke("download_media_batch", buildMediaBatchDownloadRequest(args))
        : invoke("download_media_item", buildMediaItemDownloadRequest(args)),
      requestAuth: async (authError) => {
        clearInstagramCookieConsent({ clearBackend: false });
        return instagramCookieAuthMode({
          forcePrompt: true,
          error: authError,
          avoidBrowserId: browserIdFromInstagramAuthMode(currentMediaAuthMode),
        });
      },
      refreshAnalysis: async (authMode) => {
        const refreshed = await tryAnalyzeMedia(currentUrl, authMode);
        if (!refreshed) throw new Error("Instagram analizi yenilenemedi.");
        applyMediaAnalysis(refreshed, "instagram", authMode);
      },
    });

    if (completion.recovered) {
      lastMediaDownloadArgs = null;
      setDownloadState("idle");
      return;
    }

    const result = completion.result;

    if (downloadState === "canceling") {
      finishCancelledDownload();
      return;
    }

    const normalized = normalizeMediaDownloadResult(result);
    const outcome = mediaDownloadOutcome(normalized, args);
    setProgress({
      percent: 100,
      downloaded_mb: null,
      total_mb: null,
      speed_mb: null,
      phase: outcome.status === "warning" ? "Kısmen tamamlandı." : outcome.status === "error" ? "İndirilemedi." : "Tamamlandı.",
      line: "",
    });

    if (outcome.status === "error") {
      showMediaDownloadResultMessage(normalized, args);
      lastMediaDownloadArgs = null;
      setDownloadState("idle");
      return;
    }

    const completedItem = {
      title: isBatch
        ? `${currentMediaAnalysis?.title || "Medya"} (${normalized.downloadedCount} öğe)`
        : currentMediaAnalysis?.title || item?.title || basename(normalized.filePath),
      platform: currentPlatform,
      quality: isBatch
        ? normalized.failedCount > 0
          ? `${normalized.downloadedCount} indirildi · ${normalized.failedCount} başarısız`
          : `${normalized.downloadedCount} öğe`
        : item?.isStory ? "Hikaye" : mediaItemType(item) === "video" ? "Video" : "Fotoğraf",
      url: currentUrl,
      filePath: normalized.filePath || normalized.outputDir,
      outputDir: normalized.outputDir,
      fileSize: normalized.fileSize,
      downloadedAtMs: Date.now(),
      downloadedAt: new Date().toISOString(),
    };

    showLastFileActions(completedItem);
    addHistoryItem(completedItem);
    showMediaDownloadResultMessage(normalized, args);
    const notificationTarget = mediaDownloadTarget(normalized, args);
    if (notificationTarget) {
      notifyDownloadComplete(notificationTarget, isBatch ? normalized.downloadedCount : 1);
    }

    lastMediaDownloadArgs = null;
    setDownloadState("idle");
  } catch (error) {
    console.error(error);

    if (downloadState === "canceling" || isDownloadControlError(error, "__MEDIADROP_CANCELLED__")) {
      finishCancelledDownload();
      return;
    }

    if (isDownloadControlError(error, "__MEDIADROP_PAUSED__")) {
      message.textContent = "İndirme duraklatıldı. Devam Et ile tekrar başlatabilirsin.";
      message.className = "message";
      setDownloadState("paused");
      return;
    }

    showErrorMessage(error);
    lastMediaDownloadArgs = null;
    setDownloadState("idle");
  }
}

async function startMediaBatchDownload() {
  if (!isMediaPhotoMode() || currentMediaItems.length <= 1 || downloadState !== "idle") return;

  const isStoryBatch = currentMediaItems.some((item) => item?.isStory);
  const hasVideo = currentMediaItems.some((item) => mediaItemType(item) === "video");
  const photoCount = currentMediaItems.filter((item) => mediaItemType(item) === "photo").length;
  const videoCount = currentMediaItems.length - photoCount;
  const storyCount = currentMediaItems.filter((item) => item?.isStory).length;

  lastMediaDownloadArgs = {
    mode: "batch",
    analysisId: mediaAnalysisId(),
    scope: isStoryBatch ? "all-stories" : "all",
    hasVideo,
    photoCount,
    videoCount,
    storyCount,
    outputDir: selectedOutputDir || null,
  };
  appStore.dispatch({
    type: "download/arguments",
    args: lastDownloadArgs,
    mediaArgs: lastMediaDownloadArgs,
  });

  await startOrResumeMediaDownload();
}

async function startMediaPostCardDownload() {
  if (!ensureCurrentMediaAnalysisFresh()) return;
  const item = selectedMediaItem();
  const textOnly = isTwitterTextPostMode();
  if ((!item && !textOnly) || !isTwitterMediaPreview() || downloadState !== "idle") return;

  if (twitterMediaPostDownloadKind(currentPlatform, item) === "video") {
    currentTwitterPostMetadata = {
      ...twitterMediaMetadataFromItem(item),
      duration: Number(item.durationMs || 0) / 1000,
      quality: item.height ? `${item.height}p` : "Otomatik",
    };
    await startTwitterPostDownload();
    return;
  }

  setDownloadState("downloading");
  showProgress();
  setProgress({
    percent: 0,
    downloaded_mb: null,
    total_mb: null,
    speed_mb: null,
    phase: "Gönderi kartı hazırlanıyor...",
    line: "",
  });
  message.textContent = "X/Twitter gönderi kartı hazırlanıyor...";
  message.className = "message";

  try {
    const rendered = textOnly
      ? await renderTwitterTextPostCardPng()
      : await renderTwitterPhotoPostCardPng(item);
    const result = await invoke("download_media_post_card", {
      url: currentUrl,
      imageDataUrl: rendered.dataUrl,
      title: rendered.title,
      outputDir: selectedOutputDir || null,
    });
    const normalized = normalizeMediaDownloadResult(result);

    setProgress({
      percent: 100,
      downloaded_mb: null,
      total_mb: null,
      speed_mb: null,
      phase: "Gönderi kartı indirildi.",
      line: "",
    });

    const completedItem = {
      title: rendered.title || currentMediaAnalysis?.title || "X/Twitter gönderisi",
      platform: "twitter",
      quality: "Gönderi kartı PNG",
      url: currentUrl,
      filePath: normalized.filePath,
      outputDir: normalized.outputDir,
      fileSize: normalized.fileSize,
      downloadedAtMs: Date.now(),
      downloadedAt: new Date().toISOString(),
    };

    showLastFileActions(completedItem);
    addHistoryItem(completedItem);
    showTwitterPhotoPostCardSuccessMessage(completedItem);
    notifyDownloadComplete(normalized.filePath || normalized.outputDir, 1);
    setDownloadState("idle");
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
    setDownloadState("idle");
  }
}

async function pauseCurrentDownload() {
  if (downloadState !== "downloading") return;

  setDownloadState("pausing");
  message.textContent = "İndirme duraklatılıyor...";
  message.className = "message";

  try {
    await invoke("pause_download", { jobId: activeDownloadJobId || null });
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
    setDownloadState("downloading");
  }
}

async function cancelCurrentDownload() {
  if (!["downloading", "pausing", "paused"].includes(downloadState)) return;

  const previousState = downloadState;
  const completion = downloadCancellationCompletion(previousState);
  setDownloadState("canceling");
  message.textContent = "İndirme iptal ediliyor ve yarım dosyalar temizleniyor...";
  message.className = "message";

  try {
    await invoke("cancel_download", { jobId: activeDownloadJobId || null });
    if (completion === "immediate") finishCancelledDownload();
  } catch (error) {
    console.error(error);
    showErrorMessage(error);
    setDownloadState(previousState === "paused" ? "paused" : "downloading");
  }
}

modalController.register("quality-picker", {
  element: qualityPicker,
  initialFocus: qualityCloseBtn,
  isBusy: () => downloadState !== "idle",
  onRequestClose: closeQualityPicker,
  onKeydown: (event) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      moveQualityPicker(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      moveQualityPicker(1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      applyQualityPickerSelection();
    }
  },
});
modalController.register("history-panel", {
  element: historyPanel,
  initialFocus: closeHistoryBtn,
  onRequestClose: closeHistoryPanel,
});
modalController.register("extension-setup", {
  element: extensionSetupOverlay,
  initialFocus: extensionSetupCloseBtn,
  onRequestClose: closeExtensionSetup,
});
modalController.register("clip-editor", {
  element: clipEditor,
  initialFocus: clipBackBtn,
  isBusy: () => downloadState !== "idle",
  onRequestClose: closeClipEditor,
});
modalController.register("instagram-cookie-permission", {
  element: cookieAuthOverlay,
  initialFocus: cookieDenyBtn,
  onRequestClose: () => instagramAuthController.cancelActiveDialog(),
});
modalController.register("instagram-browser-restart", {
  element: browserRestartOverlay,
  initialFocus: browserRestartDenyBtn,
  isBusy: () => instagramAuthController.isBrowserRestartBusy(),
  onRequestClose: () => instagramAuthController.cancelActiveDialog(),
});
modalController.register("tools-overlay", {
  element: toolsOverlay,
  isBusy: () => true,
  escapeEnabled: false,
});
modalController.attach();

clipBtn?.addEventListener("click", openClipEditor);
clipEditInlineBtn?.addEventListener("click", openClipEditor);
clipCancelInlineBtn?.addEventListener("click", () => clearClipSelection());
clipBackBtn?.addEventListener("click", closeClipEditor);
clipClearBtn?.addEventListener("click", () => {
  clearClipSelection();
  closeClipEditor();
});
clipSetStartBtn?.addEventListener("click", () => setClipInputToCurrent("start"));
clipSetEndBtn?.addEventListener("click", () => setClipInputToCurrent("end"));
clipPreviewBtn?.addEventListener("click", previewSelectedClip);
clipDoneBtn?.addEventListener("click", saveClipSelection);
postDownloadBtn?.addEventListener("click", startTwitterPostDownload);
mediaPrevBtn?.addEventListener("click", () => moveMediaPreview(-1));
mediaNextBtn?.addEventListener("click", () => moveMediaPreview(1));
mediaQuotePrevBtn?.addEventListener("click", () => moveMediaPreview(-1));
mediaQuoteNextBtn?.addEventListener("click", () => moveMediaPreview(1));
downloadMediaPostBtn?.addEventListener("click", startMediaPostCardDownload);
downloadMediaItemBtn?.addEventListener("click", () => startOrResumeMediaDownload());
downloadMediaBatchBtn?.addEventListener("click", startMediaBatchDownload);

qualityCard?.addEventListener("click", openQualityPicker);
qualityCloseBtn?.addEventListener("click", closeQualityPicker);
qualityPicker?.addEventListener("click", (event) => {
  if (event.target?.matches?.("[data-quality-close]")) {
    closeQualityPicker();
  }
});
qualityPrevBtn?.addEventListener("click", () => moveQualityPicker(-1));
qualityNextBtn?.addEventListener("click", () => moveQualityPicker(1));
qualitySelectBtn?.addEventListener("click", applyQualityPickerSelection);
window.addEventListener("keydown", (event) => {
  if (isMediaPhotoMode() && downloadState === "idle") {
    const tagName = event.target?.tagName?.toLowerCase?.() || "";
    if (tagName === "input" || tagName === "textarea") return;

    if (event.key === "ArrowLeft") {
      moveMediaPreview(-1);
    } else if (event.key === "ArrowRight") {
      moveMediaPreview(1);
    }
  }
});

[clipStartMin, clipStartSec, clipEndMin, clipEndSec].forEach((input) => {
  input?.addEventListener("input", () => {
    normalizeClipInputs();
    updateClipMarkers();
    validateClip(true);
  });
});

clipSeek?.addEventListener("input", () => {
  const value = Number(clipSeek.value || 0);
  if (!Number.isFinite(value)) return;

  if (clipCurrentTime) clipCurrentTime.textContent = formatClipTime(value);

  try {
    clipPlayer?.seekTo?.(value, true);
  } catch {}
});

clipPlayBtn?.addEventListener("click", toggleClipPlayback);

clipVolumeSlider?.addEventListener("input", () => {
  applyClipVolume(clipVolumeSlider.value);
});

updateClipVolumeUI();


downloadBtn.addEventListener("click", async () => {
  if (downloadState === "downloading") {
    await pauseCurrentDownload();
    return;
  }

  if (downloadState === "paused" || downloadState === "idle") {
    if (isMediaPhotoMode() || lastMediaDownloadArgs) {
      await startOrResumeMediaDownload();
    } else {
      await startOrResumeDownload();
    }
  }
});

cancelBtn?.addEventListener("click", cancelCurrentDownload);

historyBtn?.addEventListener("click", openHistoryPanel);
extensionSetupBtn?.addEventListener("click", openExtensionSetup);
extensionSetupCloseBtn?.addEventListener("click", closeExtensionSetup);
extensionSetupLaterBtn?.addEventListener("click", closeExtensionSetup);
extensionCopyPathBtn?.addEventListener("click", copyExtensionSetupPath);
extensionRevealPathBtn?.addEventListener("click", revealExtensionSetupPath);
extensionSetupOpenBtn?.addEventListener("click", launchExtensionSetup);
extensionSetupOverlay?.addEventListener("click", (event) => {
  if (event.target?.matches?.("[data-extension-setup-close]")) closeExtensionSetup();
});
extensionBrowserList?.addEventListener("click", (event) => {
  const option = event.target.closest("button[data-browser-id]");
  if (!option || option.disabled) return;
  selectedExtensionBrowserId = option.dataset.browserId || "";
  renderExtensionSetupInfo(extensionSetupInfo || {});
});
closeHistoryBtn?.addEventListener("click", closeHistoryPanel);
historyPanel?.addEventListener("click", (event) => {
  if (event.target?.matches?.("[data-history-close]")) {
    closeHistoryPanel();
  }
});

clearHistoryBtn?.addEventListener("click", () => {
  writeHistory([]);
  renderHistory();
});

revealLastBtn?.addEventListener("click", () => {
  if (lastCompletedItem) {
    revealHistoryItem(lastCompletedItem);
    return;
  }

  revealPath(lastCompletedFilePath);
});

setTimeout(checkToolsUpdateOnStartup, 1200);

void listen("open-extension-setup", () => void openExtensionSetup()).catch((error) => {
  console.warn("Extension setup event could not be attached:", error);
});

initializeWindowPlacement();
initializeDynamicWindowLayout();
updateAppVersionBadge();
formatList.innerHTML = "";
formatList.classList.add("is-hidden");
formatList.hidden = true;
resetProgress();
resetVideoPreview();
resetMediaPreview({ resize: false });
updateFolderLabel();
updateCloudReportsFromBackend();
setTimeout(flushPendingCloudReportsOnStartup, 2200);
resetPlatformBadge();
renderHistory();
setDownloadState("idle");
updateUrlClearButton();
setTimeout(async () => {
  if (await invoke("take_extension_setup_request").catch(() => false)) {
    await openExtensionSetup();
    return;
  }
  if (!(await restoreCompanionHandoff())) {
    await autofillUrlFromClipboard();
  }
}, 0);
