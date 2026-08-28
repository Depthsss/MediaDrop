import test from "node:test";
import assert from "node:assert/strict";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = new URL("../../", import.meta.url);

test("sidecar lock describes every bundled executable without repository binaries", async () => {
  const lock = JSON.parse(
    await readFile(new URL("src-tauri/binaries/sidecars.lock.json", root), "utf8"),
  );
  const expected = [
    "aria2c-x86_64-pc-windows-msvc.exe",
    "deno-x86_64-pc-windows-msvc.exe",
    "ffmpeg-x86_64-pc-windows-msvc.exe",
    "ffprobe-x86_64-pc-windows-msvc.exe",
    "gallery-dl-x86_64-pc-windows-msvc.exe",
    "instaloader-helper-x86_64-pc-windows-msvc.exe",
    "yt-dlp-x86_64-pc-windows-msvc.exe",
  ];

  assert.deepEqual(lock.sidecars.map((item) => item.file).sort(), expected);
  for (const item of lock.sidecars) {
    assert.match(item.version, /\S/);
    if (item.buildScript) {
      assert.equal(item.name, "instaloader-helper");
      assert.match(item.buildScript, /^tools\/instagram-helper\/build\.ps1$/);
      assert.equal(item.url, undefined);
      assert.equal(item.sha256, undefined);
    } else {
      assert.match(item.url, /^https:\/\//);
      assert.match(item.sha256, /^[A-F0-9]{64}$/);
    }
    assert.match(item.license, /\S/);
    assert.match(item.sourceUrl, /^https:\/\//);
  }
});

test("sidecar preparation verifies the exact local release inputs", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      new URL("prepare-sidecars.ps1", root).pathname.slice(1),
      "-VerifyOnly",
    ],
    { cwd: new URL("../..", import.meta.url), encoding: "utf8" },
  );

  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /7 sidecars verified/);
});

test("a source-built sidecar is created without a future release download", async () => {
  const tempRoot = await mkdtemp(join(tmpdir(), "mediadrop-sidecar-test-"));
  try {
    await mkdir(join(tempRoot, "src-tauri", "binaries"), { recursive: true });
    await mkdir(join(tempRoot, "tools", "helper"), { recursive: true });
    await copyFile(new URL("prepare-sidecars.ps1", root), join(tempRoot, "prepare-sidecars.ps1"));
    await writeFile(
      join(tempRoot, "src-tauri", "binaries", "sidecars.lock.json"),
      `${JSON.stringify({
        schemaVersion: 1,
        target: "x86_64-pc-windows-msvc",
        sidecars: [{
          name: "helper",
          version: "1.0.0",
          file: "helper-x86_64-pc-windows-msvc.exe",
          buildScript: "tools/helper/build.ps1",
          license: "MIT",
          sourceUrl: "https://example.invalid/source",
        }],
      })}\n`,
    );
    await writeFile(
      join(tempRoot, "tools", "helper", "build.ps1"),
      `$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))\n` +
        `$target = Join-Path $root "src-tauri\\binaries\\helper-x86_64-pc-windows-msvc.exe"\n` +
        `[IO.File]::WriteAllBytes($target, [byte[]](1, 2, 3))\n`,
    );

    const result = spawnSync(
      "powershell.exe",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(tempRoot, "prepare-sidecars.ps1")],
      { cwd: tempRoot, encoding: "utf8" },
    );

    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /BUILT helper 1\.0\.0/);
    assert.match(result.stdout, /1 sidecars verified/);
  } finally {
    await rm(tempRoot, { recursive: true, force: true });
  }
});
