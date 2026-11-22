import assert from "node:assert/strict";
import { test } from "node:test";

import {
  checkNotes,
  isNewer,
  manifestRepository,
  manifestVersion,
  parseChanges,
  parseVersion,
  plan,
  releaseBody,
  remoteFor,
  withVersion,
} from "./release.ts";

const manifest = `[package]
name = "mira"
version = "0.1.2"
edition = "2024"
repository = "https://github.com/toommyliu/mira"

[workspace.dependencies]
version = "9.9.9"
`;

test("the package version and repository come from the [package] section", () => {
  assert.deepEqual(manifestVersion(manifest), { major: 0, minor: 1, patch: 2 });
  assert.deepEqual(manifestRepository(manifest), { owner: "toommyliu", name: "mira" });
  const bumped = withVersion(manifest, parseVersion("0.2.0"));
  assert.deepEqual(manifestVersion(bumped), { major: 0, minor: 2, patch: 0 });
  assert.match(bumped, /version = "9\.9\.9"/);
});

test("a release must be a newer plain version", () => {
  const current = parseVersion("0.1.2");
  assert.ok(isNewer(parseVersion("0.1.3"), current));
  assert.ok(isNewer(parseVersion("1.0.0"), current));
  assert.ok(!isNewer(parseVersion("0.1.2"), current));
  assert.ok(!isNewer(parseVersion("0.0.9"), current));
  assert.throws(() => parseVersion("v0.1.3"));
  assert.throws(() => parseVersion("0.2.0-beta.1"));
});

test("the push goes to the remote for the manifest's repository", () => {
  const remotes = [
    "origin\thttps://github.com/toommyliu/mira.git (fetch)",
    "fork\tgit@github.com:someone/mira.git (push)",
  ].join("\n");
  assert.equal(remoteFor(remotes, { owner: "someone", name: "mira" }), "fork");
  assert.equal(remoteFor(remotes, { owner: "toommyliu", name: "mira" }), "origin");
  assert.throws(() => remoteFor(remotes, { owner: "someone", name: "else" }));
});

test("the body is the condensed notes, then the full changelog in a disclosure", () => {
  const changes = parseChanges(
    [
      "a1b2c3d4e5\x1ffeat: show updates in the sidebar\x1fLonger body\x1e",
      "f6e5d4c3b2\x1frelease: Mira 0.1.2\x1f\x1e",
      "0011223344\x1ffix: stop the spinner outgrowing its icon\x1f\x1e",
    ].join("\n"),
  );
  assert.deepEqual(
    changes.map((change) => change.subject),
    ["feat: show updates in the sidebar", "fix: stop the spinner outgrowing its icon"],
  );
  assert.equal(
    releaseBody("- **Updates in the sidebar.** Click to download.\n", changes),
    [
      "- **Updates in the sidebar.** Click to download.",
      "",
      "<details>",
      "<summary>Full changelog (2 changes)</summary>",
      "",
      "- feat: show updates in the sidebar (a1b2c3d4)",
      "- fix: stop the spinner outgrowing its icon (00112233)",
      "",
      "</details>",
      "",
    ].join("\n"),
  );
});

test("notes are a few short highlights and leave the changelog to the script", () => {
  const note = "- **Sidebar updates.** Click to download.";
  assert.throws(() => checkNotes(""));
  assert.throws(() => checkNotes("Updates in the sidebar."));
  assert.throws(() => checkNotes("- Fixes a crash."));
  assert.throws(() => checkNotes(`${note}\n<details>`));
  assert.throws(() => checkNotes(Array(6).fill(note).join("\n")));
  const lead = "- **Sidebar updates.** ";
  assert.throws(() => checkNotes(lead + "a".repeat(120 - lead.length)));
  assert.equal(checkNotes(lead + "a".repeat(119 - lead.length)).length, 119);
  assert.equal(checkNotes(`${note}\n\n${note}\n`), `${note}\n${note}`);
});

test("a publish bumps a newer version, resumes its own release commit, and refuses the rest", () => {
  const v = parseVersion;
  assert.equal(plan(v("0.1.8"), v("0.1.7"), "fix: something", "v0.1.7"), "bump");
  assert.equal(plan(v("0.1.8"), v("0.1.8"), "release: Mira 0.1.8", "v0.1.7"), "resume");
  assert.equal(plan(v("0.1.8"), v("0.1.8"), "release: Mira 0.1.8", "v0.1.8"), "resume");
  assert.equal(plan(v("0.1.0"), v("0.0.9"), "fix: something", null), "bump");
  assert.throws(() => plan(v("0.1.8"), v("0.1.8"), "fix: something", "v0.1.7"));
  assert.throws(() => plan(v("0.1.7"), v("0.1.7"), "release: Mira 0.1.7", "v0.1.9"));
  assert.throws(() => plan(v("0.1.8"), v("0.1.7"), "fix: something", "v0.2.0"));
  assert.throws(() => plan(v("0.1.7"), v("0.1.6"), "fix: something", "v0.1.7"));
});
