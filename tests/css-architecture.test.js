import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);

test("CSS entrypoint imports named feature files without polish patch layers", () => {
  const appCss = readFileSync(new URL("src/styles/app.css", root), "utf8");
  assert.doesNotMatch(appCss, /(?:quality-preview|responsive)-polish\.css/);
  assert.match(appCss, /quality-picker\.css/);
  assert.equal(existsSync(new URL("src/styles/quality-preview-polish.css", root)), false);
  assert.equal(existsSync(new URL("src/styles/responsive-polish.css", root)), false);
});

test("download panel has one authoritative declaration block", () => {
  const files = [
    "src/styles/layout-shell.css",
    "src/styles/downloads-history.css",
    "src/styles/clip-editor.css",
    "src/styles/quality-picker.css",
  ];
  const count = files
    .filter((file) => existsSync(new URL(file, root)))
    .map((file) => readFileSync(new URL(file, root), "utf8"))
    .reduce((total, css) => total + (css.match(/^\.download-panel\s*\{/gm)?.length || 0), 0);
  assert.equal(count, 1);
});

test("quality picker stylesheet does not override clip or shell components", () => {
  const css = readFileSync(new URL("src/styles/quality-picker.css", root), "utf8");
  assert.doesNotMatch(css, /^\.(?:clip|console|download-panel)\b/gm);
});

test("text-only media cards can hide the unavailable item download action", () => {
  const css = readFileSync(new URL("src/styles/features/media-preview.css", root), "utf8");
  assert.match(css, /\.media-photo-btn\.is-hidden\s*\{[^}]*display:\s*none/s);
});
