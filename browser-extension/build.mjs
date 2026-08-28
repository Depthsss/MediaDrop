import { cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const source = dirname(fileURLToPath(import.meta.url));
const root = dirname(source);
const output = join(source, "dist");
const development = process.argv.includes("--dev");
const nativeHostName = development
  ? "com.mab.mediadrop.dev"
  : "com.mab.mediadrop";

async function copyTree(from, to) {
  await mkdir(to, { recursive: true });
  for (const entry of await readdir(from, { withFileTypes: true })) {
    if (["dist", "tests"].includes(entry.name) || entry.name === "build.mjs") continue;
    const sourcePath = join(from, entry.name);
    const targetPath = join(to, entry.name);
    if (entry.isDirectory()) await copyTree(sourcePath, targetPath);
    else await cp(sourcePath, targetPath);
  }
}

await rm(output, { recursive: true, force: true });
await copyTree(source, output);
const workerPath = join(output, "service-worker.js");
const worker = await readFile(workerPath, "utf8");
const hostMarker = 'const HOST_NAME = "com.mab.mediadrop";';
if (!worker.includes(hostMarker)) throw new Error("Native host build marker is missing.");
await writeFile(
  workerPath,
  worker.replace(hostMarker, `const HOST_NAME = "${nativeHostName}";`),
  "utf8",
);
await mkdir(join(output, "shared"), { recursive: true });
await cp(join(root, "src", "features", "quality", "format-model.js"), join(output, "shared", "format-model.js"));
await mkdir(join(output, "icons"), { recursive: true });
for (const [size, name] of [[32, "32x32.png"], [64, "64x64.png"], [128, "128x128.png"]]) {
  await cp(join(root, "src-tauri", "icons", name), join(output, "icons", `icon-${size}.png`));
}
await mkdir(join(output, "popup", "assets"), { recursive: true });
for (const name of [
  "InstrumentSans-Regular.ttf",
  "InstrumentSans-SemiBold.ttf",
  "InstrumentSans-Bold.ttf",
]) {
  await cp(join(root, "src", "assets", "fonts", name), join(output, "popup", "assets", name));
}

const manifestPath = join(output, "manifest.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
if (development && !manifest.key) throw new Error("Development extension key is missing.");
if (!manifest.key) throw new Error("Stable extension public key is missing.");
const forbidden = ["storage", "cookies", "tabs", "webRequest", "unlimitedStorage"];
if (manifest.manifest_version !== 3 || forbidden.some((permission) => manifest.permissions.includes(permission))) {
  throw new Error("Extension manifest permission policy failed.");
}
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
