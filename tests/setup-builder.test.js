import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test("preview setup exposes the fixed custom UI contract", { skip: process.platform !== "win32", timeout: 120_000 }, () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "mediadrop-setup-test-"));
  try {
    const msiPath = path.join(tempRoot, "fixture.msi");
    const outputDirectory = path.join(tempRoot, "out");
    const contractPath = path.join(tempRoot, "ui-contract.json");
    writeFileSync(msiPath, "MediaDrop setup test fixture", "utf8");

    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        path.join(projectRoot, "build-setup.ps1"),
        "-MsiPath",
        msiPath,
        "-Version",
        "1.0.0",
        "-OutputDirectory",
        outputDirectory,
        "-Preview",
      ],
      { cwd: projectRoot, stdio: "pipe" },
    );

    const setupPath = path.join(outputDirectory, "MediaDrop-Setup-1.0.0.exe");
    execFileSync(setupPath, [`/UISELFTEST=${contractPath}`], { cwd: tempRoot, stdio: "pipe" });
    const contract = JSON.parse(readFileSync(contractPath, "utf8"));

    assert.deepEqual(contract, {
      schemaVersion: 1,
      window: { width: 1120, height: 650, fixedAcrossScreens: true },
      screens: ["welcome", "installing", "extension", "done", "error"],
      progress: { startsAt: 0, visualFloor: 2, logoLinked: true },
      defaults: { launchApp: true, extensionSetup: false },
      extensionInstall: {
        mode: "guidedSideload",
        staysInInstaller: true,
        opensInApp: false,
        detectsInstalledBrowsers: true,
        putsDefaultBrowserFirst: true,
        detectsNativeConnection: true,
        supportedBrowsers: ["opera_gx", "opera", "chrome", "edge"],
      },
      actions: [
        "installMsi",
        "launchApp",
        "openBrowserExtensions",
        "copyExtensionPath",
        "continueWithoutExtension",
        "retry",
        "openLog",
      ],
    });
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
});

test("production setup compiles warning-free with the guided extension action", { skip: process.platform !== "win32", timeout: 120_000 }, () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "mediadrop-setup-production-test-"));
  try {
    const msiPath = path.join(tempRoot, "fixture.msi");
    const outputDirectory = path.join(tempRoot, "out");
    writeFileSync(msiPath, "MediaDrop setup test fixture", "utf8");

    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        path.join(projectRoot, "build-setup.ps1"),
        "-MsiPath",
        msiPath,
        "-Version",
        "1.0.0",
        "-OutputDirectory",
        outputDirectory,
      ],
      { cwd: projectRoot, stdio: "pipe" },
    );

    assert.equal(
      readFileSync(path.join(outputDirectory, "MediaDrop-Setup-1.0.0.exe")).subarray(0, 2).toString("ascii"),
      "MZ",
    );
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
});
