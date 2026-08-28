import test from "node:test";
import assert from "node:assert/strict";

import * as sourceCandidates from "../shared/source-candidates.js";

const { buildSourcePayload, rankCandidates } = sourceCandidates;

test("supported social browse pages require opening a concrete content page", () => {
  const classify = sourceCandidates.classifySourcePage;
  assert.equal(typeof classify, "function");
  if (typeof classify !== "function") return;

  for (const url of [
    "https://www.youtube.com/watch?v=abc123",
    "https://youtu.be/abc123",
    "https://www.instagram.com/reel/ABC123/",
    "https://www.instagram.com/reels/DTvTp9yCLk7/",
    "https://www.instagram.com/somecreator/reel/ABC123/",
    "https://www.instagram.com/p/ABC123/",
    "https://x.com/creator/status/1234567890",
    "https://twitter.com/i/status/1234567890",
    "https://www.tiktok.com/@creator/video/1234567890",
    "https://vm.tiktok.com/short-code/",
  ]) {
    assert.equal(classify(url), "content", url);
  }

  for (const url of [
    "https://www.youtube.com/",
    "https://www.instagram.com/",
    "https://www.instagram.com/explore/",
    "https://www.instagram.com/reels/audio/123456789/",
    "https://x.com/home",
    "https://x.com/creator",
    "https://www.tiktok.com/foryou",
  ]) {
    assert.equal(classify(url), "browse", url);
  }

  assert.equal(classify("https://example.com/post/123"), "unsupported");
});

test("toolbar action is disabled only outside MediaDrop platforms", () => {
  const presentation = sourceCandidates.actionPresentationForPage;
  assert.equal(typeof presentation, "function");
  if (typeof presentation !== "function") return;

  assert.deepEqual(presentation("https://www.youtube.com/watch?v=abc123"), {
    enabled: true,
    title: "MediaDrop",
  });
  assert.deepEqual(presentation("https://x.com/home"), {
    enabled: true,
    title: "MediaDrop",
  });
  for (const url of ["https://example.com/", "opera://extensions", "file:///C:/video.mp4", ""]) {
    assert.deepEqual(presentation(url), {
      enabled: false,
      title: "MediaDrop bu sayfayı desteklemiyor.",
    }, url);
  }
});

test("Instagram reel variants are sent to the backend as one canonical content URL", () => {
  for (const pageUrl of [
    "https://www.instagram.com/reels/DTvTp9yCLk7/?igsh=tracking-value",
    "https://www.instagram.com/somecreator/reel/DTvTp9yCLk7/?igsh=tracking-value",
  ]) {
    const payload = buildSourcePayload({ pageUrl });
    assert.equal(
      payload.pageUrl,
      "https://www.instagram.com/reel/DTvTp9yCLk7/",
      pageUrl,
    );
  }
});

test("content UI variants reuse one canonical analysis until the post changes", () => {
  const cases = [
    [
      "https://x.com/NASA/status/1234567890/photo/1?s=20#media",
      "https://x.com/NASA/status/1234567890",
    ],
    [
      "https://twitter.com/NASA/status/1234567890?ref_src=twsrc%5Etfw",
      "https://x.com/NASA/status/1234567890",
    ],
    [
      "https://www.youtube.com/watch?v=abc123&t=62s&list=watch-later",
      "https://www.youtube.com/watch?v=abc123",
    ],
    [
      "https://www.youtube.com/shorts/abc123?feature=share",
      "https://www.youtube.com/watch?v=abc123",
    ],
    [
      "https://youtu.be/abc123?t=42",
      "https://www.youtube.com/watch?v=abc123",
    ],
    [
      "https://www.instagram.com/p/ABC123/?igsh=tracking-value",
      "https://www.instagram.com/p/ABC123/",
    ],
    [
      "https://www.instagram.com/stories/example/123456789/?utm_source=ig_story_item_share",
      "https://www.instagram.com/stories/example/123456789/",
    ],
    [
      "https://www.tiktok.com/@creator/video/1234567890?is_from_webapp=1",
      "https://www.tiktok.com/@creator/video/1234567890",
    ],
    [
      "https://vm.tiktok.com/ZExample/?share_app_id=1233",
      "https://vm.tiktok.com/ZExample/",
    ],
  ];

  for (const [pageUrl, expected] of cases) {
    assert.equal(buildSourcePayload({ pageUrl }).pageUrl, expected, pageUrl);
  }

  assert.notEqual(
    buildSourcePayload({ pageUrl: "https://x.com/NASA/status/1234567891" }).pageUrl,
    cases[0][1],
  );
});

test("context media wins deterministic ranking and candidate list stays bounded", () => {
  const candidates = Array.from({ length: 12 }, (_, index) => ({
    candidateUrl: `https://cdn.example/video-${index}.mp4?token=kept-${index}`,
    detectedBy: index === 9 ? "context_menu_src" : "dom_source",
    mediaType: "video",
    playing: index === 2,
    visible: true,
    width: 1280,
    height: 720,
    durationSeconds: 30,
  }));

  const ranked = rankCandidates(candidates);
  assert.equal(ranked.length, 8);
  assert.equal(ranked[0].detectedBy, "context_menu_src");
  assert.equal(ranked[0].candidateUrl, "https://cdn.example/video-9.mp4?token=kept-9");
});

test("source payload keeps page/frame but never promotes blocked schemes to URLs", () => {
  const payload = buildSourcePayload({
    pageUrl: "https://example.com/watch",
    frameUrl: "https://www.youtube.com/embed/abc",
    mediaType: "video",
    candidates: [
      { candidateUrl: "blob:https://example.com/id", detectedBy: "dom_current_src" },
      { candidateUrl: "file:///C:/secret.mp4", detectedBy: "dom_src" },
      { candidateUrl: "https://cdn.example/video.mp4?sig=do-not-strip", detectedBy: "dom_source" },
    ],
  });

  assert.equal(payload.pageUrl, "https://example.com/watch");
  assert.equal(payload.frameUrl, "https://www.youtube.com/embed/abc");
  assert.deepEqual(
    new Set(payload.candidates.map((candidate) => candidate.candidateUrl ?? null)),
    new Set([null, "https://cdn.example/video.mp4?sig=do-not-strip"]),
  );
  assert.equal(payload.candidates.find((candidate) => !candidate.candidateUrl).detectedBy, "blob_hint");
});

test("short muted looping background media ranks below meaningful visible playback", () => {
  const [first] = rankCandidates([
    {
      candidateUrl: "https://cdn.example/background.mp4",
      detectedBy: "dom_src",
      mediaType: "video",
      visible: true,
      muted: true,
      loop: true,
      durationSeconds: 10,
      width: 1920,
      height: 1080,
    },
    {
      candidateUrl: "https://cdn.example/main.mp4",
      detectedBy: "dom_current_src",
      mediaType: "video",
      visible: true,
      playing: true,
      durationSeconds: 120,
      width: 1280,
      height: 720,
    },
  ]);
  assert.equal(first.candidateUrl, "https://cdn.example/main.mp4");
});
