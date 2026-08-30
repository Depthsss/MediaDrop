import assert from "node:assert/strict";
import test from "node:test";

const model = await import("../src/features/extension-setup/setup-model.js").catch(() => ({}));

test("default browser is listed first, followed by other installed browsers", () => {
  assert.equal(typeof model.orderExtensionBrowsers, "function");
  const ordered = model.orderExtensionBrowsers([
    { id: "opera", installed: true, defaultBrowser: false },
    { id: "chrome", installed: false, defaultBrowser: false },
    { id: "edge", installed: true, defaultBrowser: true },
    { id: "opera_gx", installed: true, defaultBrowser: false },
  ]);

  assert.deepEqual(ordered.map((browser) => browser.id), [
    "edge",
    "opera",
    "opera_gx",
    "chrome",
  ]);
});

test("each supported browser receives its own unpacked-extension guide", () => {
  assert.equal(typeof model.extensionGuideForBrowser, "function");

  assert.deepEqual(model.extensionGuideForBrowser("opera_gx"), {
    page: "opera:extensions",
    shortcut: "Ctrl+Shift+E",
    launchesInternalPage: false,
    developerMode: "Sağ üstteki Geliştirici modu seçeneğini aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile düğmesine bas; değilse Paketlenmemiş öğe yükle düğmesine bas.",
  });
  assert.deepEqual(model.extensionGuideForBrowser("chrome"), {
    page: "chrome://extensions",
    shortcut: "",
    launchesInternalPage: true,
    developerMode: "Sağ üstteki Geliştirici modu anahtarını aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile simgesine bas; değilse sol üstteki Paketlenmemiş öğe yükle düğmesine bas.",
  });
  assert.deepEqual(model.extensionGuideForBrowser("edge"), {
    page: "edge://extensions",
    shortcut: "",
    launchesInternalPage: true,
    developerMode: "Sol menüdeki Geliştirici modu anahtarını aç.",
    loadUnpacked: "MediaDrop kuruluysa kartındaki Yenile düğmesine bas; değilse Paketlenmemiş öğe yükle düğmesine bas.",
  });
  assert.equal(model.extensionGuideForBrowser("firefox"), null);
});

test("setup progress advances from browser launch to automatic native connection", () => {
  assert.equal(typeof model.extensionSetupStepStates, "function");

  assert.deepEqual(
    model.extensionSetupStepStates({ selected: true, opened: false, connected: false }),
    ["complete", "current", "pending", "pending"],
  );
  assert.deepEqual(
    model.extensionSetupStepStates({ selected: true, opened: true, connected: false }),
    ["complete", "complete", "current", "pending"],
  );
  assert.deepEqual(
    model.extensionSetupStepStates({ selected: true, opened: true, connected: true }),
    ["complete", "complete", "complete", "complete"],
  );
});
