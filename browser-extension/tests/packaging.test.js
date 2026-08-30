import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const root = new URL("../../", import.meta.url);

test("native host packaging is stable, exact-origin, and uninstallable", async () => {
  const [configText, templateText, wix, hostWindows, hostMain, appLib] = await Promise.all([
    readFile(new URL("src-tauri/tauri.conf.json", root), "utf8"),
    readFile(new URL("src-tauri/native-messaging/com.mab.mediadrop.template.json", root), "utf8"),
    readFile(new URL("src-tauri/windows/fragments/native-messaging.wxs", root), "utf8"),
    readFile(new URL("src-tauri/src/bin/mediadrop-native-host/windows.rs", root), "utf8"),
    readFile(new URL("src-tauri/src/bin/mediadrop-native-host/main.rs", root), "utf8"),
    readFile(new URL("src-tauri/src/lib.rs", root), "utf8"),
  ]);
  const config = JSON.parse(configText);
  const template = JSON.parse(templateText);

  assert(!config.bundle.externalBin.includes("binaries/mediadrop-native-host"));
  assert.deepEqual(config.bundle.windows.wix.componentRefs, ["MediaDropNativeMessagingHost"]);
  assert.equal(config.build.beforeDevCommand, "npm run prepare:tauri:dev");
  assert.equal(config.build.beforeBuildCommand, "npm run prepare:tauri:build");
  assert(config.bundle.externalBin.includes("binaries/mediadrop-component-worker"));
  assert.equal(
    config.bundle.resources["../browser-extension/dist/"],
    "browser-extension/",
  );
  assert.equal(config.bundle.windows.webviewInstallMode.type, "embedBootstrapper");
  assert.equal(template.name, "__HOST_NAME__");
  assert.deepEqual(template.allowed_origins, ["chrome-extension://__EXTENSION_ID__/"]);
  assert(!templateText.includes("*"));
  assert.match(wix, /NativeMessagingHosts\\com\.mab\.mediadrop/);
  assert.match(wix, /Root="HKLM"/);
  assert.doesNotMatch(wix, /Root="HKCU"/);
  assert.match(wix, /Microsoft\\Edge\\NativeMessagingHosts\\com\.mab\.mediadrop/);
  assert.match(wix, /App Paths\\mediadrop\.exe/);
  assert.doesNotMatch(wix, /target\\release\\mediadrop-native-host\.exe/);
  assert.equal(wix.match(/<Component Id=/g)?.length, 1);
  assert.equal(wix.match(/<File\s/g)?.length, 1);
  assert.match(wix, /ForceCreateOnInstall="yes"/);
  assert.match(wix, /ForceDeleteOnUninstall="yes"/);
  assert.doesNotMatch(wix, /Action="createAndRemoveOnUninstall"/);
  assert.equal(wix.match(/KeyPath="yes"/g)?.length, 1);
  assert.equal(config.app.windows[0].visible, false);
  assert.match(hostWindows, /\.arg\("--companion"\)/);
  assert.match(hostMain, /--self-test/);
  assert.match(appLib, /companion_launch_requested/);
});
