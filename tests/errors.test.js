import test from "node:test";
import assert from "node:assert/strict";

import { parseBackendError, STRUCTURED_ERROR_PREFIX } from "../src/app/errors.js";

test("typed errors preserve retry, action and report identity", () => {
  const parsed = parseBackendError({
    code: "download_busy",
    message: "Başka bir indirme sürüyor.",
    retryable: true,
    action: "wait_for_active_download",
    reportId: "report-42",
  });
  assert.equal(parsed.code, "download_busy");
  assert.equal(parsed.retryable, true);
  assert.equal(parsed.action, "wait_for_active_download");
  assert.equal(parsed.reportId, "report-42");
});

test("structured parser isolates JSON and a legacy report suffix", () => {
  const error = `${STRUCTURED_ERROR_PREFIX}${JSON.stringify({
    code: "instagram_rate_limited",
    message: "Daha sonra tekrar deneyin.",
    retryable: true,
    action: "retry_later",
  })}\n\nHata raporu oluşturuldu: C:\\Reports\\rate.txt`;
  const parsed = parseBackendError(error);
  assert.equal(parsed.message, "Daha sonra tekrar deneyin.");
  assert.equal(parsed.retryable, true);
  assert.equal(parsed.action, "retry_later");
  assert.equal(parsed.reportId, "C:\\Reports\\rate.txt");
});

test("legacy report strings expose reportId without polluting the message", () => {
  const parsed = parseBackendError("İndirme başarısız.\n\nHata raporu oluşturuldu: report-7");
  assert.equal(parsed.message, "İndirme başarısız.");
  assert.equal(parsed.reportId, "report-7");
  assert.equal(parsed.retryable, false);
  assert.equal(parsed.action, "");
});
