import test from "node:test";
import assert from "node:assert/strict";

import { appReducer, createAppStore, createInitialAppState } from "../src/app/store.js";

test("analysis reducer bounds carousel index and resets dependent preview state", () => {
  const initial = createInitialAppState();
  const ready = appReducer(initial, {
    type: "analysis/succeeded",
    platform: "instagram",
    mediaAnalysis: { analysisId: "a" },
    items: [{ id: "one" }, { id: "two" }],
    index: 99,
  });
  assert.equal(ready.analysis.index, 1);
  assert.equal(ready.analysis.platform, "instagram");

  const reset = appReducer(ready, { type: "analysis/reset" });
  assert.equal(reset.analysis.status, "idle");
  assert.deepEqual(reset.analysis.items, []);
  assert.equal(reset.preview.itemId, null);
});

test("download reducer owns deterministic job lifecycle", () => {
  const store = createAppStore();
  store.dispatch({ type: "download/status", status: "downloading" });
  store.dispatch({ type: "download/job", jobId: "job-1" });
  assert.equal(store.getState().download.jobId, "job-1");
  store.dispatch({ type: "download/status", status: "idle" });
  assert.deepEqual(store.getState().download, {
    status: "idle",
    jobId: "",
    lastArgs: null,
    lastMediaArgs: null,
  });
});

test("unknown transitions preserve state identity", () => {
  const state = createInitialAppState();
  assert.equal(appReducer(state, { type: "unknown" }), state);
});
