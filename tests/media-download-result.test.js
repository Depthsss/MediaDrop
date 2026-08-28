import test from "node:test";
import assert from "node:assert/strict";

import {
  mediaDownloadOutcome,
  mediaDownloadTarget,
  normalizeMediaDownloadResult,
} from "../src/features/downloads/media-result.js";

test("batch result carries files, failures and explicit zero counts", () => {
  const result = normalizeMediaDownloadResult({
    files: [{ filePath: "C:\\Media\\one.jpg", fileSize: 12, sourceIndex: 2 }],
    failures: [{ itemId: "story-3", source_index: 3, message: "expired" }],
    downloadedCount: 0,
    failed_count: 1,
    outputDir: "C:\\Media",
  });

  assert.equal(result.downloadedCount, 0);
  assert.equal(result.failedCount, 1);
  assert.deepEqual(result.failures[0], { itemId: "story-3", sourceIndex: 3, message: "expired" });
  assert.equal(result.files[0].filePath, "C:\\Media\\one.jpg");
});

test("full, partial and zero Story batches have distinct outcomes", () => {
  const args = { mode: "batch", scope: "all-stories", storyCount: 3 };
  assert.deepEqual(mediaDownloadOutcome({ downloadedCount: 3, failedCount: 0 }, args), {
    status: "success", text: "3 hikaye indirildi.", label: "hikaye", downloadedCount: 3, failedCount: 0,
  });
  assert.equal(
    mediaDownloadOutcome({ downloadedCount: 2, failedCount: 1 }, args).text,
    "2 hikaye indirildi, 1 hikaye indirilemedi."
  );
  assert.equal(mediaDownloadOutcome({ downloadedCount: 0, failedCount: 3 }, args).status, "error");
});

test("photo, video and mixed media use accurate labels", () => {
  assert.equal(
    mediaDownloadOutcome({ downloadedCount: 1, failedCount: 0 }, { itemType: "video", isStory: true }).text,
    "Video hikayesi indirildi."
  );
  assert.equal(
    mediaDownloadOutcome({ downloadedCount: 2 }, { mode: "batch", photoCount: 0, videoCount: 2 }).text,
    "2 video indirildi."
  );
  assert.equal(
    mediaDownloadOutcome({ downloadedCount: 3 }, { mode: "batch", photoCount: 2, videoCount: 1 }).text,
    "3 medya indirildi."
  );
});

test("batch notifications target only a successful output folder", () => {
  const batch = { mode: "batch" };
  assert.equal(mediaDownloadTarget({ downloadedCount: 2, outputDir: "C:\\Batch", filePath: "one.jpg" }, batch), "C:\\Batch");
  assert.equal(mediaDownloadTarget({ downloadedCount: 2, outputDir: "", filePath: "one.jpg" }, batch), "");
  assert.equal(mediaDownloadTarget({ downloadedCount: 0, outputDir: "C:\\Batch" }, batch), "");
  assert.equal(mediaDownloadTarget({ downloadedCount: 1, filePath: "C:\\one.jpg" }, { mode: "item" }), "C:\\one.jpg");
});
