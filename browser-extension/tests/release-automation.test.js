import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const root = new URL("../../", import.meta.url);
const read = (path) => readFile(new URL(path, root), "utf8");

test("release entrypoint is non-interactive until one final publish confirmation", async () => {
  const [script, batch, latestWrapper] = await Promise.all([
    read("release-mediadrop.ps1"),
    read("release-mediadrop.bat"),
    read("generate-latest.ps1"),
  ]);

  assert.match(script, /\[switch\]\$GenerateLatestOnly/);
  assert.match(script, /\[switch\]\$PreflightOnly/);
  assert.doesNotMatch(script, /Start-Process\s+notepad/i);
  assert.doesNotMatch(script, /\bpause\b/i);
  assert.match(script, /prepare-sidecars\.ps1/);
  assert.match(script, /verify:frontend/);
  assert.match(script, /cargo.+test.+--locked/s);
  assert.match(script, /npm.+audit/s);
  assert.match(script, /cargo.+audit/s);
  assert.match(script, /gitleaks/);
  assert.match(script, /build-native-host\.ps1/);
  assert.match(script, /build-setup\.ps1/);
  assert.doesNotMatch(script, /browser-extension\\distribution\.json/);
  assert.doesNotMatch(script, /-ExtensionInstallUrl/);
  assert.match(script, /Start-Process.+msiexec\.exe/s);
  assert.match(script, /-Wait.+-PassThru/s);
  assert.match(script, /MediaDrop-Extension-/);
  assert.match(script, /SHA256SUMS\.txt/);
  assert.match(script, /build-info\.json/);
  assert.match(script, /--draft/);
  assert.match(script, /--latest/);
  assert.match(batch, /exit \/b %EXIT_CODE%/i);
  assert.match(latestWrapper, /release-mediadrop\.ps1/);
  assert.match(latestWrapper, /-GenerateLatestOnly/);
});

test("branded setup keeps guided extension setup inside the installer", async () => {
  const [setup, builder] = await Promise.all([
    read("installer/setup.nsi"),
    read("build-setup.ps1"),
  ]);

  assert.match(setup, /File \/oname=MediaDrop\.msi/);
  assert.match(setup, /msiexec\.exe/);
  assert.match(setup, /Page custom SetupPage/);
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

test("release compatibility gate covers supported Windows and portable paths", async () => {
  const script = await read("verify-release-compatibility.ps1");

  assert.match(script, /Windows 10 22H2/);
  assert.match(script, /Windows 11/);
  assert.match(script, /0x8664/);
  assert.match(script, /embedBootstrapper/);
  assert.match(script, /Uyumluluk İğüşöç/);
  assert.match(script, /instaloader-helper/);
  assert.match(script, /allowed_origins/);
});

test("public repository CI verifies Windows release inputs and dependency updates", async () => {
  const [workflow, dependabot] = await Promise.all([
    read(".github/workflows/ci.yml"),
    read(".github/dependabot.yml"),
  ]);

  assert.match(workflow, /windows-2022/);
  assert.match(workflow, /prepare-sidecars\.ps1/);
  assert.match(workflow, /verify:frontend/);
  assert.match(workflow, /cargo test --locked/);
  assert.match(workflow, /cargo audit/);
  assert.match(workflow, /gitleaks\/gitleaks-action/);
  assert.match(workflow, /verify-release-compatibility\.ps1/);
  assert.match(dependabot, /package-ecosystem: "npm"/);
  assert.match(dependabot, /package-ecosystem: "cargo"/);
  assert.match(dependabot, /package-ecosystem: "github-actions"/);
});

test("public-history preparation snapshots instead of rewriting the working repository", async () => {
  const script = await read("prepare-public-history.ps1");

  assert.match(script, /git\.exe.+bundle.+create/s);
  assert.match(script, /ls-files.+--others.+--exclude-standard/s);
  assert.match(script, /foreach \(\$relative in \$allSourceFiles\)/);
  assert.match(script, /foreach \(\$relative in \$publicSourceFiles\)/);
  assert.match(script, /MediaDrop 1\.0\.0/);
  assert.match(script, /checkout.+-b.+main/s);
  assert.doesNotMatch(script, /checkout.+--orphan/s);
  assert.doesNotMatch(script, /push.+--force/s);
});
