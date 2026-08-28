import test from "node:test";
import assert from "node:assert/strict";
import { createHash, createPublicKey } from "node:crypto";
import { readFile } from "node:fs/promises";

test("MV3 manifest requests only the companion permissions in use", async () => {
  const manifest = JSON.parse(
    await readFile(new URL("../manifest.json", import.meta.url), "utf8"),
  );
  assert.equal(manifest.manifest_version, 3);
  assert.equal(manifest.default_locale, "tr");
  assert.deepEqual(manifest.permissions.sort(), [
    "activeTab",
    "contextMenus",
    "nativeMessaging",
    "scripting",
    "webNavigation",
  ]);
  assert.equal(manifest.host_permissions, undefined);
  assert.equal(manifest.content_scripts, undefined);
  assert.equal(manifest.background.service_worker, "service-worker.js");
  assert.equal(manifest.action.default_popup, "popup/popup.html");
  assert.equal(manifest.action.default_state, "disabled");
  assert.equal(manifest.action.default_title, "MediaDrop bu sayfayı desteklemiyor.");
  const publicKey = createPublicKey({
    key: Buffer.from(manifest.key, "base64"),
    format: "der",
    type: "spki",
  }).export({ format: "der", type: "spki" });
  const id = [...createHash("sha256").update(publicKey).digest().subarray(0, 16)]
    .flatMap((byte) => [byte >> 4, byte & 15])
    .map((value) => String.fromCharCode(97 + value))
    .join("");
  assert.equal(id, "gifnifkakikpndieohkijmjccmmikalm");
});

test("toolbar action follows full and same-document navigation", async () => {
  const serviceWorker = await readFile(
    new URL("../service-worker.js", import.meta.url),
    "utf8",
  );

  assert.match(serviceWorker, /chrome\.webNavigation\.onCommitted\.addListener/);
  assert.match(serviceWorker, /chrome\.webNavigation\.onHistoryStateUpdated\.addListener/);
  assert.match(serviceWorker, /chrome\.tabs\.onActivated\.addListener/);
  assert.match(serviceWorker, /chrome\.webNavigation\.getFrame/);
  assert.match(serviceWorker, /chrome\.action\.disable/);
  assert.match(serviceWorker, /chrome\.action\.setIcon/);
  assert.match(serviceWorker, /chrome\.action\.setTitle/);
  assert.match(serviceWorker, /new OffscreenCanvas/);
});

test("context menu survives worker restarts and custom media players", async () => {
  const serviceWorker = await readFile(
    new URL("../service-worker.js", import.meta.url),
    "utf8",
  );

  assert.match(serviceWorker, /contexts:\s*\["page",\s*"video",\s*"audio"\]/);
  assert.match(serviceWorker, /chrome\.runtime\.onStartup\.addListener/);
  assert.match(serviceWorker, /documentUrlPatterns:\s*SUPPORTED_DOCUMENT_PATTERNS/);
  assert.doesNotMatch(serviceWorker, /documentUrlPatterns:\s*\["http:\/\/\*\/\*",\s*"https:\/\/\*\/\*"\]/);
  assert.match(serviceWorker, /\ncreateContextMenu\(\);/);
});

test("clip capture is user-invoked without persistent page UI or access", async () => {
  const [manifestSource, serviceWorker, popup, picker] = await Promise.all([
    readFile(new URL("../manifest.json", import.meta.url), "utf8"),
    readFile(new URL("../service-worker.js", import.meta.url), "utf8"),
    readFile(new URL("../popup/popup.js", import.meta.url), "utf8"),
    readFile(new URL("../shared/clip-picker.js", import.meta.url), "utf8"),
  ]);
  const manifest = JSON.parse(manifestSource);

  assert.equal(manifest.content_scripts, undefined);
  assert.equal(manifest.host_permissions, undefined);
  assert.doesNotMatch(JSON.stringify(manifest.permissions), /storage|tabs|windows/);
  assert.match(serviceWorker, /case "capture_clip_time"/);
  assert.match(serviceWorker, /chrome\.scripting\.executeScript/);
  assert.match(popup, /Bu anı al/);
  assert.match(popup, /element\("details", "clip-panel quick-clip"\)/);
  assert.match(popup, /element\("summary", "clip-title"\)/);
  assert.match(popup, /element\("strong", "", "Hızlı klip"\)/);
  assert.doesNotMatch(popup, /Videodan zaman seç/);
  assert.doesNotMatch(picker, /attachShadow/);
  assert.doesNotMatch(picker, /position:\s*fixed/);
  assert.doesNotMatch(picker, /\.innerHTML\s*=/);
  assert.match(picker, /host\.hidden\s*=\s*true/);
  assert.match(picker, /querySelectorAll\("video"\)/);
});

test("popup keeps internal error codes out of the user interface", async () => {
  const popup = await readFile(
    new URL("../popup/popup.js", import.meta.url),
    "utf8",
  );

  assert.doesNotMatch(popup, /element\("div", "error-code", state\.error\.code\)/);
  assert.match(popup, /content_page_required/);
  assert.match(popup, /İçeriğin sayfasını aç/);
});

test("popup ships the approved accessible V2 interface", async () => {
  const [html, css, popup, build] = await Promise.all([
    readFile(new URL("../popup/popup.html", import.meta.url), "utf8"),
    readFile(new URL("../popup/popup.css", import.meta.url), "utf8"),
    readFile(new URL("../popup/popup.js", import.meta.url), "utf8"),
    readFile(new URL("../build.mjs", import.meta.url), "utf8"),
  ]);

  assert.match(html, /id="connectionStatus"[^>]*role="status"/);
  assert.match(html, /id="connectionText"/);
  assert.match(html, /Tarayıcı eklentisi/);
  assert.match(css, /width:\s*416px/);
  assert.match(css, /font-family:\s*"Instrument Sans"/);
  assert.match(popup, /İndirmeye hazır/);
  assert.match(popup, /Aktif sekmeyi yeniden tara/);
  assert.match(popup, /action === "retry_active_tab"/);
  assert.match(popup, /type:\s*"analyze_active_tab"/);
  assert.match(build, /InstrumentSans-Regular\.ttf/);
  assert.match(build, /InstrumentSans-SemiBold\.ttf/);
  assert.match(build, /InstrumentSans-Bold\.ttf/);
});
