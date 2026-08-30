import test from "node:test";
import assert from "node:assert/strict";

import {
  activeTabStatePayload,
  advancedIntentForAction,
  badgeForStatus,
  bridgeFailure,
  completedStateForAction,
  grayscaleRgba,
  pendingStateForAction,
  readyStateForReturn,
  resultActionForState,
  shouldAnalyzeActiveTab,
  shouldPollState,
  tryOpenPopup,
} from "../shared/browser-flow.js";
import {
  choiceIdForCard,
  clipRangeForInput,
  mediaPrimaryAction,
  parseClipTime,
} from "../shared/choice-model.js";
import * as choiceModel from "../shared/choice-model.js";
import * as clipPicker from "../shared/clip-picker.js";

test("openPopup capability fallback never fails the context-menu analysis path", async () => {
  assert.equal(await tryOpenPopup({ openPopup: async () => {} }), true);
  assert.equal(await tryOpenPopup({ openPopup: async () => Promise.reject(new Error("rejected")) }), false);
  assert.equal(await tryOpenPopup({}), false);
  assert.equal(await tryOpenPopup(null), false);
});

test("badge distinguishes pending work from user attention", () => {
  assert.equal(badgeForStatus("analyzing"), "…");
  assert.equal(badgeForStatus("downloading"), "…");
  assert.equal(badgeForStatus("busy"), "1");
  assert.equal(shouldPollState({ status: "accepted", payload: {} }), false);
  assert.equal(shouldPollState({ status: "accepted", payload: { analysisRequestId: "analysis" } }), true);
  assert.equal(shouldPollState({ status: "busy", payload: {} }), false);
  assert.equal(shouldPollState({ status: "busy", payload: { activeJob: { jobId: "job" } } }), true);
  assert.equal(badgeForStatus("ready"), "1");
  assert.equal(badgeForStatus("needs_user"), "1");
  assert.equal(badgeForStatus("invalid_request"), "1");
  assert.equal(badgeForStatus("completed"), "1");
  assert.equal(badgeForStatus("idle"), "");
});

test("completed work can return to the same ready analysis without retaining the finished job", () => {
  const completed = {
    status: "completed",
    payload: {
      analysisRequestId: "analysis-1",
      site: "youtube",
      media: [{ mediaId: "media-1", displayTitle: "Video" }],
      activeJob: { jobId: "job-1", status: "completed" },
    },
    capabilities: { startDownload: true },
    error: { code: "old_error" },
  };

  const ready = readyStateForReturn(completed);

  assert.equal(ready.status, "ready");
  assert.equal(ready.payload.activeJob, null);
  assert.equal(ready.payload.analysisRequestId, "analysis-1");
  assert.deepEqual(ready.payload.media, completed.payload.media);
  assert.equal(ready.error, null);
  assert.equal(readyStateForReturn({ status: "completed", payload: {} }), null);
});

test("opening the desktop app replaces a stale technical error with a starting state", () => {
  const source = {
    status: "needs_user",
    payload: { analysisRequestId: "analysis-1" },
    error: { message: "gallery-dl SECRET_CANARY" },
  };

  const pending = pendingStateForAction("advanced", source);

  assert.equal(pending.status, "app_starting");
  assert.equal(pending.error, null);
  assert.deepEqual(pending.payload, source.payload);
  assert.equal(pendingStateForAction("download_all", source), source);

  const opened = completedStateForAction("open_advanced", { status: "accepted" }, source);
  assert.equal(opened.status, "app_opened");
  assert.equal(opened.error, null);
});

test("disabled action icon becomes grayscale without losing transparency", () => {
  const source = new Uint8ClampedArray([
    242, 174, 0, 255,
    15, 30, 45, 64,
    0, 0, 0, 0,
  ]);
  const result = grayscaleRgba(source);

  assert.notEqual(result, source);
  assert.deepEqual([...source], [242, 174, 0, 255, 15, 30, 45, 64, 0, 0, 0, 0]);
  for (let offset = 0; offset < result.length; offset += 4) {
    assert.equal(result[offset], result[offset + 1]);
    assert.equal(result[offset + 1], result[offset + 2]);
    assert.equal(result[offset + 3], source[offset + 3]);
  }
});

test("direct icon analysis follows tab navigation without replacing active work", () => {
  const instagramTab = { id: 7, url: "https://www.instagram.com/reel/new/" };
  const previous = { tabId: 7, pageUrl: "https://www.youtube.com/watch?v=old" };
  const current = { tabId: instagramTab.id, pageUrl: instagramTab.url };

  assert.equal(shouldAnalyzeActiveTab({ status: "completed", payload: {} }, previous, instagramTab), true);
  assert.equal(shouldAnalyzeActiveTab({ status: "ready", payload: { analysisRequestId: "same-page" } }, current, instagramTab), false);
  assert.equal(shouldAnalyzeActiveTab({ status: "ready", payload: { analysisRequestId: "recovered" } }, null, instagramTab), false);
  assert.equal(shouldAnalyzeActiveTab({ status: "analyzing", payload: {} }, previous, instagramTab), false);
  assert.equal(shouldAnalyzeActiveTab({ status: "completed", payload: { activeJob: { jobId: "job" } } }, previous, instagramTab), true);
});

test("initial state recovery is scoped to the browser's current page", () => {
  assert.deepEqual(
    activeTabStatePayload({ url: "https://x.com/example/status/123?ref=home" }),
    { pageUrl: "https://x.com/example/status/123" },
  );
  assert.deepEqual(activeTabStatePayload({ url: "opera://extensions" }), {});
  assert.deepEqual(activeTabStatePayload(null), {});
});

test("protocol mismatch is preserved instead of looking like a missing host", () => {
  const failure = bridgeFailure(
    Object.assign(new Error("Eklenti dosyaları yenilendi."), {
      code: "version_mismatch",
      action: "reload_extension",
      expectedExtensionVersion: "1.0.2",
    }),
    "hello",
    "11111111-1111-4111-8111-111111111111",
  );
  assert.equal(failure.status, "version_mismatch");
  assert.equal(failure.error.code, "version_mismatch");
  assert.equal(failure.error.retryable, false);
  assert.equal(failure.error.action, "reload_extension");
  assert.equal(failure.error.message, "Eklenti dosyaları yenilendi.");
  assert.equal(failure.payload.expectedExtensionVersion, "1.0.2");
  assert.equal(
    bridgeFailure(
      Object.assign(new Error("pipe"), { code: "pipe_disconnected" }),
      "get_state",
      "22222222-2222-4222-8222-222222222222",
    ).error.code,
    "pipe_disconnected",
  );
});

test("completed owned jobs reveal their exact backend result", () => {
  assert.deepEqual(resultActionForState({
    status: "completed",
    capabilities: { revealResult: true, openDownloads: true },
    payload: {
      activeJob: {
        result: { kind: "file", displayName: "Gönderi.mp4", canReveal: true },
      },
    },
  }), { action: "reveal_result", label: "Dosyayı göster" });
  assert.deepEqual(resultActionForState({
    status: "completed",
    capabilities: { revealResult: true },
    payload: { activeJob: { result: { kind: "directory", canReveal: true } } },
  }), { action: "reveal_result", label: "Klasörü aç" });
  assert.equal(resultActionForState({ status: "ready", payload: {} }), null);
});

test("extension sends opaque choices rather than backend format selectors", () => {
  assert.equal(choiceIdForCard({ id: "137", type: "video" }, "youtube"), "video:137");
  assert.equal(choiceIdForCard({ id: "140", type: "audio" }, "youtube"), "audio:140");
  assert.equal(choiceIdForCard({ id: "anything", type: "instagram" }, "instagram"), "social:auto");
  assert.equal(choiceIdForCard(null, "youtube", true), "audio:best");
});

test("social photos and videos expose the correct direct popup action", () => {
  assert.deepEqual(mediaPrimaryAction({ type: "photo" }, "instagram", null), {
    label: "Fotoğrafı indir",
    choiceId: "social:auto",
  });
  assert.deepEqual(mediaPrimaryAction({ type: "video" }, "twitter", null), {
    label: "Videoyu indir",
    choiceId: "social:auto",
  });
  assert.equal(mediaPrimaryAction({ type: "text" }, "twitter", null), null);
  assert.deepEqual(mediaPrimaryAction({ type: "video" }, "youtube", { id: "137", type: "video" }), {
    label: "Videoyu indir",
    choiceId: "video:137",
  });
});

test("quick clip times accept clock notation and reject unsafe ranges", () => {
  assert.equal(parseClipTime("42"), 42);
  assert.equal(parseClipTime("01:02"), 62);
  assert.equal(parseClipTime("1:02:03"), 3723);
  assert.equal(parseClipTime("1:90"), null);
  assert.equal(parseClipTime("Infinity"), null);

  assert.deepEqual(clipRangeForInput("00:15", "00:42", 120), {
    startSeconds: 15,
    endSeconds: 42,
  });
  assert.equal(clipRangeForInput("10", "10.5", 120), null);
  assert.equal(clipRangeForInput("10", "121", 120), null);
});

test("current video time fills the selected clip edge predictably", () => {
  assert.equal(typeof choiceModel.clipTimeForCapture, "function");
  assert.equal(typeof choiceModel.clipInputLabel, "function");
  assert.equal(choiceModel.clipTimeForCapture(62.9, "start"), 62);
  assert.equal(choiceModel.clipTimeForCapture(62.1, "end"), 62);
  assert.equal(choiceModel.clipTimeForCapture(45.9, "end"), 45);
  assert.equal(choiceModel.clipTimeForCapture(Number.NaN, "start"), null);
  assert.equal(choiceModel.clipInputLabel(62), "01:02");
  assert.equal(choiceModel.clipInputLabel(3662), "1:01:02");
});

test("quality choices hide backend format details from users", () => {
  assert.equal(typeof choiceModel.qualityLabelForCard, "function");
  assert.equal(choiceModel.qualityLabelForCard({
    quality: "1080p",
    detail: "AVC · ses sonradan birleşecek",
  }), "1080p");
});

test("current YouTube time is captured into the selected inline clip field", () => {
  assert.equal(typeof clipPicker.captureClipTime, "function");
  assert.equal(typeof clipPicker.readClipDraft, "function");
  if (typeof clipPicker.captureClipTime !== "function" || typeof clipPicker.readClipDraft !== "function") return;

  let storedHost = null;
  const video = {
    currentTime: 62.9,
    paused: false,
    ended: false,
    getBoundingClientRect: () => ({ width: 1280, height: 720 }),
  };
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  globalThis.window = {
    location: { href: "https://www.youtube.com/watch?v=abc123" },
    addEventListener() {},
    removeEventListener() {},
  };
  globalThis.document = {
    addEventListener() {},
    removeEventListener() {},
    getElementById: () => storedHost,
    querySelectorAll: (selector) => selector === "video" ? [video] : [],
    createElement: () => ({
      id: "",
      hidden: false,
      dataset: {},
      remove() { storedHost = null; },
    }),
    documentElement: { append(host) { storedHost = host; } },
  };
  const base = {
    sourceKey: "https://www.youtube.com/watch?v=abc123",
    analysisRequestId: "analysis-1",
    mediaId: "media-1",
    startSeconds: 0,
    endSeconds: 15,
    target: "start",
  };
  try {
    const start = clipPicker.captureClipTime(base);
    assert.deepEqual(start, {
      ok: true,
      ...base,
      startSeconds: 62,
      target: "end",
      capturedTarget: "start",
      capturedSeconds: 62,
    });
    assert.equal(storedHost.hidden, true);

    video.currentTime = 90.2;
    const end = clipPicker.captureClipTime({ ...start, endSeconds: 15 });
    assert.equal(end.endSeconds, 90);
    assert.equal(end.target, "end");
    assert.equal(end.capturedTarget, "end");
    assert.equal(end.capturedSeconds, 90);
    assert.deepEqual(clipPicker.readClipDraft(base), {
      sourceKey: base.sourceKey,
      analysisRequestId: base.analysisRequestId,
      mediaId: base.mediaId,
      startSeconds: 62,
      endSeconds: 90,
      target: "end",
    });
  } finally {
    globalThis.document = previousDocument;
    globalThis.window = previousWindow;
  }
});

test("clip draft recovery accepts only the same analysis and page", () => {
  let removed = false;
  const host = {
    dataset: {
      mediadropClipState: JSON.stringify({
        sourceKey: "https://www.youtube.com/watch?v=abc123",
        analysisRequestId: "analysis-1",
        mediaId: "media-1",
        startSeconds: 62,
        endSeconds: 91,
        target: "end",
      }),
    },
    remove() { removed = true; },
  };
  const previousDocument = globalThis.document;
  globalThis.document = { getElementById: () => host };
  try {
    assert.deepEqual(clipPicker.readClipDraft({
      sourceKey: "https://www.youtube.com/watch?v=abc123",
      analysisRequestId: "analysis-1",
      mediaId: "media-1",
    }), {
      sourceKey: "https://www.youtube.com/watch?v=abc123",
      analysisRequestId: "analysis-1",
      mediaId: "media-1",
      startSeconds: 62,
      endSeconds: 91,
      target: "end",
    });
    assert.equal(clipPicker.readClipDraft({
      sourceKey: "https://www.youtube.com/watch?v=different",
      analysisRequestId: "analysis-1",
      mediaId: "media-1",
    }), null);
    assert.equal(removed, true);
  } finally {
    globalThis.document = previousDocument;
  }
});

test("selected clip edge survives toolbar popup focus loss", () => {
  let storedHost = null;
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  globalThis.window = {
    location: { href: "https://www.youtube.com/watch?v=abc123" },
    addEventListener() {},
  };
  globalThis.document = {
    addEventListener() {},
    getElementById: () => storedHost,
    querySelectorAll: () => {
      throw new Error("Persisting the selected field must not read or play the video.");
    },
    createElement: () => ({
      id: "",
      hidden: false,
      dataset: {},
      remove() { storedHost = null; },
    }),
    documentElement: { append(host) { storedHost = host; } },
  };
  const draft = {
    sourceKey: "https://www.youtube.com/watch?v=abc123",
    analysisRequestId: "analysis-1",
    mediaId: "media-1",
    startSeconds: 62,
    endSeconds: 90,
    target: "end",
    capture: false,
  };
  try {
    assert.deepEqual(clipPicker.captureClipTime(draft), {
      ok: true,
      sourceKey: draft.sourceKey,
      analysisRequestId: draft.analysisRequestId,
      mediaId: draft.mediaId,
      startSeconds: 62,
      endSeconds: 90,
      target: "end",
    });
    assert.deepEqual(clipPicker.readClipDraft(draft), {
      sourceKey: draft.sourceKey,
      analysisRequestId: draft.analysisRequestId,
      mediaId: draft.mediaId,
      startSeconds: 62,
      endSeconds: 90,
      target: "end",
    });
  } finally {
    globalThis.document = previousDocument;
    globalThis.window = previousWindow;
  }
});

test("Twitter post-card action uses the existing desktop download intent", () => {
  assert.equal(
    advancedIntentForAction("download_post", "twitter"),
    "download_twitter_post",
  );
  assert.equal(advancedIntentForAction("download_post", "instagram"), null);
  assert.equal(advancedIntentForAction("advanced", "twitter"), null);
});
