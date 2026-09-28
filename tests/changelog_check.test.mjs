// Contract of scripts/check-changelog-immutable.sh against fixture repositories.
//
// Hermetic: every repository is a fresh mkdtemp sandbox with its own copy of
// the script, whose pinned archive commit, cutoff tag and reconciliation digest
// are rewritten to the fixture's. Global and system git config are disabled, so no user identity,
// hook or signing setting applies.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

// Sandboxes are kept, never removed, like the other script contract tests.
const script = fileURLToPath(new URL("../scripts/check-changelog-immutable.sh", import.meta.url));
const HISTORY = "docs/history/CHANGELOG.original.md";
const RECONCILIATIONS = "docs/history/changelog-reconciliations.tsv";
const INSERTED = "docs/history/reconciliations/v1.0.0.inserted";
const UNSHIPPED = "docs/history/changelog-unshipped-tags.tsv";
const UNSHIPPED_HEADER = "# tag\ttag_object\tkind\tsuccessor\tpublished_sha256\n";
const HEADER = "# tag\ttag_object\ttag_commit\tamend_commit\tpublished_sha256\tarchived_sha256\tinsert_after\n";

const env = {
  PATH: process.env.PATH,
  HOME: tmpdir(),
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_CONFIG_NOSYSTEM: "1",
  GIT_AUTHOR_NAME: "Fixture",
  GIT_AUTHOR_EMAIL: "fixture@example.invalid",
  GIT_COMMITTER_NAME: "Fixture",
  GIT_COMMITTER_EMAIL: "fixture@example.invalid",
  LC_ALL: "C",
};

const v100 = "## [1.0.0] — 2020-01-01\n\n### Added\n\n- First release.\n\n";
const v110 = "## [1.1.0] — 2020-02-01\n\n### Fixed\n\n- A published fix.\n\n";
// Written in the archived changelog, but not tagged before the move (1.23.0).
const v120 = "## [1.2.0] — 2020-03-01\n\n### Added\n\n- Archived before its tag.\n\n";
const v200 = "## [2.0.0] — 2021-01-01\n\n### Alterado\n\n- Primeira versão após o arquivamento.\n\n";
const links = (...versions) => versions.map(v => `[${v}]: https://example.invalid/v${v}\n`).join("");

// The untagged 0.9.0 section closes every tagged range, as older releases do
// in the real file; the last section otherwise runs on into the link list.
const v090 = "## [0.9.0] — 2019-12-01\n\n- Before tagging.\n\n";
// Each section, as the script extracts it, ends with the next heading.
const next = "## [0.9.0] — 2019-12-01\n";

// Added to the released [1.0.0] section after v1.0.0, before the archive move,
// as the real 0.6.0 section was.
const inserted = "- Added after the tag.\n";
const v100amended = v100.replace("- First release.\n", `- First release.\n${inserted}`);

// Tagged, then folded into 1.1.0 by the next commit, as v1.20.0 was.
const v101 = "## [1.0.1] — 2020-01-15\n\n- Never shipped.\n\n";

const sha256 = text => createHash("sha256").update(text).digest("hex");

function changelog(...sections) {
  return `# Changelog\n\n## [Unreleased]\n\n${sections.join("")}${v090}`;
}

/// Tags v1.0.0 and v1.1.0 ship the original changelog, which also holds an
/// untagged 1.2.0 section, the archive cutoff; the next commit moves it to the
/// archive and starts the active one, then v2.0.0 ships from that.
/// `tagged: false` builds the same history without any tag; `untagged` lists
/// tags to leave out. `amend` inserts a line into the released [1.0.0] section
/// between v1.0.0 and the archived commit, and `reconcile` pins it in the
/// reconciliation file (by default whenever `amend` is set). `early` starts
/// with a v0.1.0 cut before CHANGELOG.md existed, and `superseded` tags a
/// v1.0.1 that v1.1.0 replaces; both are pinned as unshipped.
function fixture({ tagged = true, untagged = [], amend = false, reconcile = amend, early = false, superseded = false } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "aiub-changelog-"));
  const git = (...args) => execFileSync("git", args, { cwd: root, env, encoding: "utf8" }).trim();
  const write = (file, body) => {
    mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    writeFileSync(path.join(root, file), body);
  };
  const commit = (message, tag) => {
    git("add", "-A");
    git("commit", "--quiet", "--no-verify", "--no-gpg-sign", "-m", message);
    if (tag && tagged && !untagged.includes(tag)) git("tag", "-a", "-m", tag, tag);
  };

  git("init", "--quiet", "--initial-branch=main");
  const unshipped = [];
  if (early) {
    write("Cargo.toml", 'version = "0.1.0"\n');
    commit("v0.1.0", "v0.1.0");
    unshipped.push(["v0.1.0", git("rev-parse", "refs/tags/v0.1.0"), "no-changelog", "-", "-"]);
  }
  write("Cargo.toml", 'version = "1.0.0"\n');
  write("CHANGELOG.md", changelog(v100) + links("1.0.0"));
  commit("v1.0.0", "v1.0.0");
  const v100tagged = tagged && !untagged.includes("v1.0.0");
  const tag = { object: v100tagged ? git("rev-parse", "refs/tags/v1.0.0") : "", commit: git("rev-parse", "HEAD") };

  if (amend) {
    write("CHANGELOG.md", changelog(v100amended) + links("1.0.0"));
    commit("amend the released 1.0.0 section");
  }
  const amendCommit = git("rev-parse", "HEAD");

  if (superseded) {
    write("CHANGELOG.md", changelog(v101, amend ? v100amended : v100) + links("1.0.1", "1.0.0"));
    commit("v1.0.1", "v1.0.1");
    const published = sha256(v101 + "## [1.0.0] — 2020-01-01\n");
    unshipped.push(["v1.0.1", git("rev-parse", "refs/tags/v1.0.1"), "superseded", "v1.1.0", published]);
  }

  write("Cargo.toml", 'version = "1.1.0"\n');
  const released = amend ? v100amended : v100;
  const original = changelog(v120, v110, released) + links("1.2.0", "1.1.0", "1.0.0");
  write("CHANGELOG.md", original);
  commit("v1.1.0", "v1.1.0");
  const source = git("rev-parse", "HEAD");

  // The row the real file holds for v0.6.0, rebuilt for this history.
  const row = (fields = {}) => {
    const r = {
      tag: "v1.0.0",
      object: tag.object,
      commit: tag.commit,
      amend: amendCommit,
      published: sha256(v100 + next),
      archived: sha256(v100amended + next),
      after: "5",
      ...fields,
    };
    return [r.tag, r.object, r.commit, r.amend, r.published, r.archived, r.after].join("\t") + "\n";
  };

  write(HISTORY, original);
  write(RECONCILIATIONS, HEADER + (reconcile ? row() : ""));
  write(UNSHIPPED, UNSHIPPED_HEADER + unshipped.map(r => r.join("\t") + "\n").join(""));
  if (reconcile) write(INSERTED, inserted);
  write("CHANGELOG.md", changelog(v200) + links("2.0.0"));
  write("Cargo.toml", 'version = "2.0.0"\n');
  commit("move the published changelog", "v2.0.0");

  const copy = path.join(root, "scripts", "check-changelog-immutable.sh");
  mkdirSync(path.dirname(copy), { recursive: true });
  const body = readFileSync(script, "utf8");
  const read = file => readFileSync(path.join(root, file), "utf8");
  // Pins the row files as they are now, as a reviewed script edit would.
  const repin = () => {
    for (const pin of ["HISTORY_SOURCE", "HISTORY_LAST_TAG", "RECONCILIATIONS_SHA256", "UNSHIPPED_SHA256"]) {
      assert.match(body, new RegExp(`^${pin}=`, "m"), `the script must pin ${pin}`);
    }
    const pinned = body
      .replace(/^HISTORY_SOURCE=.*$/m, `HISTORY_SOURCE=${source}`)
      .replace(/^HISTORY_LAST_TAG=.*$/m, "HISTORY_LAST_TAG=v1.2.0")
      .replace(/^RECONCILIATIONS_SHA256=.*$/m, `RECONCILIATIONS_SHA256=${sha256(read(RECONCILIATIONS))}`)
      .replace(/^UNSHIPPED_SHA256=.*$/m, `UNSHIPPED_SHA256=${sha256(read(UNSHIPPED))}`);
    writeFileSync(copy, pinned, { mode: 0o755 });
  };
  repin();

  const run = (extra = {}) => spawnSync("/bin/bash", [copy], { cwd: root, env: { ...env, ...extra }, encoding: "utf8" });
  return { root, read, write, run, git, repin, row, unshipped, source, amendCommit, tag };
}

test("an intact archive and active changelog pass", () => {
  const fx = fixture();
  const result = fx.run();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /are intact/);
});

test("archived tags are not required in the active changelog", () => {
  const fx = fixture();
  assert.doesNotMatch(fx.read("CHANGELOG.md"), /\[1\.1\.0\]/);
  assert.equal(fx.run().status, 0);
});

test("a missing archive fails closed", () => {
  const fx = fixture();
  renameSync(path.join(fx.root, HISTORY), path.join(fx.root, `${HISTORY}.moved`));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /CHANGELOG\.original\.md is missing/);
});

test("an edited archived section fails against its tag", () => {
  const fx = fixture();
  fx.write(HISTORY, fx.read(HISTORY).replace("- First release.", "- First release, reworded."));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /no longer matches CHANGELOG\.md at/);
  assert.match(result.stdout, /published \[1\.0\.0\] section in docs\/history\/CHANGELOG\.original\.md no longer matches v1\.0\.0/);
});

test("any byte changed outside a section still breaks the frozen archive", () => {
  const fx = fixture();
  fx.write(HISTORY, fx.read(HISTORY) + "\n");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /no longer matches CHANGELOG\.md at/);
});

test("a section dropped from the archive is reported as missing", () => {
  const fx = fixture();
  fx.write(HISTORY, fx.read(HISTORY).replace(v110, ""));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /\[1\.1\.0\] section is MISSING from docs\/history\/CHANGELOG\.original\.md/);
});

test("a tag after the cutoff is checked against the active changelog", () => {
  const fx = fixture();
  fx.write("CHANGELOG.md", fx.read("CHANGELOG.md").replace("- Primeira versão", "- Versão reescrita"));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /published \[2\.0\.0\] section in CHANGELOG\.md no longer matches v2\.0\.0/);
});

test("a new section merged away from the active changelog fails", () => {
  const fx = fixture();
  fx.write("CHANGELOG.md", fx.read("CHANGELOG.md").replace(v200, ""));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /\[2\.0\.0\] section is MISSING from CHANGELOG\.md/);
});

test("a Cargo version older than the newest tag fails", () => {
  const fx = fixture();
  fx.write("Cargo.toml", 'version = "1.9.0"\n');
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /Cargo\.toml says 1\.9\.0, older than 2\.0\.0/);
});

test("the archive cutoff is a version floor even before its tag exists", () => {
  const fx = fixture({ untagged: ["v2.0.0"] });
  assert.equal(fx.git("tag", "--list", "v2*"), "");
  fx.write("Cargo.toml", 'version = "1.2.0"\n');
  assert.equal(fx.run().status, 0, "Cargo at the archived version passes");
  fx.write("Cargo.toml", 'version = "1.1.5"\n');
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /Cargo\.toml says 1\.1\.5, older than 1\.2\.0/);
});

test("an archived version tagged after the move is checked against the archive", () => {
  const fx = fixture();
  assert.equal(fx.run().status, 0, "untagged archived section passes");
  // Cut from the post-move commit, whose CHANGELOG.md no longer has 1.2.0.
  fx.git("tag", "v1.2.0");
  const intact = fx.run();
  assert.equal(intact.status, 0, intact.stdout);
  fx.write(HISTORY, fx.read(HISTORY).replace("- Archived before its tag.", "- Reworded."));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /published \[1\.2\.0\] section in docs\/history\/CHANGELOG\.original\.md no longer matches v1\.2\.0/);
});

test("removing a section together with its link does not retire the release", () => {
  const fx = fixture();
  fx.write("CHANGELOG.md", fx.read("CHANGELOG.md").replace(v200, "").replace(links("2.0.0"), ""));
  assert.doesNotMatch(fx.read("CHANGELOG.md"), /2\.0\.0/);
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /\[2\.0\.0\] section is MISSING from CHANGELOG\.md/);
});

test("a new tag without a section of its own fails", () => {
  const fx = fixture();
  fx.write("Cargo.toml", 'version = "2.1.0"\n');
  fx.git("tag", "v2.1.0");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /v2\.1\.0 has no \[2\.1\.0\] section of its own/);
});

test("only pinned unshipped tags are skipped, after verification", () => {
  const fx = fixture({ early: true, superseded: true });
  const skipped = fx.run();
  assert.equal(skipped.status, 0, skipped.stdout);
  assert.match(skipped.stdout, /skip: v0\.1\.0 has no published section, verified against/);
  assert.match(skipped.stdout, /skip: v1\.0\.1 has no published section, verified against/);
  // The real v1.20.0 is pinned by its object; a tag of that name elsewhere is not.
  fx.git("tag", "v1.20.0");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /v1\.20\.0 has no \[1\.20\.0\] section of its own/);
});

test("a Cargo version bumped past the newest tag passes", () => {
  const fx = fixture();
  fx.write("Cargo.toml", 'version = "2.1.0"\n');
  assert.equal(fx.run().status, 0);
});

test("a repository without release tags is an error, not a pass", () => {
  const fx = fixture({ tagged: false });
  assert.equal(fx.git("tag", "--list"), "");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /no v\* tags found/);
});

test("every tag is compared by default; a limit is explicit and validated", () => {
  const fx = fixture();
  // v1.1.0 moved back onto the v1.0.0 commit no longer ships a [1.1.0] section.
  fx.git("tag", "-f", "v1.1.0", "v1.0.0^{commit}");
  const limited = fx.run({ CHANGELOG_TAGS_TO_CHECK: "1" });
  assert.equal(limited.status, 0, limited.stdout);
  assert.match(limited.stdout, /note: comparing only the 1 newest tags/);
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /v1\.1\.0 has no \[1\.1\.0\] section of its own/);
  for (const bad of ["", "0", "08", "-1", "all", "3 "]) {
    const invalid = fx.run({ CHANGELOG_TAGS_TO_CHECK: bad });
    assert.equal(invalid.status, 1, `limit ${JSON.stringify(bad)}`);
    assert.match(invalid.stdout, /CHANGELOG_TAGS_TO_CHECK must be a positive integer/);
  }
});

test("a section amended after its tag fails without a reconciliation row", () => {
  const fx = fixture({ amend: true, reconcile: false });
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /published \[1\.0\.0\] section in docs\/history\/CHANGELOG\.original\.md no longer matches v1\.0\.0/);
  assert.match(result.stdout, /> - Added after the tag\./);
});

test("a pinned reconciliation accepts exactly the recorded amendment", () => {
  const fx = fixture({ amend: true });
  const result = fx.run();
  assert.equal(result.status, 0, result.stdout);
  assert.match(result.stdout, /reconciled: the archived \[1\.0\.0\] section differs from v1\.0\.0/);
  // The row is verified even when its tag is outside the compared window.
  const limited = fx.run({ CHANGELOG_TAGS_TO_CHECK: "1" });
  assert.equal(limited.status, 0, limited.stdout);
  assert.doesNotMatch(limited.stdout, /reconciled:/);
});

test("an edit inside a reconciled section still fails", () => {
  const fx = fixture({ amend: true });
  fx.write(HISTORY, fx.read(HISTORY).replace("- Added after the tag.", "- Added after the tag, reworded."));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /no longer matches CHANGELOG\.md at/);
  assert.match(result.stdout, /archived \[1\.0\.0\] section no longer has digest/);
  assert.match(result.stdout, /published \[1\.0\.0\] section in docs\/history\/CHANGELOG\.original\.md no longer matches v1\.0\.0/);
});

test("a reconciled tag that is moved or recreated fails", () => {
  const fx = fixture({ amend: true });
  // Recreated on the amending commit, the tag would match the archive outright.
  fx.git("tag", "-f", "-a", "-m", "v1.0.0", "v1.0.0", fx.amendCommit);
  const moved = fx.run();
  assert.equal(moved.status, 1);
  assert.match(moved.stdout, /v1\.0\.0 in .*changelog-reconciliations\.tsv: the tag no longer points at object/);
  // Same commit, new tag object.
  fx.git("tag", "-f", "-a", "-m", "again", "v1.0.0", fx.tag.commit);
  const recreated = fx.run({ CHANGELOG_TAGS_TO_CHECK: "1" });
  assert.equal(recreated.status, 1);
  assert.match(recreated.stdout, /the tag no longer points at object/);
});

test("a reconciliation file changed without its pin fails", () => {
  const fx = fixture({ amend: true });
  fx.write(RECONCILIATIONS, fx.read(RECONCILIATIONS) + fx.row({ tag: "v1.1.0" }));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /changelog-reconciliations\.tsv does not match its pinned digest/);
  // Nothing in an unpinned file is trusted, not even the valid row.
  assert.match(result.stdout, /published \[1\.0\.0\] section in docs\/history\/CHANGELOG\.original\.md no longer matches v1\.0\.0/);
});

test("a missing reconciliation file fails closed", () => {
  const fx = fixture({ amend: true });
  renameSync(path.join(fx.root, RECONCILIATIONS), path.join(fx.root, `${RECONCILIATIONS}.moved`));
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /changelog-reconciliations\.tsv is missing/);
});

test("a re-pinned row still has to match git and the archive", () => {
  const cases = [
    [{ published: sha256("other") }, /the tag's \[1\.0\.0\] section no longer has digest/],
    [{ archived: sha256("other") }, /archived \[1\.0\.0\] section no longer has digest/],
    [{ commit: "0".repeat(40) }, /the tag no longer resolves to commit/],
    [{ after: "4" }, /does not give the archived section/],
    [{ after: "99" }, /insert_after is past the section/],
    [{ after: "five" }, /insert_after is not a line number/],
    [{ tag: "v1.0.0; x" }, /not a release tag name/],
  ];
  for (const [fields, message] of cases) {
    const fx = fixture({ amend: true });
    fx.write(RECONCILIATIONS, HEADER + fx.row(fields));
    fx.repin();
    const result = fx.run();
    assert.equal(result.status, 1, JSON.stringify(fields));
    assert.match(result.stdout, message, JSON.stringify(fields));
    assert.doesNotMatch(result.stdout, /reconciled:/);
  }
});

test("a re-pinned row with the wrong amending commit fails", () => {
  const fx = fixture({ amend: true });
  const side = fx.git("commit-tree", `${fx.amendCommit}^{tree}`, "-p", fx.tag.commit, "-m", "side");
  const cases = [
    [fx.tag.commit, /amending commit .* does not follow the tag/],
    [side, /amending commit .* is not in the history of/],
    [fx.source, /commit .* does not turn the tag's section into the archived one/],
    ["f".repeat(40), /amending commit .* does not follow the tag/],
  ];
  for (const [amend, message] of cases) {
    fx.write(RECONCILIATIONS, HEADER + fx.row({ amend }));
    fx.repin();
    const result = fx.run();
    assert.equal(result.status, 1, amend);
    assert.match(result.stdout, message, amend);
  }
});

test("a re-pinned row outside the archive, stale or duplicated fails", () => {
  const stale = fixture();
  stale.write(RECONCILIATIONS, HEADER + stale.row({ amend: stale.source }));
  stale.repin();
  const staleResult = stale.run();
  assert.equal(staleResult.status, 1);
  assert.match(staleResult.stdout, /stale row: the archive already matches the tag/);

  const active = fixture({ amend: true });
  active.write(RECONCILIATIONS, HEADER + active.row() + active.row({ tag: "v2.0.0" }));
  active.repin();
  const activeResult = active.run();
  assert.equal(activeResult.status, 1);
  assert.match(activeResult.stdout, /v2\.0\.0 .*: only sections in docs\/history\/CHANGELOG\.original\.md can be reconciled/);

  const duplicate = fixture({ amend: true });
  duplicate.write(RECONCILIATIONS, HEADER + duplicate.row() + duplicate.row());
  duplicate.repin();
  const duplicateResult = duplicate.run();
  assert.equal(duplicateResult.status, 1);
  assert.match(duplicateResult.stdout, /duplicate row/);

  const extra = fixture({ amend: true });
  extra.write(RECONCILIATIONS, HEADER + extra.row().replace("\n", "\textra\n"));
  extra.repin();
  const extraResult = extra.run();
  assert.equal(extraResult.status, 1);
  assert.match(extraResult.stdout, /expected 7 tab-separated fields/);
});

test("an altered insertion file fails", () => {
  const fx = fixture({ amend: true });
  fx.write(INSERTED, "- Something else.\n");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /inserting reconciliations\/v1\.0\.0\.inserted after line 5 does not give the archived section/);
  renameSync(path.join(fx.root, INSERTED), path.join(fx.root, `${INSERTED}.moved`));
  const missing = fx.run();
  assert.equal(missing.status, 1);
  assert.match(missing.stdout, /v1\.0\.0\.inserted is missing/);
});

test("an unshipped-tags file changed without its pin fails", () => {
  const fx = fixture({ superseded: true });
  fx.write(UNSHIPPED, fx.read(UNSHIPPED) + ["v2.0.0", fx.git("rev-parse", "refs/tags/v2.0.0"), "no-changelog", "-", "-"].join("\t") + "\n");
  const result = fx.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /changelog-unshipped-tags\.tsv does not match its pinned digest/);
  // Nothing in an unpinned file is trusted, not even the valid row.
  assert.match(result.stdout, /\[1\.0\.1\] section is MISSING from docs\/history\/CHANGELOG\.original\.md/);
  renameSync(path.join(fx.root, UNSHIPPED), path.join(fx.root, `${UNSHIPPED}.moved`));
  const missing = fx.run();
  assert.equal(missing.status, 1);
  assert.match(missing.stdout, /changelog-unshipped-tags\.tsv is missing/);
});

test("a moved unshipped tag fails", () => {
  const fx = fixture({ early: true, superseded: true });
  fx.git("tag", "-f", "-a", "-m", "moved", "v1.0.1", "v1.0.0^{commit}");
  fx.git("tag", "-f", "-a", "-m", "again", "v0.1.0", "v0.1.0^{commit}");
  const result = fx.run({ CHANGELOG_TAGS_TO_CHECK: "1" });
  assert.equal(result.status, 1);
  assert.match(result.stdout, /v1\.0\.1 in .*: the tag no longer points at object/);
  assert.match(result.stdout, /v0\.1\.0 in .*: the tag no longer points at object/);
});

test("a re-pinned unshipped row still has to match git and the changelogs", () => {
  const cases = [
    [fx => ["v1.0.1", fx.unshipped[0][1], "superseded", "v2.0.0", fx.unshipped[0][4]], /successor v2\.0\.0 is not the tag's direct child/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "superseded", "v1.0.0", fx.unshipped[0][4]], /successor v1\.0\.0 is not a later release tag/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "superseded", "v1.1.0", sha256("other")], /the tag's \[1\.0\.1\] section no longer has digest/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "abandoned", "v1.1.0", fx.unshipped[0][4]], /unknown kind 'abandoned'/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "no-changelog", "-", "-"], /the tag has a changelog after all/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "no-changelog", "v1.1.0", "-"], /no-changelog rows take no successor or digest/],
    [fx => ["v1.0.0", fx.tag.object, "superseded", "v1.0.1", sha256(v100 + next)], /a \[1\.0\.0\] section is published after all/],
    [fx => ["v1.0.1", fx.unshipped[0][1], "superseded", "v1.1.0"], /expected 5 tab-separated fields/],
  ];
  for (const [build, message] of cases) {
    const fx = fixture({ superseded: true });
    fx.write(UNSHIPPED, UNSHIPPED_HEADER + build(fx).join("\t") + "\n");
    fx.repin();
    const result = fx.run();
    assert.equal(result.status, 1, message.source);
    assert.match(result.stdout, message);
  }
  const duplicate = fixture({ superseded: true });
  duplicate.write(UNSHIPPED, duplicate.read(UNSHIPPED) + duplicate.unshipped[0].join("\t") + "\n");
  duplicate.repin();
  const result = duplicate.run();
  assert.equal(result.status, 1);
  assert.match(result.stdout, /v1\.0\.1 in .*: duplicate row/);
});
