import test from "node:test";
import assert from "node:assert/strict";

import {
  companionTwitterRenderModel,
  createLocalPreviewVideoSource,
  previewCanvasSize,
} from "../src/features/twitter-card/companion-renderer.js";

test("companion X renderer derives a stable card model without bootstrap globals", () => {
  const model = companionTwitterRenderModel({
    sourceUrl: "https://x.com/example/status/1",
    mediaId: "opaque-media-id",
    analysis: {
      platform: "twitter",
      contentKind: "video",
      title: "opaque-media-id",
      uploader: "Yazar",
      author: { name: "Yazar", handle: "yazar" },
      items: [{
        id: "opaque-media-id",
        type: "video",
        sourceIndex: 0,
        title: "opaque-media-id",
        text: "Gerçek gönderi metni",
        durationMs: 42_000,
        width: 1280,
        height: 720,
        hasAudio: true,
      }],
    },
  });

  assert.equal(model.mode, "video");
  assert.equal(model.title, "Gerçek gönderi metni");
  assert.equal(model.metadata.text, "Gerçek gönderi metni");
  assert.equal(model.metadata.duration, 42);
  assert.equal(model.metadata.authorHandle, "@yazar");
});

test("companion X renderer preserves quote ownership for the selected media", () => {
  const model = companionTwitterRenderModel({
    sourceUrl: "https://x.com/outer/status/2",
    mediaId: "quoted-photo",
    analysis: {
      platform: "twitter",
      contentKind: "photo",
      title: "Dış gönderi",
      uploader: "Dış Yazar",
      items: [{
        id: "outer-photo", type: "photo", sourceIndex: 0, width: 1200, height: 900,
      }, {
        id: "quoted-photo", type: "photo", sourceIndex: 1, width: 900, height: 1200,
      }],
      twitterQuote: {
        outer: { id: "outer", authorName: "Dış Yazar", authorHandle: "dis", text: "Dış gönderi" },
        quoted: { id: "quoted", authorName: "Alıntı Yazar", authorHandle: "alinti", text: "Alıntı gönderi" },
        quotedMediaIndexes: [1],
      },
    },
  });

  assert.equal(model.mode, "photo");
  assert.equal(model.metadata.activeMediaRole, "quoted");
  assert.equal(model.metadata.quotedPost.text, "Alıntı gönderi");
  assert.equal(model.title, "Dış gönderi");
  assert.deepEqual(model.secondaryMedia, {
    itemId: "outer-photo",
    role: "outer",
    aspectRatio: 4 / 3,
  });
});

test("companion X renderer uses preserved metadata for text-only posts", () => {
  const model = companionTwitterRenderModel({
    sourceUrl: "https://x.com/NASA/status/2090883745628704991",
    mediaId: "post",
    analysis: {
      platform: "twitter",
      contentKind: "text",
      title: "The word attitude here refers to spacecraft position.",
      uploader: "NASA",
      items: [],
      twitterPost: {
        id: "2090883745628704991",
        authorName: "NASA",
        authorHandle: "NASA",
        text: "The word attitude here refers to spacecraft position.",
        displayDate: "2026-08-21 19:28:26",
        isVerified: true,
        replyCount: 3,
        likeCount: 42,
      },
    },
  });

  assert.equal(model.mode, "text");
  assert.equal(model.title, "The word attitude here refers to spacecraft position.");
  assert.equal(model.metadata.authorName, "NASA");
  assert.equal(model.metadata.authorHandle, "@NASA");
  assert.equal(model.metadata.displayDate, "21 Ağu 2026");
  assert.equal(model.metadata.likeCount, 42);
});

test("companion previews stay within the popup raster budget", () => {
  assert.deepEqual(previewCanvasSize(1920, 1080, "video"), { width: 480, height: 270 });
  assert.deepEqual(previewCanvasSize(1080, 1920, "photo"), { width: 270, height: 480 });
  assert.deepEqual(previewCanvasSize(0, 0, "video"), { width: 480, height: 270 });
});

test("companion video previews use a revocable local Blob URL", async () => {
  const calls = [];
  const prepared = await createLocalPreviewVideoSource("http://asset.localhost/video.mp4", {
    fetchFn: async (source, options) => {
      calls.push(["fetch", source, options]);
      return { ok: true, blob: async () => ({ size: 42 }) };
    },
    createObjectURL: (blob) => {
      calls.push(["create", blob.size]);
      return "blob:companion-preview";
    },
    revokeObjectURL: (source) => calls.push(["revoke", source]),
  });

  assert.equal(prepared.source, "blob:companion-preview");
  assert.deepEqual(calls[0], [
    "fetch",
    "http://asset.localhost/video.mp4",
    { cache: "no-store", credentials: "omit" },
  ]);
  prepared.release();
  assert.deepEqual(calls.at(-1), ["revoke", "blob:companion-preview"]);
});
