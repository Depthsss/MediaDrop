import test from "node:test";
import assert from "node:assert/strict";

import {
  asNumber,
  clipDownloadStatusText,
  displayProgressPercent,
  downloadCancellationCompletion,
  parseFallbackProgressLine,
  progressJobId,
  unitToMb,
} from "../src/features/downloads/progress-model.js";

test("progress units preserve existing MB conversion rules", () => {
  assert.equal(unitToMb("1,5", "GiB"), 1536);
  assert.equal(unitToMb(1024, "KiB"), 1);
  assert.equal(unitToMb(4, "MiB"), 4);
  assert.equal(unitToMb("invalid", "MiB"), null);
});

test("fallback yt-dlp progress keeps total, downloaded and speed labels", () => {
  assert.equal(
    parseFallbackProgressLine("[download] 25% of 2.00GiB at 4.00MiB/s", 25),
    "512.0 / 2048.0 MB • 4.00 MB/s"
  );
  assert.equal(parseFallbackProgressLine("[download] Destination: output.mp4", null), "Dosya hazırlanıyor...");
  assert.equal(parseFallbackProgressLine("[Merger] Merging formats", null), "Video ve ses birleştiriliyor...");
});

test("clip status and staged percentage remain behavior-compatible", () => {
  assert.equal(clipDownloadStatusText({ line: "ses indiriliyor" }), "Ses indiriliyor...");
  assert.equal(clipDownloadStatusText({ phase: "encode" }), "Video derleniyor...");
  assert.equal(clipDownloadStatusText({}), "Klip işleniyor...");
  assert.equal(displayProgressPercent(50, "[download] segment", true), 41);
  assert.equal(displayProgressPercent(120, "", false), 100);
  assert.equal(displayProgressPercent(-5, "", false), 0);
});

test("numeric payload normalization rejects non-finite values", () => {
  assert.equal(asNumber("12.5"), 12.5);
  assert.equal(asNumber(undefined), null);
  assert.equal(asNumber(Number.POSITIVE_INFINITY), null);
});

test("download progress job IDs support camelCase and legacy snake_case payloads", () => {
  assert.equal(progressJobId({ jobId: " job-42 " }), "job-42");
  assert.equal(progressJobId({ job_id: "legacy-job" }), "legacy-job");
  assert.equal(progressJobId({}), "");
});

test("cancellation completes immediately only when no download command is running", () => {
  assert.equal(downloadCancellationCompletion("paused"), "immediate");
  assert.equal(downloadCancellationCompletion("downloading"), "deferred");
  assert.equal(downloadCancellationCompletion("pausing"), "deferred");
});
