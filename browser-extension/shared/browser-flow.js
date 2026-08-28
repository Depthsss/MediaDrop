import { buildSourcePayload } from "./source-candidates.js";

const ATTENTION = new Set(["ready", "busy", "completed", "needs_user", "error", "invalid_request", "unsupported", "version_mismatch"]);
const PENDING = new Set(["accepted", "app_starting", "connecting", "analyzing", "downloading", "paused", "postprocessing", "validating"]);
const BRIDGE_ERROR_CODES = new Set([
  "native_host_disconnected",
  "native_timeout",
  "pipe_disconnected",
  "version_mismatch",
]);

export async function tryOpenPopup(action) {
  if (typeof action?.openPopup !== "function") return false;
  try {
    await action.openPopup();
    return true;
  } catch {
    return false;
  }
}

export function badgeForStatus(status) {
  if (ATTENTION.has(status)) return "1";
  if (PENDING.has(status)) return "…";
  return "";
}

export function grayscaleRgba(source) {
  const pixels = new Uint8ClampedArray(source);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    const gray = Math.round(
      pixels[offset] * 0.2126
      + pixels[offset + 1] * 0.7152
      + pixels[offset + 2] * 0.0722,
    );
    pixels[offset] = gray;
    pixels[offset + 1] = gray;
    pixels[offset + 2] = gray;
  }
  return pixels;
}

export function shouldPoll(status) {
  return PENDING.has(status);
}

export function shouldPollState(state) {
  if (state?.status === "accepted") return Boolean(state?.payload?.analysisRequestId);
  return shouldPoll(state?.status) || (state?.status === "busy" && Boolean(state?.payload?.activeJob));
}

export function shouldAnalyzeActiveTab(state) {
  if (shouldPollState(state)) return false;
  return !state?.payload?.analysisRequestId;
}

export function activeTabStatePayload(tab) {
  const pageUrl = buildSourcePayload({ pageUrl: tab?.url }).pageUrl;
  return pageUrl ? { pageUrl } : {};
}

export function resultActionForState(state) {
  if (state?.status !== "completed") return null;
  const result = state?.payload?.activeJob?.result;
  if (state?.capabilities?.revealResult === true && result?.canReveal) {
    return {
      action: "reveal_result",
      label: result.kind === "directory" ? "Klasörü aç" : "Dosyayı göster",
    };
  }
  return state?.capabilities?.openDownloads === true
    ? { action: "open_downloads", label: "İndirilenler klasörünü aç" }
    : null;
}

export function readyStateForReturn(state) {
  const payload = state?.payload;
  if (!payload?.analysisRequestId || !Array.isArray(payload.media) || payload.media.length === 0) {
    return null;
  }
  return {
    ...state,
    status: "ready",
    payload: { ...payload, activeJob: null },
    error: null,
  };
}

export function advancedIntentForAction(action, site) {
  return action === "download_post" && site === "twitter" ? "download_twitter_post" : null;
}

export function bridgeFailure(error, command, requestId) {
  const code = BRIDGE_ERROR_CODES.has(error?.code) ? error.code : "native_host_not_found";
  return {
    messageType: "response",
    protocolVersion: 1,
    requestId,
    command,
    status: code === "version_mismatch" ? "version_mismatch" : "error",
    stateRevision: 0,
    payload: {},
    capabilities: {},
    error: {
      code,
      message: code === "version_mismatch"
        ? "MediaDrop ve eklenti protokol sürümleri uyumlu değil."
        : "MediaDrop masaüstü bağlantısı kurulamadı.",
      retryable: code !== "version_mismatch",
      action: code === "version_mismatch" ? "update_app_or_extension" : "open_app",
      reportId: null,
    },
  };
}
