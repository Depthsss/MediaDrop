import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const setupVersion = JSON.parse(readFileSync(path.join(projectRoot, "package.json"), "utf8")).version;
const removeTempRoot = (tempRoot) =>
  rmSync(tempRoot, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });

test("production setup delegates MSI work to the normal-integrity broker", () => {
  const source = readFileSync(path.join(projectRoot, "installer", "setup.nsi"), "utf8");
  assert.match(source, /^RequestExecutionLevel user$/m);
  assert.match(source, /File \/oname=mediadrop-installer-worker\.exe "\$\{WORKER_PATH\}"/);
  assert.match(source, /--broker --session-dir/);
  assert.match(source, /SetOutPath "\$SessionDir"[\s\S]*File \/oname=MediaDrop\.msi/);
  assert.match(source, /Exec '\"\$SessionDir\\mediadrop-installer-worker\.exe\" --broker/);
  assert.match(source, /status\.ini/);
  assert.match(source, /command\.ini/);
  assert.match(source, /ReadINIStr/);
  assert.match(source, /CreateMutexW[^\r\n]+\?e/);
  assert.match(source, /\$BrokerStarted == 0[\s\S]*ShowControl \$RetryHit/);
  assert.doesNotMatch(source, /\bmsiexec(?:\.exe)?\b/i);
  assert.doesNotMatch(source, /ShellExecuteExW/i);
  assert.doesNotMatch(source, /TerminateProcess/i);
  assert.doesNotMatch(source, /GetExitCodeProcess/i);
  assert.doesNotMatch(source, /Var InstallProcess/);
});

test("setup grows buttons on hover and slides toggles without adding a UI runtime", () => {
  const setup = readFileSync(path.join(projectRoot, "installer", "setup.nsi"), "utf8");
  const builder = readFileSync(path.join(projectRoot, "build-setup.ps1"), "utf8");

  assert.match(builder, /for \(\$frame = 0; \$frame -le 6; \$frame\+\+\)/);
  assert.match(builder, /for \(\$frame = 1; \$frame -le 4; \$frame\+\+\)/);
  assert.match(setup, /Function AnimateToggle[\s\S]*?Sleep 16[\s\S]*?FunctionEnd/);
  assert.match(setup, /Function ApplyHoverFrame[\s\S]*?hover-\$HoverTarget-\$HoverProgress\.bmp[\s\S]*?FunctionEnd/);
  assert.doesNotMatch(setup, /SetWindowPos\(p \$HoverAccent[^\r\n]+i 3,/);
  assert.match(setup, /SystemParametersInfoW\(i 0x1042/);
  assert.match(setup, /File "\$\{ASSET_DIR\}\\toggle-\*\.bmp"/);
  assert.match(setup, /File "\$\{ASSET_DIR\}\\hover-\*\.bmp"/);
  assert.match(builder, /\$WorkerBuildKind = "production"/);
  assert.match(builder, /if \(\$TestEngine\) \{ \$WorkerBuildKind = "test-engine" \}/);
  assert.match(builder, /--target-dir", \$WorkerTargetRoot/);
});

test("setup makes the branded first frame ready before showing the large offline payload", () => {
  const setup = readFileSync(path.join(projectRoot, "installer", "setup.nsi"), "utf8");
  const reservedDialog = setup.indexOf("ReserveFile /plugin nsDialogs.dll");
  const embeddedMsi = setup.indexOf('File /oname=MediaDrop.msi "${MSI_PATH}"');

  assert.ok(reservedDialog >= 0, "nsDialogs must be reserved at the front of the solid data block");
  assert.ok(reservedDialog < embeddedMsi, "the first-page plug-in must precede the embedded MSI");
  assert.match(setup, /Function \.onGUIInit[\s\S]*?ShowWindow \$HWNDPARENT \$\{SW_HIDE\}[\s\S]*?FunctionEnd/);
  assert.match(setup, /Call ShowWelcome[\s\S]*?ShowWindow \$HWNDPARENT \$\{SW_SHOW\}[\s\S]*?nsDialogs::Show/);
});

test("extension guide keeps dynamic controls clickable and never blocks on browser-only state", () => {
  const setup = readFileSync(path.join(projectRoot, "installer", "setup.nsi"), "utf8");
  const builder = readFileSync(path.join(projectRoot, "build-setup.ps1"), "utf8");

  assert.match(setup, /Function RefreshDynamicUi[\s\S]*?RedrawWindow[^\r\n]+i 0x1A1[\s\S]*?FunctionEnd/);
  assert.doesNotMatch(setup, /Function RefreshDynamicUi[\s\S]*?RedrawWindow[^\r\n]+i 0x185[\s\S]*?FunctionEnd/);
  assert.match(builder, /\$statusBrush[\s\S]*?FillRectangle\(\$statusBrush, 494, 488, 556, 47\)/);
  assert.match(setup, /Function ApplyDynamicHover[\s\S]*?browser_[0-3][\s\S]*?extension_primary[\s\S]*?FunctionEnd/);
  assert.match(setup, /StrCpy \$3 "opera:extensions"/);
  assert.doesNotMatch(setup, /opera:\/\/extensions/);
  assert.match(setup, /Function OnExtensionPrimary[\s\S]*?copy_extension_address:\$SelectedBrowserPage[\s\S]*?Exec '\"\$SelectedBrowserExe\" -noautoupdate --'[\s\S]*?FunctionEnd/);
  assert.match(setup, /Function OnExtensionConfirm[\s\S]*?Call ShowDone[\s\S]*?FunctionEnd/);
  assert.match(setup, /Eklentiyi yükledim/);
  assert.match(setup, /Ctrl\+Shift\+E/);
  assert.match(setup, /StrCpy \$4 \$ExtensionPath/);
  assert.match(setup, /lstrcpyW\(p \$3, w "\$4"\)/);
  assert.match(setup, /RecordPreviewAction "copy_extension_path"/);
  assert.match(setup, /RecordPreviewAction "reveal_extension_path"/);
  assert.match(setup, /explorer\.exe[\s\S]*?\/select,[\s\S]*?manifest\.json/);
  assert.match(setup, /CreateOverlayButton \$ExtensionCopyHit[^\r\n]+"Yolu kopyala"/);
  assert.match(setup, /CreateOverlayButton \$ExtensionRevealHit[^\r\n]+"Klasörü göster"/);
  assert.match(setup, /SetCtlColors \$ProgressNumber 0xF6F5F1 0x17181C/);
  assert.match(setup, /SetCtlColors \$ProgressCurrent 0xCCCBC5 0x17181C/);
});

test("every custom setup click handler consumes its callback argument", () => {
  const source = readFileSync(path.join(projectRoot, "installer", "setup.nsi"), "utf8");
  const callbacks = new Set([
    ...[...source.matchAll(/\$\{NSD_OnClick\}\s+\$\w+\s+(\w+)/g)].map((match) => match[1]),
    ...[...source.matchAll(/!insertmacro Create(?:Hit|TextButton)[^\r\n]+\s(\w+)\s*$/gm)].map(
      (match) => match[1],
    ),
  ]);
  assert.ok(callbacks.size > 0);
  for (const callback of callbacks) {
    assert.match(source, new RegExp(`Function ${callback}\\r?\\n\\s+Pop \\$0`), callback);
  }
});

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
        setupVersion,
        "-OutputDirectory",
        outputDirectory,
        "-Preview",
      ],
      { cwd: projectRoot, stdio: "pipe" },
    );

    const setupPath = path.join(outputDirectory, `MediaDrop-Setup-${setupVersion}.exe`);
    execFileSync(setupPath, [`/UISELFTEST=${contractPath}`], { cwd: tempRoot, stdio: "pipe" });
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        path.join(projectRoot, "tests", "setup-ui-smoke.ps1"),
        "-SetupPath",
        setupPath,
      ],
      { cwd: tempRoot, stdio: "pipe" },
    );
    const contract = JSON.parse(readFileSync(contractPath, "utf8"));

    assert.deepEqual(contract, {
      schemaVersion: 1,
      window: { width: 1120, height: 650, fixedAcrossScreens: true },
      screens: ["welcome", "installing", "extension", "done", "error"],
      progress: { startsAt: 0, visualFloor: 2, logoLinked: true },
      motion: {
        hoverFeedback: true,
        hoverStyle: "buttonScale",
        hoverFrames: 4,
        toggleFrames: 7,
        respectsReducedMotion: true,
      },
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
        "revealExtensionFolder",
        "continueWithoutExtension",
        "retry",
        "openLog",
      ],
    });
  } finally {
    removeTempRoot(tempRoot);
  }
});

test("production setup rejects a payload that is not the MediaDrop MSI", { skip: process.platform !== "win32", timeout: 120_000 }, () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "mediadrop-setup-production-test-"));
  try {
    const msiPath = path.join(tempRoot, "fixture.msi");
    const outputDirectory = path.join(tempRoot, "out");
    writeFileSync(msiPath, "MediaDrop setup test fixture", "utf8");

    assert.throws(() =>
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
          setupVersion,
          "-OutputDirectory",
          outputDirectory,
        ],
        { cwd: projectRoot, stdio: "pipe" },
      ),
    );
  } finally {
    removeTempRoot(tempRoot);
  }
});

test("real custom UI drives broker cancellation, rollback, retry, success, and close", { skip: process.platform !== "win32", timeout: 180_000 }, () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "MediaDrop Çağrı 你好-"));
  try {
    const msiPath = path.join(tempRoot, "fixture.msi");
    const outputDirectory = path.join(tempRoot, "out");
    writeFileSync(msiPath, "MediaDrop lifecycle fixture", "utf8");
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
        setupVersion,
        "-OutputDirectory",
        outputDirectory,
        "-LifecycleTest",
      ],
      { cwd: projectRoot, stdio: "pipe" },
    );
    const setupPath = path.join(outputDirectory, `MediaDrop-Setup-${setupVersion}.exe`);
    for (let index = 0; index < 3; index += 1) {
      execFileSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-ExecutionPolicy",
          "Bypass",
          "-File",
          path.join(projectRoot, "tests", "setup-ui-smoke.ps1"),
          "-SetupPath",
          setupPath,
          "-Lifecycle",
        ],
        { cwd: tempRoot, stdio: "pipe" },
      );
    }
  } finally {
    removeTempRoot(tempRoot);
  }
});

const releaseMsi = process.env.MEDIADROP_RELEASE_MSI;
test("production setup compiles from the real MediaDrop MSI", { skip: process.platform !== "win32" || !releaseMsi, timeout: 180_000 }, () => {
  const tempRoot = mkdtempSync(path.join(os.tmpdir(), "mediadrop-real-setup-test-"));
  try {
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        path.join(projectRoot, "build-setup.ps1"),
        "-MsiPath",
        releaseMsi,
        "-Version",
        setupVersion,
        "-OutputDirectory",
        tempRoot,
      ],
      { cwd: projectRoot, stdio: "pipe" },
    );
    const setupBytes = readFileSync(path.join(tempRoot, `MediaDrop-Setup-${setupVersion}.exe`));
    assert.equal(setupBytes.subarray(0, 2).toString("ascii"), "MZ");
    assert.equal(setupBytes.indexOf(Buffer.from("requireAdministrator")), -1);
    assert.notEqual(setupBytes.indexOf(Buffer.from("asInvoker")), -1);
    assert.equal(setupBytes.indexOf(Buffer.from("MEDIADROP_INSTALLER_TEST_ENGINE_SCENARIO")), -1);
  } finally {
    removeTempRoot(tempRoot);
  }
});
