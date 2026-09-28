import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("../scripts/sign-macos-tray.sh", import.meta.url));
const bundleScript = fileURLToPath(new URL("../scripts/bundle-macos-app.sh", import.meta.url));
const certificate = "Apple Development: Fixture (TESTTEAM)";
const identityListing = `  1) ${"0".repeat(40)} "${certificate}"\n     1 valid identities found`;

// All commands and paths are fixtures. No real keychain, certificate, HOME or app.
function fixture({ bundle = false, plist = true, identity = identityListing, explicit, verifyStatus = "0", signStatus = "0", securityStatus = "0" } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "aiub-signing-"));
  const bin = path.join(root, "bin");
  const home = path.join(root, "home");
  mkdirSync(bin);
  mkdirSync(home);
  const target = path.join(root, bundle ? "AI Usage.app" : "ai-usagebar-tray");
  if (bundle) {
    mkdirSync(path.join(target, "Contents"), { recursive: true });
    if (plist) writeFileSync(path.join(target, "Contents/Info.plist"), "fixture plist\n");
  } else {
    writeFileSync(target, "fixture binary\n");
  }
  const log = path.join(root, "calls.log");
  writeFileSync(path.join(bin, "security"), '#!/bin/sh\nprintf "%s\\n" "$MOCK_IDENTITIES"\nexit "$MOCK_SECURITY_STATUS"\n', { mode: 0o755 });
  writeFileSync(path.join(bin, "plutil"), '#!/bin/sh\nprintf "%s\\n" "ai-usagebar-tray"\n', { mode: 0o755 });
  writeFileSync(path.join(bin, "codesign"), `#!/bin/sh
{ printf 'CALL\\n'; printf '%s\\n' "$@"; } >> "$SIGN_LOG"
if [ "$1" = --verify ]; then exit "$MOCK_VERIFY_STATUS"; fi
exit "$MOCK_SIGN_STATUS"
`, { mode: 0o755 });
  const env = {
    PATH: `${bin}:/usr/bin:/bin`, HOME: home,
    MOCK_IDENTITIES: identity, SIGN_LOG: log,
    MOCK_VERIFY_STATUS: verifyStatus, MOCK_SIGN_STATUS: signStatus, MOCK_SECURITY_STATUS: securityStatus,
  };
  if (explicit !== undefined) env.CODESIGN_IDENTITY = explicit;
  const result = spawnSync("/bin/bash", [script, target], { env, encoding: "utf8" });
  const calls = () => {
    try { return readFileSync(log, "utf8").split("CALL\n").filter(Boolean).map(call => call.trimEnd().split("\n")); }
    catch (error) { if (error.code === "ENOENT") return []; throw error; }
  };
  return { ...result, target, calls };
}

const nativeTest = (name, fn) => test(name, { skip: process.platform === "win32" }, fn);

nativeTest("standalone binary retains its stable identifier", () => {
  const result = fixture();
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.calls()[0], ["--force", "--sign", certificate, "--identifier", "com.akitaonrails.ai-usagebar-tray", result.target]);
});

nativeTest("bundle signing preserves the existing CFBundleIdentifier", () => {
  const result = fixture({ bundle: true });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.calls()[0], ["--force", "--sign", certificate, "--identifier", "ai-usagebar-tray", result.target]);
});

nativeTest("missing certificate fails instead of silently changing to ad hoc", () => {
  const result = fixture({ identity: "     0 valid identities found" });
  assert.notEqual(result.status, 0);
  assert.deepEqual(result.calls(), []);
  assert.match(result.stderr, /CODESIGN_IDENTITY/);
});

nativeTest("ad hoc signing remains an explicit development choice", () => {
  const result = fixture({ explicit: "-", identity: "" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.calls()[0][2], "-");
  assert.match(result.stderr, /ad.hoc/i);
});

nativeTest("keychain lookup failure gives an actionable error and never signs", () => {
  const result = fixture({ securityStatus: "5" });
  assert.notEqual(result.status, 0);
  assert.deepEqual(result.calls(), []);
  assert.match(result.stderr, /CODESIGN_IDENTITY/);
});

nativeTest("bundle without Info.plist fails before signing", () => {
  const result = fixture({ bundle: true, plist: false });
  assert.notEqual(result.status, 0);
  assert.deepEqual(result.calls(), []);
  assert.match(result.stderr, /Info.plist/);
});

nativeTest("successful signing is followed by strict verification of the same target", () => {
  const result = fixture({ bundle: true });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.calls()[1], ["--verify", "--strict", result.target]);
});

nativeTest("signing failure stops before verification", () => {
  const result = fixture({ signStatus: "4" });
  assert.equal(result.status, 4);
  assert.equal(result.calls().length, 1);
});

nativeTest("verification failure remains a failed command", () => {
  const result = fixture({ verifyStatus: "7" });
  assert.equal(result.status, 7);
  assert.equal(result.calls().length, 2);
});

// A throwaway repo copy with every external command faked. PATH holds only the
// fixture bin, so a trash command installed on the machine is never found and
// nothing reaches the real Trash, target/ or /tmp/claude-backups.
function bundleFixture({ trashCommand = false } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "aiub-bundle-"));
  const repo = path.join(root, "repo");
  const bin = path.join(root, "bin");
  const trashDir = path.join(root, "Trash");
  for (const dir of [path.join(repo, "scripts"), bin, trashDir, path.join(root, "tmp")]) mkdirSync(dir, { recursive: true });
  copyFileSync(bundleScript, path.join(repo, "scripts/bundle-macos-app.sh"));
  writeFileSync(path.join(repo, "scripts/sign-macos-tray.sh"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  writeFileSync(path.join(repo, "Cargo.toml"), '[package]\nname = "fixture"\nversion = "9.9.9"\n');
  writeFileSync(path.join(repo, "LICENSE"), "fixture license\n");
  for (const tool of ["dirname", "sed", "mktemp", "mkdir", "cp", "mv"]) {
    const real = ["/bin", "/usr/bin"].map(dir => path.join(dir, tool)).find(existsSync);
    symlinkSync(real, path.join(bin, tool));
  }
  const log = path.join(root, "calls.log");
  const record = name => `{ printf 'CALL ${name}\\n'; printf '%s\\n' "$@"; } >> "${log}"\n`;
  const fakeTrash = name => `#!/bin/sh\n${record(name)}[ "$MOCK_TRASH_STATUS" = 0 ] || exit "$MOCK_TRASH_STATUS"\nfor last; do :; done\nexec /bin/mv "$last" "${trashDir}/$$.app"\n`;
  const mocks = {
    uname: '#!/bin/sh\ncase "$1" in -s) echo Darwin ;; -m) echo arm64 ;; esac\n',
    cargo: '#!/bin/sh\n/bin/mkdir -p target/release && printf "%s\\n" "$MOCK_BUILD" > target/release/ai-usagebar-tray\n',
    lipo: "#!/bin/sh\necho arm64\n",
    plutil: "#!/bin/sh\nexit 0\n",
    codesign: "#!/bin/sh\nexit 0\n",
    date: '#!/bin/sh\necho 20260928_120000\n',
    osascript: fakeTrash("osascript"),
  };
  if (trashCommand) mocks.trash = fakeTrash("trash");
  for (const [name, body] of Object.entries(mocks)) writeFileSync(path.join(bin, name), body, { mode: 0o755 });

  const app = path.join(repo, "target/release/AI Usage.app");
  const backups = path.join(root, "backups");
  const build = (label, { trashStatus = "0" } = {}) => spawnSync("/bin/bash", [path.join(repo, "scripts/bundle-macos-app.sh")], {
    env: { PATH: bin, HOME: path.join(root, "home"), TMPDIR: path.join(root, "tmp"), BACKUP_ROOT: backups, MOCK_BUILD: label, MOCK_TRASH_STATUS: trashStatus },
    encoding: "utf8",
  });
  const exeIn = bundle => readFileSync(path.join(bundle, "Contents/MacOS/ai-usagebar-tray"), "utf8").trim();
  const calls = () => existsSync(log) ? readFileSync(log, "utf8").split("CALL ").filter(Boolean).map(call => call.trimEnd().split("\n")) : [];
  const backupBuilds = () => existsSync(backups) ? readdirSync(backups).sort().map(dir => exeIn(path.join(backups, dir, "AI Usage.app"))) : [];
  const trashedBuilds = () => readdirSync(trashDir).map(entry => exeIn(path.join(trashDir, entry)));
  return { app, build, exeIn, calls, backupBuilds, trashedBuilds };
}

nativeTest("a second bundle without the trash command uses Foundation and keeps a backup", () => {
  const f = bundleFixture();
  const first = f.build("build 1");
  assert.equal(first.status, 0, first.stderr);
  assert.deepEqual(f.calls(), [], "nothing to replace on the first build");

  const second = f.build("build 2");
  assert.equal(second.status, 0, second.stderr);
  assert.equal(f.exeIn(f.app), "build 2");
  assert.deepEqual(f.trashedBuilds(), ["build 1"]);
  assert.deepEqual(f.backupBuilds(), ["build 1"]);
  const [call] = f.calls();
  assert.deepEqual([call[0], call[1], call[2], call[3]], ["osascript", "-l", "JavaScript", "-e"]);
  const source = call.slice(4, -1).join("\n");
  assert.match(source, /NSFileManager\.defaultManager\.trashItemAtURLResultingItemURLError/);
  assert.doesNotMatch(source, /Finder|System Events|display/);
  assert.equal(call.at(-1), f.app);
});

nativeTest("the trash command is used when it exists", () => {
  const f = bundleFixture({ trashCommand: true });
  assert.equal(f.build("build 1").status, 0);
  const second = f.build("build 2");
  assert.equal(second.status, 0, second.stderr);
  assert.deepEqual(f.calls().map(call => call[0]), ["trash"]);
  assert.deepEqual(f.trashedBuilds(), ["build 1"]);
  assert.equal(f.exeIn(f.app), "build 2");
});

nativeTest("a failed move to the Trash keeps the previous bundle and the signed staging copy", () => {
  const f = bundleFixture();
  assert.equal(f.build("build 1").status, 0);
  const second = f.build("build 2", { trashStatus: "1" });
  assert.notEqual(second.status, 0);
  assert.equal(f.exeIn(f.app), "build 1");
  assert.deepEqual(f.trashedBuilds(), []);
  assert.deepEqual(f.backupBuilds(), ["build 1"]);
  const staged = second.stderr.match(/the new signed bundle is at (.+)$/m)?.[1];
  assert.ok(staged, second.stderr);
  assert.equal(f.exeIn(staged), "build 2");
});

nativeTest("backups made in the same second do not overwrite each other", () => {
  const f = bundleFixture();
  for (const label of ["build 1", "build 2", "build 3"]) {
    const result = f.build(label);
    assert.equal(result.status, 0, result.stderr);
  }
  assert.deepEqual(f.backupBuilds().sort(), ["build 1", "build 2"]);
});
