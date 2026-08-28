import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const root = new URL("../../", import.meta.url);
const read = (path) => readFile(new URL(path, root), "utf8");

test("branded setup keeps guided extension setup inside the installer", async () => {
  const [setup, builder] = await Promise.all([
    read("installer/setup.nsi"),
    read("build-setup.ps1"),
  ]);

  assert.match(setup, /File \/oname=MediaDrop\.msi/);
  assert.match(setup, /msiexec\.exe/);
  assert.match(setup, /Page custom SetupPage/);
  assert.match(setup, /SetWindowPos\(p \$Background, p 1/);
  assert.match(setup, /GetDlgItem \$0 \$HWNDPARENT 1/);
  assert.match(setup, /RequestExecutionLevel user/);
  assert.doesNotMatch(setup, /RequestExecutionLevel admin/);
  assert.match(setup, /ShellExecuteExW[\s\S]*t "runas"/);
  assert.match(setup, /ExecShellWait "runas"/);
  assert.match(setup, /InstallExitCode == 1223[\s\S]*StrCpy \$InstallExitCode 1602/);
  assert.match(setup, /i 1120, i 650/);
  assert.match(setup, /StrCpy \$ExtensionSetup 0/);
  assert.match(setup, /toggle-off\.bmp/);
  assert.match(setup, /Function ShowDone[\s\S]*Call SyncDoneExtensionToggle[\s\S]*FunctionEnd/);
  assert.match(setup, /Function ShowExtensionSetup/);
  assert.match(setup, /Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\https\\UserChoice/);
  assert.match(setup, /PROGRAMFILES64\\Opera GX\\launcher\.exe/);
  assert.match(setup, /Opera GXStable/);
  assert.match(setup, /App Paths\\chrome\.exe/);
  assert.match(setup, /App Paths\\msedge\.exe/);
  assert.match(setup, /CreateEventW/);
  assert.match(setup, /WaitForSingleObject/);
  assert.match(setup, /--companion/);
  assert.doesNotMatch(setup, /--extension-setup/);
  assert.doesNotMatch(setup, /EXTENSION_INSTALL_URL/);
  assert.doesNotMatch(setup, /--load-extension/);
  assert.doesNotMatch(setup, /Software\\Policies/i);
  assert.match(setup, /VIProductVersion "\$\{APP_VERSION\}\.0"/);
  assert.match(setup, /VIAddVersionKey.+"ProductVersion"/);
  assert.match(builder, /MediaDrop-Setup-/);
  assert.match(builder, /makensis\.exe/);
  assert.doesNotMatch(builder, /addons\.opera\.com/);
});

test("distribution compatibility gate covers supported Windows and portable paths", async () => {
  const script = await read("verify-release-compatibility.ps1");

  assert.match(script, /Windows 10 22H2/);
  assert.match(script, /Windows 11/);
  assert.match(script, /0x8664/);
  assert.match(script, /embedBootstrapper/);
  assert.match(script, /Uyumluluk İğüşöç/);
  assert.match(script, /instaloader-helper/);
  assert.match(script, /allowed_origins/);
});

test("public repository CI verifies Windows build inputs and dependency updates", async () => {
  const [workflow, dependabot] = await Promise.all([
    read(".github/workflows/ci.yml"),
    read(".github/dependabot.yml"),
  ]);

  assert.match(workflow, /windows-2022/);
  assert.match(workflow, /prepare-sidecars\.ps1/);
  assert.match(workflow, /verify:frontend/);
  assert.match(workflow, /cargo test --locked/);
  assert.match(workflow, /cargo audit/);
  assert.match(workflow, /gitleaks_\$\{version\}_windows_x64\.zip/);
  assert.match(workflow, /Get-FileHash.+SHA256/);
  assert.match(workflow, /verify-release-compatibility\.ps1/);
  assert.match(dependabot, /package-ecosystem: "npm"/);
  assert.match(dependabot, /package-ecosystem: "cargo"/);
  assert.match(dependabot, /package-ecosystem: "github-actions"/);
});
