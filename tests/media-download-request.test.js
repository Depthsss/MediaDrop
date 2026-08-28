import test from "node:test";
import assert from "node:assert/strict";

import {
  buildMediaBatchDownloadRequest,
  buildMediaItemDownloadRequest,
  buildOptionalMediaRegistryTarget,
} from "../src/features/downloads/media-request.js";

test("media item downloads send only registry identity and output directory", () => {
  assert.deepEqual(
    buildMediaItemDownloadRequest({
      analysisId: " analysis-1 ",
      itemId: " story-7 ",
      outputDir: " C:\\Media ",
      url: "https://instagram.com/stories/owner/7/",
      sourceIndex: 4,
      authMode: "saved:instagram",
    }),
    {
      analysisId: "analysis-1",
      itemId: "story-7",
      outputDir: "C:\\Media",
    }
  );
});

test("media batch downloads send only registry identity, scope and output directory", () => {
  assert.deepEqual(
    buildMediaBatchDownloadRequest({
      analysisId: " analysis-1 ",
      scope: "all-stories",
      outputDir: " C:\\Media ",
      url: "https://instagram.com/stories/owner/7/",
      filter: "all",
      authMode: "saved:instagram",
    }),
    {
      analysisId: "analysis-1",
      scope: "all-stories",
      outputDir: "C:\\Media",
    }
  );
});

test("media download requests reject missing registry identity", () => {
  assert.throws(() => buildMediaItemDownloadRequest({ analysisId: "", itemId: "story-7" }));
  assert.throws(() => buildMediaItemDownloadRequest({ analysisId: "analysis-1", itemId: "" }));
  assert.throws(() => buildMediaBatchDownloadRequest({ analysisId: "", scope: "all" }));
  assert.throws(() => buildMediaBatchDownloadRequest({ analysisId: "analysis-1", scope: "videos" }));
});

test("Twitter post video requests bind to the selected registry media when available", () => {
  assert.deepEqual(
    buildOptionalMediaRegistryTarget({ analysisId: " analysis-1 ", itemId: " quoted-video " }),
    { analysisId: "analysis-1", itemId: "quoted-video" }
  );
  assert.deepEqual(buildOptionalMediaRegistryTarget(), { analysisId: null, itemId: null });
  assert.throws(() => buildOptionalMediaRegistryTarget({ analysisId: "analysis-1" }));
  assert.throws(() => buildOptionalMediaRegistryTarget({ itemId: "quoted-video" }));
});
