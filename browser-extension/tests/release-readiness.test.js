import test from "node:test";
import assert from "node:assert/strict";
import { createHash, createPublicKey } from "node:crypto";
import { readFile } from "node:fs/promises";

const root = new URL("../../", import.meta.url);

async function fileExists(relativePath) {
  try {
    await readFile(new URL(relativePath, root));
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

const releaseOperatorTest = await fileExists("release-mediadrop.ps1") ? test : test.skip;

function extensionIdFromKey(key) {
  const publicKey = createPublicKey({
    key: Buffer.from(key, "base64"),
    format: "der",
    type: "spki",
  }).export({ format: "der", type: "spki" });
  return [...createHash("sha256").update(publicKey).digest().subarray(0, 16)]
    .flatMap((byte) => [byte >> 4, byte & 15])
    .map((value) => String.fromCharCode(97 + value))
    .join("");
}

test("all canonical package manifests publish MediaDrop 1.0.1", async () => {
  const expectedVersion = "1.0.1";
  const [
    packageJson,
    packageLock,
    cargoToml,
    cargoLock,
    tauriConfig,
    extensionManifest,
    setupSource,
    setupBuilder,
    workerCargo,
    workerLock,
    workerResources,
    workerManifest,
    componentCargo,
    componentLock,
    componentWorkerResources,
    indexHtml,
  ] =
    await Promise.all([
      readFile(new URL("package.json", root), "utf8").then(JSON.parse),
      readFile(new URL("package-lock.json", root), "utf8").then(JSON.parse),
      readFile(new URL("src-tauri/Cargo.toml", root), "utf8"),
      readFile(new URL("src-tauri/Cargo.lock", root), "utf8"),
      readFile(new URL("src-tauri/tauri.conf.json", root), "utf8").then(JSON.parse),
      readFile(new URL("browser-extension/manifest.json", root), "utf8").then(JSON.parse),
      readFile(new URL("installer/setup.nsi", root), "utf8"),
      readFile(new URL("build-setup.ps1", root), "utf8"),
      readFile(new URL("installer/worker/Cargo.toml", root), "utf8"),
      readFile(new URL("installer/worker/Cargo.lock", root), "utf8"),
      readFile(new URL("installer/worker/worker.rc", root), "utf8"),
      readFile(new URL("installer/worker/worker.manifest", root), "utf8"),
      readFile(new URL("component-update/Cargo.toml", root), "utf8"),
      readFile(new URL("component-update/Cargo.lock", root), "utf8"),
      readFile(new URL("installer/worker/component-worker.rc", root), "utf8"),
      readFile(new URL("src/index.html", root), "utf8"),
    ]);

  assert.equal(packageJson.version, expectedVersion);
  assert.equal(packageLock.version, expectedVersion);
  assert.equal(packageLock.packages[""].version, expectedVersion);
  assert.match(cargoToml, /^version = "1\.0\.1"$/m);
  assert.match(cargoLock, /name = "mediadrop"\r?\nversion = "1\.0\.1"/);
  assert.equal(tauriConfig.version, expectedVersion);
  assert.equal(extensionManifest.version, expectedVersion);
  assert.match(setupSource, /!define APP_VERSION "1\.0\.1"/);
  assert.match(setupBuilder, /\[string\]\$Version = "1\.0\.1"/);
  assert.match(workerCargo, /^version = "1\.0\.1"$/m);
  assert.match(workerLock, /name = "mediadrop-installer-worker"\r?\nversion = "1\.0\.1"/);
  assert.match(workerResources, /FILEVERSION 1,0,1,0/);
  assert.match(workerResources, /VALUE "ProductVersion", "1\.0\.1\\0"/);
  assert.match(workerManifest, /assemblyIdentity version="1\.0\.1\.0"/);
  assert.match(componentCargo, /^version = "1\.0\.1"$/m);
  assert.match(componentLock, /name = "mediadrop-component-update"\r?\nversion = "1\.0\.1"/);
  assert.match(componentWorkerResources, /FILEVERSION 1,0,1,0/);
  assert.match(indexHtml, /data-fallback="1\.0\.1">v1\.0\.1/);
});

test("1.0.1 preserves stable application, updater, MSI and extension identities", async () => {
  const [tauriConfig, extensionManifest] = await Promise.all([
    readFile(new URL("src-tauri/tauri.conf.json", root), "utf8").then(JSON.parse),
    readFile(new URL("browser-extension/manifest.json", root), "utf8").then(JSON.parse),
  ]);

  assert.equal(tauriConfig.identifier, "com.mab.mediadrop");
  assert.equal(
    tauriConfig.bundle.windows.wix.upgradeCode,
    "8585b38d-5f90-4110-b089-6b89a3fb6339",
  );
  assert.equal(
    tauriConfig.plugins.updater.pubkey,
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENGNTNBRTkzRDM5MzY1RgpSV1JmTmprOTZUcjFES051RStNeTdyNHlUSlJLYnlHSkVBSjFIamhibHY5UURoV1J4dE56UVd4MQo=",
  );
  assert.equal(extensionIdFromKey(extensionManifest.key), "gifnifkakikpndieohkijmjccmmikalm");
});

releaseOperatorTest("release validation includes installer worker metadata, notes, and generated updater metadata", async () => {
  const script = await readFile(new URL("release-mediadrop.ps1", root), "utf8");

  for (const source of [
    "installer\\worker\\Cargo.toml",
    "installer\\worker\\Cargo.lock",
    "installer\\worker\\worker.rc",
    "installer\\worker\\worker.manifest",
    "installer\\worker\\component-worker.rc",
    "component-update\\Cargo.toml",
    "component-update\\Cargo.lock",
    "installer\\setup.nsi",
    "build-setup.ps1",
    "src\\index.html",
    "release-notes.md",
    "latest.json",
  ]) {
    assert.match(script, new RegExp(source.replace(/\\/g, "\\\\")));
  }
  assert.match(script, /Release notes heading does not match/);
  assert.match(script, /Generated latest\.json version does not match/);
  assert.match(script, /Branded setup version does not match/);
  assert.match(script, /Setup builder default version does not match/);
  assert.match(script, /User-visible fallback version does not match/);
});

test("production yt-dlp updates are signed components and never raw self-updates", async () => {
  const [appSource, componentSource, config] = await Promise.all([
    readFile(new URL("src-tauri/src/app_impl.rs", root), "utf8"),
    readFile(new URL("src-tauri/src/component_updates.rs", root), "utf8"),
    readFile(new URL("src-tauri/tauri.conf.json", root), "utf8").then(JSON.parse),
  ]);

  assert.doesNotMatch(appSource, /command\.arg\("-U"\)/);
  assert.match(appSource, /#\[cfg\(debug_assertions\)\]\s*fn find_in_path/);
  assert.match(componentSource, /verify_signed_manifest/);
  assert.match(componentSource, /ComponentSessions/);
  assert.match(componentSource, /components-stable\/component-manifest\.json/);
  assert(config.bundle.externalBin.includes("binaries/mediadrop-component-worker"));
});

test("production extension keeps the stable public key and extension identity", async () => {
  const manifest = JSON.parse(
    await readFile(new URL("browser-extension/dist/manifest.json", root), "utf8"),
  );

  assert.equal(typeof manifest.key, "string");
  assert.equal(extensionIdFromKey(manifest.key), "gifnifkakikpndieohkijmjccmmikalm");
});
