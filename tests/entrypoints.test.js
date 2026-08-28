import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("frontend and Rust entrypoints stay thin", () => {
  const frontendEntry = read("src/main.js").trim().split(/\r?\n/);
  const rustEntry = read("src-tauri/src/lib.rs").trim().split(/\r?\n/);

  assert.ok(frontendEntry.length <= 5, "src/main.js must remain a thin bootstrap entry");
  assert.match(frontendEntry.join("\n"), /app\/bootstrap\.js/);
  assert.ok(rustEntry.length <= 120, "src-tauri/src/lib.rs must remain a thin crate entry");
  assert.match(rustEntry.join("\n"), /app_impl\.rs/);
  assert.match(rustEntry.join("\n"), /tauri::Builder::default/);
  assert.ok(read("src/app/bootstrap.js").length > 0);
  assert.ok(read("src-tauri/src/app_impl.rs").length > 0);
});

test("twitter renderer stays outside the frontend bootstrap", () => {
  const bootstrap = read("src/app/bootstrap.js");
  const renderer = read("src/features/twitter-card/renderer.js");

  assert.doesNotMatch(bootstrap, /function renderTwitterPostCardPng\s*\(/);
  assert.doesNotMatch(bootstrap, /function hydrateTwitterAvatarDataUrl\s*\(/);
  assert.match(renderer, /function renderTwitterPostCardPng\s*\(/);
  assert.match(renderer, /function hydrateTwitterAvatarDataUrl\s*\(/);
  assert.doesNotMatch(renderer, /\bclampNumber\b/);
});

test("legacy video analysis clears the old media UI before publishing the new platform", () => {
  const bootstrap = read("src/app/bootstrap.js");
  const start = bootstrap.indexOf("function applyVideoAnalysisInfo");
  const end = bootstrap.indexOf("function isBrowserAuthError", start);
  const body = bootstrap.slice(start, end);

  assert.ok(start >= 0 && end > start, "video analysis function must remain discoverable");
  assert.ok(
    body.indexOf("resetMediaPreview({ resize: false })") <
      body.indexOf('type: "analysis/succeeded"'),
    "media reset must not erase the successfully detected YouTube platform"
  );
});

test("extension setup is reachable, accessible, and backed by fixed native commands", () => {
  const html = read("src/index.html");
  const bootstrap = read("src/app/bootstrap.js");
  const styles = read("src/styles/app.css");

  assert.match(html, /id="extensionSetupBtn"/);
  assert.match(html, /id="extensionSetupOverlay"[^>]+role="dialog"/);
  assert.match(html, /aria-labelledby="extensionSetupTitle"/);
  assert.match(bootstrap, /invoke\("get_extension_setup_info"\)/);
  assert.match(bootstrap, /invoke\("open_extension_setup"/);
  assert.match(bootstrap, /invoke\("take_extension_setup_request"\)/);
  assert.match(bootstrap, /modalController\.register\("extension-setup"/);
  assert.match(styles, /extension-setup\.css/);
});

test("cloud diagnostics stay off unless the backend explicitly returns true", () => {
  const bootstrap = read("src/app/bootstrap.js");
  const start = bootstrap.indexOf("async function updateCloudReportsFromBackend");
  const end = bootstrap.indexOf("async function setCloudReportsEnabled", start);
  const body = bootstrap.slice(start, end);

  assert.match(body, /checked = enabled === true/);
  assert.match(body, /catch[\s\S]*checked = false/);
});
