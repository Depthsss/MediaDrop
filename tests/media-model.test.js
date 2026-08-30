import test from "node:test";
import assert from "node:assert/strict";
import * as previewModel from "../src/features/preview/media-model.js";

import {
  isMediaAnalysisExpired,
  instagramAnalysisNeedsAvatarAuth,
  mediaAnalysisAuthorIdentity,
  mediaCardDescription,
  mediaAnalysisWarningMessages,
  mediaInitialIndex,
  mediaItemKindLabel,
  mediaItemType,
  mediaPreviewPolicy,
  normalizeMediaAnalysis,
  normalizeMediaAnalysisItems,
  normalizeMediaItem,
  normalizeMediaPreviewResponse,
  normalizeRasterImageSource,
  reusableMediaPreviewValue,
  selectMediaPreviewPrefetchItems,
  shouldTryMediaInventory,
  supportsLegacyVideoFallback,
  twitterMediaPostDownloadKind,
} from "../src/features/preview/media-model.js";

test("clip preview policy shares one deadline and rejects stale builds", () => {
  assert.equal(typeof previewModel.clipPreviewAttemptBudgetMs, "function");
  assert.equal(typeof previewModel.isClipPreviewBuildActive, "function");

  assert.equal(previewModel.clipPreviewAttemptBudgetMs(20_000, 10_000, 2), 3_000);
  assert.equal(previewModel.clipPreviewAttemptBudgetMs(20_000, 19_500, 2), 500);
  assert.equal(previewModel.clipPreviewAttemptBudgetMs(20_000, 20_000, 1), 0);
  assert.equal(previewModel.clipPreviewAttemptBudgetMs(20_000, 10_000, 0), 0);

  assert.equal(previewModel.isClipPreviewBuildActive(4, 4, 20_000, 19_999), true);
  assert.equal(previewModel.isClipPreviewBuildActive(4, 5, 20_000, 19_999), false);
  assert.equal(previewModel.isClipPreviewBuildActive(4, 4, 20_000, 20_000), false);
});

test("paused native clip previews remain playable after metadata loads", () => {
  assert.equal(typeof previewModel.nativeClipPlayerState, "function");
  assert.equal(previewModel.nativeClipPlayerState(true, 1), 2);
  assert.equal(previewModel.nativeClipPlayerState(false, 1), 3);
  assert.equal(previewModel.nativeClipPlayerState(false, 2), 1);
});

test("clip preview response keeps separate audio attached to the native video", () => {
  assert.equal(typeof previewModel.clipPreviewStreamSources, "function");
  assert.deepEqual(
    previewModel.clipPreviewStreamSources({
      url: "https://media.example/video.mp4",
      urls: ["https://media.example/video.mp4"],
      audioUrl: "https://media.example/audio.m4a",
    }),
    {
      videoUrls: ["https://media.example/video.mp4"],
      audioUrl: "https://media.example/audio.m4a",
    }
  );
  assert.deepEqual(
    previewModel.clipPreviewStreamSources("https://media.example/progressive.mp4"),
    {
      videoUrls: ["https://media.example/progressive.mp4"],
      audioUrl: "",
    }
  );
});

test("native clip audio resyncs only after audible clock drift", () => {
  assert.equal(typeof previewModel.nativeClipAudioSyncTarget, "function");
  const playable = { videoReadyState: 4, audioReadyState: 4 };
  assert.equal(previewModel.nativeClipAudioSyncTarget(15, 14.8, playable), null);
  assert.equal(previewModel.nativeClipAudioSyncTarget(15, 14.5, playable), 15);
  assert.equal(previewModel.nativeClipAudioSyncTarget(15, Number.NaN, playable), 15);
  assert.equal(
    previewModel.nativeClipAudioSyncTarget(15, 14.5, {
      videoReadyState: 2,
      audioReadyState: 4,
    }),
    null
  );
  assert.equal(
    previewModel.nativeClipAudioSyncTarget(15, 14.5, {
      videoReadyState: 4,
      audioReadyState: 4,
      seeking: true,
    }),
    null
  );
});

test("Instagram Story cards never use generated filenames as descriptions", () => {
  assert.equal(
    mediaCardDescription(
      { isStory: true, text: "775493207_17888113278674638_7601426557850047607_n" },
      { platform: "instagram", contentKind: "story", title: "fallback" }
    ),
    ""
  );
  assert.equal(
    mediaCardDescription(
      { isStory: false, text: "Gerçek gönderi açıklaması" },
      { platform: "instagram", contentKind: "post" }
    ),
    "Gerçek gönderi açıklaması"
  );
});

test("public Instagram analysis retries authentication only when the canonical avatar is missing", () => {
  assert.equal(
    instagramAnalysisNeedsAvatarAuth({
      platform: "instagram",
      items: [{ id: "post-1" }],
      author: { name: "Gönderi sahibi", avatarDataUrl: null },
    }),
    true
  );
  assert.equal(
    instagramAnalysisNeedsAvatarAuth({
      platform: "instagram",
      items: [{ id: "post-1" }],
      author: { avatarDataUrl: "data:image/jpeg;base64,AA==" },
    }),
    false
  );
  assert.equal(
    instagramAnalysisNeedsAvatarAuth({ platform: "twitter", items: [{ id: "tweet-1" }] }),
    false
  );
});

test("Twitter media exposes post download for both photos and videos", () => {
  assert.equal(twitterMediaPostDownloadKind("twitter", { type: "photo" }), "photo");
  assert.equal(twitterMediaPostDownloadKind("twitter", { type: "video" }), "video");
  assert.equal(twitterMediaPostDownloadKind("instagram", { type: "video" }), "");
  assert.equal(twitterMediaPostDownloadKind("twitter", null), "");
});
import { loadRasterImageSource } from "../src/features/preview/raster-loader.js";
import {
  consumeInstagramAuthPromptBudget,
  instagramInitialAuthMode,
  isInstagramAuthRecoverySignal,
  nextInstagramAuthRecoveryStep,
  nextInstagramCookiePrepareStep,
} from "../src/features/instagram/auth-policy.js";
import {
  clampWindowHeight,
  createWindowLayoutCoordinator,
  isDuplicateWindowHeight,
  isLikelyProgrammaticResize,
  measureWindowContentHeight,
} from "../src/features/window-layout.js";

test("media item normalization preserves fields and normalizes the model boundary", () => {
  assert.deepEqual(
    normalizeMediaItem({ id: 42, type: "VIDEO", sourceIndex: "3", previewRef: " ref ", isStory: 1 }),
    { id: "42", type: "video", sourceIndex: 3, previewRef: "ref", previewUrl: "", isStory: true }
  );
  assert.equal(mediaItemKindLabel({ type: "video", isStory: true }), "Video hikayesi");
  assert.equal(mediaItemKindLabel({ type: "photo" }), "Fotoğraf");
});

test("media item helpers tolerate an empty selection before analysis starts", () => {
  assert.equal(mediaItemType(null), "photo");
  assert.equal(mediaItemKindLabel(null), "Fotoğraf");
});

test("analysis initialIndex survives filtering and requestedItemId is the fallback", () => {
  const analysis = {
    analysisId: "analysis-1",
    initialIndex: 2,
    items: [
      { id: "one", type: "photo", previewRef: "one" },
      { id: "hidden", type: "photo" },
      { id: "three", type: "video", previewRef: "three" },
    ],
  };
  const normalized = normalizeMediaAnalysisItems(analysis);
  assert.deepEqual(normalized.items.map((item) => item.id), ["one", "hidden", "three"]);
  assert.equal(normalized.initialIndex, 2);

  const items = normalized.items;
  assert.equal(mediaInitialIndex({ requestedItemId: "three" }, analysis.items, items), 2);
});

test("MediaAnalysis normalizes expiry and exposes known Instagram warnings", () => {
  const analysis = normalizeMediaAnalysis({
    analysisId: " analysis-42 ",
    expiresAtMs: "5000",
    warnings: [
      "requestedStoryUnavailable",
      "instagramAuthenticatedPublicFallback",
      "requestedStoryUnavailable",
      "",
    ],
  });

  assert.equal(analysis.analysisId, "analysis-42");
  assert.equal(analysis.expiresAtMs, 5000);
  assert.deepEqual(analysis.warnings, [
    "requestedStoryUnavailable",
    "instagramAuthenticatedPublicFallback",
  ]);
  assert.equal(isMediaAnalysisExpired(analysis, 4999), false);
  assert.equal(isMediaAnalysisExpired(analysis, 5000), true);
  assert.equal(isMediaAnalysisExpired({ expiresAtMs: 1 }, 5000), false);
  assert.deepEqual(mediaAnalysisWarningMessages(analysis), [
    "Bağlantıdaki hikaye artık aktif değil; hesabın ilk erişilebilir hikayesi gösteriliyor.",
    "Kayıtlı Instagram oturumuyla analiz tamamlanamadı; gönderi herkese açık verilerle gösteriliyor.",
  ]);
});

test("registry-backed author identity never falls back to an item commenter avatar", () => {
  const commenter = {
    authorId: "commenter-id",
    authorName: "Commenter",
    authorHandle: "commenter",
    avatarDataUrl: "data:image/jpeg;base64,COMMENTER",
    avatarUrl: "https://cdn.example/commenter.jpg",
  };
  const canonical = mediaAnalysisAuthorIdentity({
    analysisId: "analysis-1",
    author: { id: "owner-id", name: "Owner", handle: "owner" },
  }, commenter);
  assert.deepEqual(canonical, {
    registryBacked: true,
    id: "owner-id",
    name: "Owner",
    handle: "owner",
    avatarDataUrl: "",
    avatarUrl: "",
  });

  const missingCanonical = mediaAnalysisAuthorIdentity({ analysisId: "analysis-2" }, commenter);
  assert.equal(missingCanonical.name, "");
  assert.equal(missingCanonical.avatarDataUrl, "");
  assert.equal(missingCanonical.avatarUrl, "");
  assert.equal(mediaAnalysisAuthorIdentity({}, commenter).name, "Commenter");
});

test("Instagram MediaAnalysis never falls through to the legacy video analyzer", () => {
  assert.equal(supportsLegacyVideoFallback("instagram"), false);
  assert.equal(supportsLegacyVideoFallback(" INSTAGRAM "), false);
  assert.equal(supportsLegacyVideoFallback("youtube"), true);
  assert.equal(supportsLegacyVideoFallback("twitter"), true);
});

test("direct TikTok videos skip the slow gallery inventory analyzer", () => {
  assert.equal(
    shouldTryMediaInventory(
      "tiktok",
      "https://www.tiktok.com/@lolbert.3/video/7672947206864866590"
    ),
    false
  );
  assert.equal(
    shouldTryMediaInventory("tiktok", "https://www.tiktok.com/@creator/photo/1234567890"),
    true
  );
  assert.equal(shouldTryMediaInventory("tiktok", "https://vt.tiktok.com/short-code/"), true);
  assert.equal(
    shouldTryMediaInventory("twitter", "https://x.com/creator/status/1234567890"),
    true
  );
});

test("preview response normalizer supports old and new backend response shapes", () => {
  assert.equal(normalizeMediaPreviewResponse("  http://127.0.0.1/media  "), "http://127.0.0.1/media");
  assert.equal(
    normalizeMediaPreviewResponse(
      { filePath: " C:\\Cache\\media-previews\\story.mp4 ", mediaType: "video", hasAudio: true },
      (filePath) => `asset://${filePath.replaceAll("\\\\", "/")}`
    ),
    "asset://C:/Cache/media-previews/story.mp4"
  );
  assert.equal(normalizeMediaPreviewResponse({ filePath: "C:\\private\\story.mp4" }), "");
  assert.equal(normalizeMediaPreviewResponse({ streamUrl: "stream://story" }), "stream://story");
  assert.equal(normalizeMediaPreviewResponse({ dataUrl: "data:image/jpeg;base64,AA==" }), "data:image/jpeg;base64,AA==");
  assert.equal(normalizeMediaPreviewResponse(null), "");
});

test("registry preview policy blocks item URL fallbacks and refreshes access on display", () => {
  assert.deepEqual(
    mediaPreviewPolicy(
      { analysisId: " analysis-1 " },
      {
        id: " story-1 ",
        previewUrl: "https://cdn.example/preview.jpg",
        streamUrl: "https://cdn.example/stream",
        url: "https://cdn.example/original.jpg",
      }
    ),
    {
      analysisId: "analysis-1",
      itemId: "story-1",
      registryBacked: true,
      allowLegacyFallback: false,
      refreshAccessOnDisplay: true,
      legacySource: "",
      cacheKey: "analysis-1:story-1",
    }
  );
});

test("preview policy rejects direct sources without registry identity", () => {
  assert.deepEqual(
    mediaPreviewPolicy({}, { id: "legacy-1", previewUrl: " data:image/jpeg;base64,AA== " }),
    {
      analysisId: "",
      itemId: "legacy-1",
      registryBacked: false,
      allowLegacyFallback: false,
      refreshAccessOnDisplay: false,
      legacySource: "",
      cacheKey: "",
    }
  );

  assert.deepEqual(
    normalizeMediaAnalysisItems({
      items: [
        { id: "legacy-photo", previewUrl: "https://cdn.example/photo.jpg" },
        { id: "legacy-video", previewRef: "https://cdn.example/video.mp4" },
      ],
    }).items,
    []
  );
});

test("canvas raster sources accept local prepared previews and reject remote URLs", () => {
  const dataUrl = "data:image/jpeg;base64,AA==";
  assert.equal(normalizeRasterImageSource(dataUrl), dataUrl);
  assert.equal(normalizeRasterImageSource("asset://localhost/C:/cache/post.jpg"), "asset://localhost/C:/cache/post.jpg");
  assert.equal(normalizeRasterImageSource("blob:http://localhost/image-id"), "blob:http://localhost/image-id");
  assert.equal(normalizeRasterImageSource("http://asset.localhost/C:/cache/post.jpg"), "http://asset.localhost/C:/cache/post.jpg");
  assert.equal(normalizeRasterImageSource("http://127.0.0.1:4312/previews/item"), "http://127.0.0.1:4312/previews/item");
  assert.equal(normalizeRasterImageSource("https://pbs.twimg.com/media/post.jpg"), "");
  assert.equal(normalizeRasterImageSource("javascript:alert(1)"), "");
});

test("raster loader turns prepared asset sources into a revoked local Blob image", async () => {
  const blob = { size: 32 };
  const loadedSources = [];
  const revoked = [];
  const expectedImage = { width: 1200, height: 800 };

  const result = await loadRasterImageSource("asset://localhost/C:/cache/post.jpg", {
    fetchFn: async (source, options) => {
      assert.equal(source, "asset://localhost/C:/cache/post.jpg");
      assert.deepEqual(options, { cache: "no-store", credentials: "omit" });
      return { ok: true, blob: async () => blob };
    },
    createObjectURL: (value) => {
      assert.equal(value, blob);
      return "blob:prepared-preview";
    },
    revokeObjectURL: (value) => revoked.push(value),
    loadImage: async (source) => {
      loadedSources.push(source);
      return expectedImage;
    },
    onFetchError: (error) => assert.fail(error),
  });

  assert.equal(result, expectedImage);
  assert.deepEqual(loadedSources, ["blob:prepared-preview"]);
  assert.deepEqual(revoked, ["blob:prepared-preview"]);

  const legacyDataUrl = "data:image/png;base64,AA==";
  assert.equal(
    await loadRasterImageSource(legacyDataUrl, {
      fetchFn: () => assert.fail("data URLs must not be fetched"),
      loadImage: async (source) => source,
    }),
    legacyDataUrl
  );
});

test("prefetch selection stays within current plus immediate neighbors and orders nearest first", () => {
  const items = Array.from({ length: 8 }, (_, index) => ({ id: String(index) }));
  const queue = selectMediaPreviewPrefetchItems(items, 4, {
    canPrefetch: (item) => item.id !== "3",
    isPrepared: (item) => item.id === "5",
  });
  assert.deepEqual(queue.map(({ index }) => index), [4]);
});

test("forced preview refresh reuses an in-flight preparation request", async () => {
  const pending = Promise.resolve("asset://prepared-preview");

  assert.equal(
    reusableMediaPreviewValue({ source: "asset://stale", promise: pending }, true),
    pending
  );
  assert.equal(
    reusableMediaPreviewValue({ source: "asset://cached" }, true),
    ""
  );
  assert.equal(
    reusableMediaPreviewValue({ source: "asset://cached" }, false),
    "asset://cached"
  );
});

test("Instagram auth policy starts public posts silently and requires auth for stories", () => {
  assert.equal(instagramInitialAuthMode({ isStory: false, hasSavedCookies: false }), "public");
  assert.equal(instagramInitialAuthMode({ isStory: false, hasSavedCookies: true }), "saved:instagram");
  assert.equal(instagramInitialAuthMode({ isStory: true, hasSavedCookies: false }), null);
  assert.equal(
    instagramInitialAuthMode({
      isStory: false,
      hasSavedCookies: true,
      forcePrompt: true,
    }),
    null
  );
});

test("Instagram auth recovery ignores parser/not-found failures and accepts explicit auth signals", () => {
  assert.equal(isInstagramAuthRecoverySignal({ code: "instagram_auth_required" }), true);
  assert.equal(isInstagramAuthRecoverySignal({ action: "request_cookie_permission" }), true);
  assert.equal(isInstagramAuthRecoverySignal({ action: "request_browser_restart" }), true);
  assert.equal(
    isInstagramAuthRecoverySignal({ code: "instagram_story_not_found", message: "oturum bulunamadı gibi içerik" }),
    false
  );
  assert.equal(isInstagramAuthRecoverySignal({ code: "instagram_schema_error", message: "admin" }), false);
  assert.equal(isInstagramAuthRecoverySignal({ code: "instagram_highlight_unsupported" }), false);
  assert.equal(isInstagramAuthRecoverySignal({ message: "Instagram oturumu bulunamadı" }), true);
  assert.equal(
    isInstagramAuthRecoverySignal({
      message: "gallery-dl: HTTP redirect to login page (https://www.instagram.com/accounts/login/)",
    }),
    true
  );
});

test("Instagram auth recovery allows at most one refresh followed by one prompt", () => {
  const base = {
    isAuthError: true,
    authMode: "saved:instagram",
    hasRefreshBrowser: true,
  };
  assert.equal(nextInstagramAuthRecoveryStep(base), "refresh");
  assert.equal(
    nextInstagramAuthRecoveryStep({ ...base, refreshAttempts: 1 }),
    "prompt"
  );
  assert.equal(
    nextInstagramAuthRecoveryStep({ ...base, refreshAttempts: 1, promptAttempts: 1 }),
    "stop"
  );
  assert.equal(
    nextInstagramAuthRecoveryStep({ ...base, isAuthError: false }),
    "stop"
  );
});

test("Instagram initial Story prompt consumes the shared recovery prompt budget", () => {
  const initial = consumeInstagramAuthPromptBudget({ promptAttempts: 0 });
  assert.deepEqual(initial, { allowed: true, promptAttempts: 1 });
  assert.equal(
    nextInstagramAuthRecoveryStep({
      isAuthError: true,
      authMode: "browser:chrome:save",
      promptAttempts: initial.promptAttempts,
    }),
    "stop"
  );
  assert.deepEqual(
    consumeInstagramAuthPromptBudget({ promptAttempts: initial.promptAttempts }),
    { allowed: false, promptAttempts: 1 }
  );
});

test("Instagram cookie preparation asks once before directly closing a locked browser", () => {
  assert.equal(nextInstagramCookiePrepareStep({ code: "", forcePromptAttempts: 0 }), "stop");
  assert.equal(
    nextInstagramCookiePrepareStep({
      code: "browser_restart_required",
      forcePromptAttempts: 0,
    }),
    "force-close"
  );
  assert.equal(
    nextInstagramCookiePrepareStep({
      code: "instagram_browser_locked",
      forcePromptAttempts: 0,
    }),
    "force-close"
  );
  assert.equal(
    nextInstagramCookiePrepareStep({
      code: "browser_still_running",
      forcePromptAttempts: 0,
    }),
    "force-close"
  );
  assert.equal(
    nextInstagramCookiePrepareStep({
      code: "browser_still_running",
      forcePromptAttempts: 1,
    }),
    "stop"
  );
});

test("window layout helpers clamp measurements and deduplicate height requests", () => {
  assert.equal(measureWindowContentHeight({ scrollHeight: 700, rectHeight: 680 }), 720);
  assert.equal(measureWindowContentHeight({ scrollHeight: 2000 }), 980);
  assert.equal(clampWindowHeight(100), 520);
  assert.equal(isDuplicateWindowHeight(840, 841), true);
  assert.equal(isDuplicateWindowHeight(840, 843), false);
});

test("window resize origin detection protects programmatic resize from manual opt-out", () => {
  assert.equal(isLikelyProgrammaticResize({ now: 1500, lastRequestAt: 1000, graceMs: 900 }), true);
  assert.equal(isLikelyProgrammaticResize({ now: 2000, lastRequestAt: 1000, graceMs: 900 }), false);
  assert.equal(isLikelyProgrammaticResize({ now: 1000, lastRequestAt: 0, graceMs: 900 }), false);
});

test("window layout coordinator deduplicates requests and resumes after a mode change", async () => {
  let clock = 1000;
  let queuedTask = null;
  const requested = [];
  const coordinator = createWindowLayoutCoordinator({
    measureHeight: () => 840,
    requestHeight: async (height) => requested.push(height),
    requestFrame: (callback) => callback(),
    setTimer: (callback) => {
      queuedTask = callback;
      return 1;
    },
    clearTimer: () => {},
    now: () => clock,
  });

  assert.equal(await coordinator.resizeNow(840), true);
  assert.equal(await coordinator.resizeNow(841), false);
  assert.deepEqual(requested, [840]);

  clock = 2100;
  assert.equal(coordinator.handleWindowResize(), "manual");
  assert.equal(coordinator.refresh(), false);
  assert.equal(coordinator.getState().autoResizeSuspended, true);

  assert.equal(coordinator.setMode("media", 840), true);
  assert.equal(coordinator.getState().autoResizeSuspended, false);
  await queuedTask?.();
  coordinator.dispose();
});
