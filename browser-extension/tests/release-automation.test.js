import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const root = new URL("../../", import.meta.url);
const read = (path) => readFile(new URL(path, root), "utf8");
const execFileAsync = promisify(execFile);

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
const publicHistoryTest = await fileExists("prepare-public-history.ps1") ? test : test.skip;

async function runReleaseFunctions(testRoot, body) {
  const source = await read("release-mediadrop.ps1");
  const definitions = source
    .slice(0, source.search(/\r?\ntry \{/))
    .replace("$Root = Split-Path -Parent $MyInvocation.MyCommand.Path", "$Root = $env:MEDIADROP_RELEASE_TEST_ROOT");
  const encode = (value) => Buffer.from(value, "utf8").toString("base64");
  const harness = [
    "$ErrorActionPreference = 'Stop'",
    `$env:MEDIADROP_RELEASE_TEST_ROOT = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${encode(testRoot)}'))`,
    `$definitions = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${encode(definitions)}'))`,
    "Invoke-Expression $definitions",
    body,
  ].join("\n");
  const harnessPath = path.join(testRoot, "release-function-harness.ps1");
  await writeFile(harnessPath, harness, "utf8");
  return execFileAsync("powershell.exe", ["-NoProfile", "-NonInteractive", "-File", harnessPath]);
}

releaseOperatorTest("release entrypoint builds locally and requires a hash-bound VM gate before publishing", async () => {
  const [script, batch, latestWrapper, instagramBuilder] = await Promise.all([
    read("release-mediadrop.ps1"),
    read("release-mediadrop.bat"),
    read("generate-latest.ps1"),
    read("tools/instagram-helper/build.ps1"),
  ]);

  assert.match(script, /\[switch\]\$GenerateLatestOnly/);
  assert.match(script, /\[switch\]\$PreflightOnly/);
  assert.match(script, /\[switch\]\$PublishExisting/);
  assert.match(script, /Assert-InstallerVmGate/);
  assert.match(script, /independentlyReviewed/);
  assert.match(script, /Publishing blocked: the clean Windows 10\/11 VM gate/);
  assert.doesNotMatch(script, /Start-Process\s+notepad/i);
  assert.doesNotMatch(script, /\bpause\b/i);
  assert.match(script, /prepare-sidecars\.ps1/);
  assert.doesNotMatch(script, /prepare-sidecars\.ps1"\)\s*,\s*"-VerifyOnly"/);
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
  assert.match(script, /\$objectLines = \(Get-CheckedOutput.+rev-list.+--objects.+HEAD.+\) -split/s);
  assert.doesNotMatch(script, /Get-FileHash/);
  assert.doesNotMatch(instagramBuilder, /Get-FileHash/);
  assert.match(script, /--draft/);
  assert.match(script, /--latest/);
  assert.match(batch, /exit \/b %EXIT_CODE%/i);
  assert.match(latestWrapper, /release-mediadrop\.ps1/);
  assert.match(latestWrapper, /-GenerateLatestOnly/);
});

releaseOperatorTest("release build accepts only the freshly generated MSI and revalidates staged provenance before GitHub", async () => {
  const script = await read("release-mediadrop.ps1");

  assert.match(script, /function Reset-TauriMsiBundle/);
  assert.match(script, /src-tauri\\target\\release\\bundle\\msi/);
  assert.match(script, /Remove-Item -LiteralPath \$msiRoot -Recurse -Force/);
  assert.match(script, /New-Item -ItemType Directory -Path \$msiRoot -Force/);
  assert.match(script, /Reset-TauriMsiBundle\s*\r?\n\s*Invoke-Checked "npm\.cmd" @\("run", "tauri"/);
  assert.doesNotMatch(script, /LastWriteTime(?:Utc)?/);
  assert.match(script, /\$matches\.Count -eq 1/);
  assert.match(script, /\$signatures\.Count -eq 1/);
  assert.match(script, /function Assert-StagedReleaseProvenance/);
  for (const field of [
    "sourceTreeClean",
    "buildFingerprint",
    "msiSha256",
    "setupSha256",
    "extensionSha256",
    "sidecarLockSha256",
  ]) {
    assert.match(script, new RegExp(field));
  }
  assert.match(
    script,
    /if \(\$PublishExisting\) \{[\s\S]*?Invoke-Preflight \$false[\s\S]*?Assert-SourceGitState \$metadata[\s\S]*?Assert-StagedReleaseProvenance \$metadata[\s\S]*?Ensure-GitHubState \$metadata[\s\S]*?Publish-Release/,
  );
  assert.match(
    script,
    /\$metadata = Invoke-Preflight \(-not \$SkipPublish\)[\s\S]*?if \(\$SkipPublish\) \{\s*Assert-LocalBuildSourceState\s*\}[\s\S]*?Invoke-ReleaseBuild \$metadata/,
  );
});

releaseOperatorTest("release functions reject a substring-only MSI version", async () => {
  const testRoot = await mkdtemp(path.join(tmpdir(), "mediadrop-release-msi-"));
  try {
    const msiRoot = path.join(testRoot, "src-tauri", "target", "release", "bundle", "msi");
    await mkdir(msiRoot, { recursive: true });
    await writeFile(path.join(msiRoot, "MediaDrop_11.0.0_x64_en-US.msi"), "stale");
    await writeFile(path.join(msiRoot, "MediaDrop_11.0.0_x64_en-US.msi.sig"), "signature");

    const { stdout } = await runReleaseFunctions(testRoot, `
      try {
        Find-BuiltMsi "1.0.0" | Out-Null
        throw "accepted substring MSI"
      } catch {
        if ($_.Exception.Message -notmatch "requested-version MSI") { throw }
        Write-Output "substring MSI rejected"
      }
    `);
    assert.match(stdout, /substring MSI rejected/);
  } finally {
    await rm(testRoot, { recursive: true, force: true });
  }
});

releaseOperatorTest("release functions reject source state drift after a build snapshot", async () => {
  const testRoot = await mkdtemp(path.join(tmpdir(), "mediadrop-release-source-"));
  try {
    const { stdout } = await runReleaseFunctions(testRoot, `
      $script:commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      $script:status = ""
      $script:fingerprint = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
      function Get-CheckedOutput([string]$Command, [string[]]$Arguments) {
        if ($Arguments -contains "rev-parse") { return $script:commit }
        if ($Arguments -contains "status") { return $script:status }
        throw "unexpected command"
      }
      function Get-ReleaseBuildFingerprint { $script:fingerprint }
      $snapshot = Get-ReleaseSourceState
      $script:commit = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      try {
        Assert-ReleaseSourceState $snapshot | Out-Null
        throw "accepted source drift"
      } catch {
        if ($_.Exception.Message -notmatch "source commit changed") { throw }
        Write-Output "source drift rejected"
      }
    `);
    assert.match(stdout, /source drift rejected/);
  } finally {
    await rm(testRoot, { recursive: true, force: true });
  }
});

releaseOperatorTest("release functions reject an uploadable artifact absent from SHA256SUMS", async () => {
  const testRoot = await mkdtemp(path.join(tmpdir(), "mediadrop-release-assets-"));
  try {
    const artifactRoot = path.join(testRoot, "artifacts");
    await mkdir(artifactRoot, { recursive: true });
    const { stdout } = await runReleaseFunctions(testRoot, `
      $ArtifactRoot = Join-Path $Root "artifacts"
      $metadata = [pscustomobject]@{ Version = "1.0.0" }
      $expected = Get-ExpectedReleaseAssetNames $metadata
      foreach ($name in $expected | Where-Object { $_ -ne "SHA256SUMS.txt" }) {
        [IO.File]::WriteAllText((Join-Path $ArtifactRoot $name), $name)
      }
      $checksumLines = foreach ($name in $expected | Where-Object { $_ -ne "SHA256SUMS.txt" }) {
        "$(Get-Sha256 (Join-Path $ArtifactRoot $name)) *$name"
      }
      [IO.File]::WriteAllLines((Join-Path $ArtifactRoot "SHA256SUMS.txt"), $checksumLines)
      [IO.File]::WriteAllText((Join-Path $ArtifactRoot "unexpected.txt"), "extra")
      try {
        Get-PublishableAssets $metadata "" | Out-Null
        throw "accepted unexpected upload"
      } catch {
        if ($_.Exception.Message -notmatch "unexpected uploadable artifact") { throw }
        Write-Output "extra artifact rejected"
      }
    `);
    assert.match(stdout, /extra artifact rejected/);
  } finally {
    await rm(testRoot, { recursive: true, force: true });
  }
});

test("branded setup keeps guided extension setup inside the installer", async () => {
  const [setup, builder] = await Promise.all([
    read("installer/setup.nsi"),
    read("build-setup.ps1"),
  ]);

  assert.match(setup, /File \/oname=MediaDrop\.msi/);
  assert.match(setup, /File \/oname=mediadrop-installer-worker\.exe/);
  assert.match(setup, /Page custom SetupPage/);
  assert.match(setup, /RequestExecutionLevel user/);
  assert.doesNotMatch(setup, /RequestExecutionLevel admin/);
  assert.doesNotMatch(setup, /\bmsiexec(?:\.exe)?\b/i);
  assert.doesNotMatch(setup, /ShellExecuteExW/i);
  assert.doesNotMatch(setup, /TerminateProcess/i);
  assert.match(setup, /--broker --session-dir/);
  assert.match(setup, /Function SendBrokerCommand/);
  assert.match(setup, /"cancel"/);
  assert.match(setup, /i 1120, i 650/);
  assert.match(setup, /StrCpy \$ExtensionSetup 0/);
  assert.match(setup, /toggle-\*\.bmp/);
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
  assert.match(workflow, /installer\/worker\/Cargo\.toml/);
  assert.match(workflow, /cargo clippy.+test-engine/s);
  assert.match(workflow, /cargo audit/);
  assert.match(workflow, /gitleaks_\$\{version\}_windows_x64\.zip/);
  assert.match(workflow, /Get-FileHash.+SHA256/);
  assert.match(workflow, /verify-release-compatibility\.ps1/);
  assert.match(dependabot, /package-ecosystem: "npm"/);
  assert.match(dependabot, /package-ecosystem: "cargo"/);
  assert.match(dependabot, /package-ecosystem: "github-actions"/);
});

publicHistoryTest("public-history preparation snapshots instead of rewriting the working repository", async () => {
  const script = await read("prepare-public-history.ps1");

  assert.match(script, /git\.exe.+bundle.+create/s);
  assert.match(script, /ls-files.+--others.+--exclude-standard/s);
  assert.match(script, /foreach \(\$relative in \$allSourceFiles\)/);
  assert.match(script, /foreach \(\$relative in \$publicSourceFiles\)/);
  assert.match(script, /MediaDrop \$Version/);
  assert.match(script, /clone --branch main --single-branch --no-tags/);
  assert.match(script, /origin\/main\.\.HEAD/);
  assert.match(script, /checkout.+-b.+main/s);
  assert.doesNotMatch(script, /checkout.+--orphan/s);
  assert.doesNotMatch(script, /push.+--force/s);
});
