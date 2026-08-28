import test from "node:test";
import assert from "node:assert/strict";

import {
  DOWNLOAD_HISTORY_LIMIT,
  basename,
  historyTimeMs,
  prependDownloadHistoryItem,
  readDownloadHistory,
  removeDownloadHistoryItem,
  writeDownloadHistory,
} from "../src/features/downloads/history-store.js";

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
  };
}

test("history store safely handles corrupt JSON and bounds persisted entries", () => {
  const storage = memoryStorage({ history: "not-json" });
  assert.deepEqual(readDownloadHistory(storage, "history"), []);

  const items = Array.from({ length: DOWNLOAD_HISTORY_LIMIT + 10 }, (_, id) => ({ id }));
  const written = writeDownloadHistory(storage, "history", items);
  assert.equal(written.length, DOWNLOAD_HISTORY_LIMIT);
  assert.equal(readDownloadHistory(storage, "history").length, DOWNLOAD_HISTORY_LIMIT);
});

test("history item is canonicalized, deduplicated by path and removable by id", () => {
  const existing = [
    { id: "old", filePath: "C:\\Downloads\\same.mp4" },
    { id: "keep", filePath: "C:\\Downloads\\keep.mp4" },
  ];
  const next = prependDownloadHistoryItem(
    existing,
    { filePath: " C:\\Downloads\\same.mp4 ", platform: "instagram" },
    1_700_000_000_000,
    "fixed",
  );

  assert.equal(next.length, 2);
  assert.equal(next[0].id, "1700000000000-fixed");
  assert.equal(next[0].title, "same.mp4");
  assert.equal(basename(next[0].filePath), "same.mp4");
  assert.equal(historyTimeMs(next[0]), 1_700_000_000_000);
  assert.deepEqual(removeDownloadHistoryItem(next, { id: "keep" }), [next[0]]);
});
