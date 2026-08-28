import test from "node:test";
import assert from "node:assert/strict";
import { createHash, createPublicKey } from "node:crypto";
import { readFile } from "node:fs/promises";

const root = new URL("../../", import.meta.url);

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

test("all canonical package manifests publish MediaDrop 1.0.0", async () => {
  const [packageJson, packageLock, cargoToml, tauriConfig, extensionManifest] =
    await Promise.all([
      readFile(new URL("package.json", root), "utf8").then(JSON.parse),
      readFile(new URL("package-lock.json", root), "utf8").then(JSON.parse),
      readFile(new URL("src-tauri/Cargo.toml", root), "utf8"),
      readFile(new URL("src-tauri/tauri.conf.json", root), "utf8").then(JSON.parse),
      readFile(new URL("browser-extension/manifest.json", root), "utf8").then(JSON.parse),
    ]);

  assert.equal(packageJson.version, "1.0.0");
  assert.equal(packageLock.version, "1.0.0");
  assert.equal(packageLock.packages[""].version, "1.0.0");
  assert.match(cargoToml, /^version = "1\.0\.0"$/m);
  assert.equal(tauriConfig.version, "1.0.0");
  assert.equal(extensionManifest.version, "1.0.0");
});

test("production extension keeps the stable public key and extension identity", async () => {
  const manifest = JSON.parse(
    await readFile(new URL("browser-extension/dist/manifest.json", root), "utf8"),
  );

  assert.equal(typeof manifest.key, "string");
  assert.equal(extensionIdFromKey(manifest.key), "gifnifkakikpndieohkijmjccmmikalm");
});
