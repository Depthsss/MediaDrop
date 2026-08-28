import {
  activeTabStatePayload,
  badgeForStatus,
  bridgeFailure,
  grayscaleRgba,
  shouldAnalyzeActiveTab,
  shouldPollState,
  tryOpenPopup,
} from "./shared/browser-flow.js";
import { NativeClient } from "./shared/native-client.js";
import {
  actionPresentationForPage,
  buildSourcePayload,
  classifySourcePage,
  rankCandidates,
} from "./shared/source-candidates.js";
import { captureClipTime, readClipDraft } from "./shared/clip-picker.js";
import { scanMediaInPage } from "./scan-media.js";

const HOST_NAME = "com.mab.mediadrop";
const MENU_ID = "mediadrop-download";
const SUPPORTED_DOCUMENT_PATTERNS = [
  "*://*.youtube.com/*",
  "*://youtu.be/*",
  "*://*.instagram.com/*",
  "*://*.x.com/*",
  "*://*.twitter.com/*",
  "*://*.tiktok.com/*",
];
const COLOR_ACTION_ICONS = {
  32: "icons/icon-32.png",
  64: "icons/icon-64.png",
  128: "icons/icon-128.png",
};
const POLL_MS = 2_000;
const RECONNECT_DELAYS = [250, 500, 1_000, 2_000, 4_000];

let nativeCloseTimer = null;
let grayscaleActionIconsPromise = null;
const actionUpdateVersions = new Map();
const tabAnalyses = new Map();
const native = new NativeClient(chrome.runtime, HOST_NAME, () => crypto.randomUUID(), (event) => {
  const tracked = analysisForRequest(event.requestId);
  if (tracked) void setBadge(event.status, tracked.tabId);
  try {
    chrome.runtime.sendMessage({ type: "native_status", state: event }, () => void chrome.runtime.lastError);
  } catch {
    // No popup is normally open for most native status events.
  }
});

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function analysisForRequest(requestId) {
  return [...tabAnalyses.values()].find((entry) => entry.requestId === requestId) || null;
}

async function setBadge(status, tabId) {
  if (!Number.isInteger(tabId) || tabId < 0) return;
  try {
    await Promise.all([
      chrome.action.setBadgeBackgroundColor({
        color: status === "error" ? "#e11d48" : "#52525b",
        tabId,
      }),
      chrome.action.setBadgeText({ text: badgeForStatus(status), tabId }),
    ]);
  } catch {
    // The target tab can close while an async native response is arriving.
  }
}

function trackAnalysis(tabId, pageUrl, requestId) {
  if (!Number.isInteger(tabId) || tabId < 0 || !pageUrl || !requestId) return null;
  const current = tabAnalyses.get(tabId);
  if (current?.pageUrl === pageUrl && current.requestId === requestId) return current;
  clearTimeout(current?.timer);
  const tracked = { tabId, pageUrl, requestId, timer: null };
  tabAnalyses.set(tabId, tracked);
  return tracked;
}

function forgetAnalysis(tabId, clearBadge = false) {
  const tracked = tabAnalyses.get(tabId);
  clearTimeout(tracked?.timer);
  tabAnalyses.delete(tabId);
  if (clearBadge) void setBadge("idle", tabId);
}

function grayscaleActionIcons() {
  grayscaleActionIconsPromise ||= Promise.all(
    Object.entries(COLOR_ACTION_ICONS).map(async ([size, path]) => {
      const response = await fetch(chrome.runtime.getURL(path));
      if (!response.ok) throw new Error("action_icon_unavailable");
      const bitmap = await createImageBitmap(await response.blob());
      try {
        const edge = Number(size);
        const canvas = new OffscreenCanvas(edge, edge);
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) throw new Error("action_icon_unavailable");
        context.drawImage(bitmap, 0, 0, edge, edge);
        const imageData = context.getImageData(0, 0, edge, edge);
        imageData.data.set(grayscaleRgba(imageData.data));
        return [size, imageData];
      } finally {
        bitmap.close?.();
      }
    }),
  ).then(Object.fromEntries);
  return grayscaleActionIconsPromise;
}

async function actionIcon(enabled) {
  if (enabled) return { path: COLOR_ACTION_ICONS };
  try {
    return { imageData: await grayscaleActionIcons() };
  } catch {
    return { path: COLOR_ACTION_ICONS };
  }
}

async function updateActionForTab(tabId, url) {
  if (!Number.isInteger(tabId) || tabId < 0) return;
  const pageUrl = activeTabStatePayload({ url }).pageUrl || "";
  const tracked = tabAnalyses.get(tabId);
  if (!tracked || tracked.pageUrl !== pageUrl) forgetAnalysis(tabId, true);
  const version = (actionUpdateVersions.get(tabId) || 0) + 1;
  actionUpdateVersions.set(tabId, version);
  const presentation = actionPresentationForPage(url);
  try {
    await Promise.all([
      presentation.enabled ? chrome.action.enable(tabId) : chrome.action.disable(tabId),
      chrome.action.setTitle({ tabId, title: presentation.title }),
    ]);
    const icon = await actionIcon(presentation.enabled);
    if (actionUpdateVersions.get(tabId) === version) {
      await chrome.action.setIcon({ tabId, ...icon });
    }
  } catch {
    // The tab may have closed while its navigation event was being handled.
  }
}

async function refreshActionForTab(tabId) {
  try {
    const frame = await chrome.webNavigation.getFrame({ tabId, frameId: 0 });
    await updateActionForTab(tabId, frame?.url);
  } catch {
    await updateActionForTab(tabId, "");
  }
}

async function refreshAllTabActions() {
  try {
    const tabs = await chrome.tabs.query({});
    await Promise.all(tabs.map((tab) => refreshActionForTab(tab.id)));
  } catch {
    // Browser startup can finish before every tab has a main frame.
  }
}

function safeBridgeError(error, command) {
  return bridgeFailure(error, command, crypto.randomUUID());
}

async function nativeCall(command, payload = {}, requestId, retryRead = false) {
  clearTimeout(nativeCloseTimer);
  let attempt = 0;
  for (;;) {
    try {
      const response = await native.call(command, payload, requestId);
      if (retryRead && response.error?.code === "pipe_disconnected") {
        throw Object.assign(new Error("pipe_disconnected"), { code: "pipe_disconnected" });
      }
      if (!shouldPollState(response)) {
        nativeCloseTimer = setTimeout(() => native.close(), 15_000);
      }
      return response;
    } catch (error) {
      if (!retryRead || attempt >= RECONNECT_DELAYS.length) {
        nativeCloseTimer = setTimeout(() => native.close(), 15_000);
        return safeBridgeError(error, command);
      }
      native.close();
      await delay(RECONNECT_DELAYS[attempt++]);
    }
  }
}

async function watchAnalysis(tabId, pageUrl, analysisRequestId) {
  const tracked = trackAnalysis(tabId, pageUrl, analysisRequestId);
  if (!tracked) return;
  clearTimeout(tracked.timer);
  const poll = async () => {
    if (tabAnalyses.get(tabId) !== tracked) return;
    const state = await nativeCall(
      "get_state",
      { analysisRequestId },
      undefined,
      true,
    );
    if (tabAnalyses.get(tabId) !== tracked) return;
    await setBadge(state.status, tabId);
    if (shouldPollState(state)) {
      tracked.timer = setTimeout(poll, POLL_MS);
    } else {
      tracked.timer = null;
    }
  };
  tracked.timer = setTimeout(poll, POLL_MS);
}

async function beginAnalysis(payload, requestId = crypto.randomUUID(), tabId = null) {
  const tracked = trackAnalysis(tabId, payload.pageUrl, requestId);
  const pendingResponse = nativeCall("analyze_source", payload, requestId, false);
  void setBadge("analyzing", tabId);
  const response = await pendingResponse;
  if (tracked && tabAnalyses.get(tabId) !== tracked) return response;
  await setBadge(response.status, tabId);
  if (tracked && ["accepted", "analyzing", "app_starting"].includes(response.status)) {
    void watchAnalysis(tabId, tracked.pageUrl, requestId);
  }
  return response;
}

async function scanTab(tab) {
  try {
    return await chrome.scripting.executeScript({
      target: { tabId: tab.id, allFrames: true },
      func: scanMediaInPage,
    });
  } catch {
    return chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: scanMediaInPage,
    });
  }
}

async function analyzeActiveTab(activeTab = null) {
  const tab = activeTab || (await chrome.tabs.query({ active: true, currentWindow: true }))[0];
  if (!tab?.id || !/^https?:\/\//i.test(tab.url || "")) {
    return {
      status: "unsupported",
      payload: {},
      error: { code: "unsupported_source", message: "Bu sekme MediaDrop tarafından analiz edilemez." },
    };
  }
  if (classifySourcePage(tab.url) === "browse") {
    return {
      status: "unsupported",
      payload: {},
      capabilities: {},
      error: {
        code: "content_page_required",
        message: "Lütfen indirmek istediğin gönderi veya videonun sayfasını aç.",
      },
    };
  }
  let scanResults = [];
  try {
    scanResults = await scanTab(tab);
  } catch {
    // Restricted pages still use the tab URL as the primary source.
  }
  const frames = scanResults.map((entry) => entry.result).filter(Boolean);
  const candidates = rankCandidates(frames.flatMap((frame) => frame.candidates || []));
  const frameUrl = frames.find(
    (frame) => frame.frameUrl && frame.frameUrl !== tab.url && (frame.candidates || []).length,
  )?.frameUrl;
  return beginAnalysis(
    buildSourcePayload({ pageUrl: tab.url, frameUrl, mediaType: "video", candidates }),
    crypto.randomUUID(),
    tab.id,
  );
}

function clipPickerError(code, message) {
  return {
    status: code === "invalid_request" ? "invalid_request" : "error",
    payload: {},
    capabilities: {},
    error: { code, message, retryable: true },
  };
}

async function activeClipContext(message) {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const sourceKey = buildSourcePayload({ pageUrl: tab?.url }).pageUrl;
  const analysisRequestId = String(message?.analysisRequestId || "");
  const mediaId = String(message?.mediaId || "");
  if (!tab?.id || !sourceKey.startsWith("https://www.youtube.com/watch?v=")) {
    return { error: clipPickerError("clip_picker_unavailable", "Klip zamanı yalnız açık YouTube videosundan alınabilir.") };
  }
  if (!analysisRequestId || analysisRequestId.length > 64 || !mediaId || mediaId.length > 256) {
    return { error: clipPickerError("invalid_request", "Klip seçici isteği geçersiz.") };
  }
  return { tab, sourceKey, analysisRequestId, mediaId };
}

async function captureCurrentClipTime(message, capture = true) {
  const context = await activeClipContext(message);
  if (context.error) return context.error;
  const draft = message.draft || {};
  try {
    const [injection] = await chrome.scripting.executeScript({
      target: { tabId: context.tab.id },
      func: captureClipTime,
      args: [{
        sourceKey: context.sourceKey,
        analysisRequestId: context.analysisRequestId,
        mediaId: context.mediaId,
        startSeconds: draft.startSeconds,
        endSeconds: draft.endSeconds,
        target: draft.target,
        capture,
      }],
    });
    if (!injection?.result?.ok) {
      if (injection?.result?.error === "clip_range_invalid") {
        return clipPickerError("clip_range_invalid", "Bitiş, başlangıçtan sonra olmalı.");
      }
      if (injection?.result?.error === "video_time_unavailable") {
        return clipPickerError("clip_picker_unavailable", "Açık YouTube oynatıcısının zamanı okunamadı.");
      }
      return clipPickerError("clip_picker_unavailable", "Sekme değişti; açık videoyu yeniden analiz et.");
    }
    return { status: "accepted", payload: { clipDraft: injection.result }, capabilities: {}, error: null };
  } catch {
    return clipPickerError("clip_picker_unavailable", "Açık YouTube oynatıcısının zamanı okunamadı.");
  }
}

async function getClipDraft(message) {
  const context = await activeClipContext(message);
  if (context.error) return { status: "accepted", payload: { clipDraft: null }, capabilities: {}, error: null };
  try {
    const [injection] = await chrome.scripting.executeScript({
      target: { tabId: context.tab.id },
      func: readClipDraft,
      args: [{
        sourceKey: context.sourceKey,
        analysisRequestId: context.analysisRequestId,
        mediaId: context.mediaId,
      }],
    });
    return { status: "accepted", payload: { clipDraft: injection?.result || null }, capabilities: {}, error: null };
  } catch {
    return { status: "accepted", payload: { clipDraft: null }, capabilities: {}, error: null };
  }
}

function createContextMenu() {
  chrome.contextMenus.create({
    id: MENU_ID,
    title: "MediaDrop ile indir…",
    contexts: ["page", "video", "audio"],
    documentUrlPatterns: SUPPORTED_DOCUMENT_PATTERNS,
  }, () => void chrome.runtime.lastError);
}

createContextMenu();
void refreshAllTabActions();
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(createContextMenu);
  void refreshAllTabActions();
  void nativeCall("get_state", {}, undefined, true);
});
chrome.runtime.onStartup.addListener(() => {
  createContextMenu();
  void refreshAllTabActions();
});
const updateFromNavigation = (details) => {
  if (details.frameId === 0) void updateActionForTab(details.tabId, details.url);
};
chrome.webNavigation.onCommitted.addListener(updateFromNavigation);
chrome.webNavigation.onHistoryStateUpdated.addListener(updateFromNavigation);
chrome.tabs.onActivated.addListener(({ tabId }) => void refreshActionForTab(tabId));
chrome.tabs.onRemoved.addListener((tabId) => {
  forgetAnalysis(tabId);
  actionUpdateVersions.delete(tabId);
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== MENU_ID) return;
  const candidate = info.srcUrl
    ? [{
        candidateUrl: info.srcUrl,
        detectedBy: "context_menu_src",
        mediaType: info.mediaType === "audio" ? "audio" : "video",
        playing: true,
        visible: true,
      }]
    : [];
  const payload = buildSourcePayload({
    pageUrl: info.pageUrl,
    frameUrl: info.frameUrl,
    mediaType: info.mediaType === "audio" ? "audio" : "video",
    candidates: candidate,
  });
  const requestId = crypto.randomUUID();
  void beginAnalysis(payload, requestId, tab?.id);
  void tryOpenPopup(chrome.action);
});

async function handlePopupMessage(message) {
  switch (message?.type) {
    case "get_state": {
      const [tab] = message.preferActiveTab
        ? await chrome.tabs.query({ active: true, currentWindow: true })
        : [null];
      const statePayload = message.analysisRequestId
        ? { analysisRequestId: message.analysisRequestId }
        : activeTabStatePayload(tab);
      const state = await nativeCall(
        "get_state",
        statePayload,
        undefined,
        true,
      );
      if (message.preferActiveTab) {
        if (shouldAnalyzeActiveTab(state)) return analyzeActiveTab(tab);
      }
      if (state.payload?.analysisRequestId) {
        const pageUrl = activeTabStatePayload(tab).pageUrl;
        if (Number.isInteger(tab?.id) && pageUrl) {
          trackAnalysis(tab.id, pageUrl, state.payload.analysisRequestId);
        }
        if (shouldPollState(state) && Number.isInteger(tab?.id) && pageUrl) {
          void setBadge(state.status, tab.id);
          void watchAnalysis(tab.id, pageUrl, state.payload.analysisRequestId);
        }
      }
      return state;
    }
    case "analyze_active_tab":
      return analyzeActiveTab();
    case "capture_clip_time":
      return captureCurrentClipTime(message);
    case "set_clip_draft":
      return captureCurrentClipTime(message, false);
    case "get_clip_draft":
      return getClipDraft(message);
    case "native_command": {
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
      const state = await nativeCall(message.command, message.payload || {}, message.requestId, false);
      await setBadge(state.status, tab?.id);
      const analysisRequestId = state.payload?.analysisRequestId || message.payload?.analysisRequestId;
      const pageUrl = activeTabStatePayload(tab).pageUrl;
      if (shouldPollState(state) && analysisRequestId && Number.isInteger(tab?.id) && pageUrl) {
        void watchAnalysis(tab.id, pageUrl, analysisRequestId);
      }
      return state;
    }
    case "clear_badge":
      {
        const tracked = analysisForRequest(message.analysisRequestId);
        if (tracked) {
          await setBadge("idle", tracked.tabId);
        } else {
          const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
          await setBadge("idle", tab?.id);
        }
      }
      return { status: "accepted" };
    default:
      return { status: "invalid_request", error: { code: "invalid_request", message: "Bilinmeyen popup isteği." } };
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  handlePopupMessage(message).then(sendResponse, (error) => sendResponse(safeBridgeError(error, "popup")));
  return true;
});
