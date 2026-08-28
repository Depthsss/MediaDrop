import assert from "node:assert/strict";
import test from "node:test";

import {
  checkForUpdate,
  convertFileSrc,
  invoke,
  listen,
  openDialog,
  readClipboardText,
  relaunch,
  writeClipboardText,
} from "../src/app/tauri.js";

test("Tauri facade routes core and event calls through one boundary", async () => {
  const calls = [];
  globalThis.window = {
    __TAURI__: {
      core: {
        invoke(command, args) {
          calls.push(["invoke", command, args]);
          return Promise.resolve("ok");
        },
        convertFileSrc(path, protocol) {
          calls.push(["convert", path, protocol]);
          return `asset://${path}`;
        },
      },
      event: {
        listen(event, handler) {
          calls.push(["listen", event, typeof handler]);
          return Promise.resolve(() => {});
        },
      },
      dialog: {
        open(options) {
          calls.push(["dialog", options.directory]);
          return Promise.resolve("C:/Downloads");
        },
      },
      process: {
        relaunch() {
          calls.push(["relaunch"]);
          return Promise.resolve();
        },
      },
      updater: {
        check() {
          calls.push(["updater"]);
          return Promise.resolve(null);
        },
      },
    },
  };

  assert.equal(await invoke("analyze_media", { url: "x" }), "ok");
  assert.equal(convertFileSrc("C:/media.mp4"), "asset://C:/media.mp4");
  await listen("download-progress", () => {});
  assert.equal(await openDialog({ directory: true }), "C:/Downloads");
  assert.equal(await readClipboardText(), "ok");
  assert.equal(await writeClipboardText("C:/MediaDrop Extension"), "ok");
  await relaunch();
  assert.equal(await checkForUpdate(), null);
  assert.deepEqual(calls.map((entry) => entry[0]), [
    "invoke",
    "convert",
    "listen",
    "dialog",
    "invoke",
    "invoke",
    "relaunch",
    "updater",
  ]);
  assert.deepEqual(calls[4], ["invoke", "plugin:clipboard-manager|read_text", undefined]);
  assert.deepEqual(calls[5], [
    "invoke",
    "plugin:clipboard-manager|write_text",
    { text: "C:/MediaDrop Extension" },
  ]);

  delete globalThis.window;
});
