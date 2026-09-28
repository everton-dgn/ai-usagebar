import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("../scripts/sign-macos-tray.sh", import.meta.url));
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
