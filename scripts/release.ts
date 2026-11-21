import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

export type Version = { readonly major: number; readonly minor: number; readonly patch: number };

export type Change = { readonly sha: string; readonly subject: string; readonly body: string };

export type Repository = { readonly owner: string; readonly name: string };

const root = fileURLToPath(new URL("../", import.meta.url));
const manifestPath = join(root, "Cargo.toml");
const usage = `Usage:
  pnpm release changes
  pnpm release publish <version> --notes <file> [--dry-run]`;

export function parseVersion(text: string): Version {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(text.trim());
  if (!match) throw new Error(`${text} isn't a version like 1.2.3`);
  const [major, minor, patch] = match.slice(1).map(Number) as [number, number, number];
  return { major, minor, patch };
}

export function formatVersion(version: Version): string {
  return `${version.major}.${version.minor}.${version.patch}`;
}

export function isNewer(next: Version, current: Version): boolean {
  return (
    (next.major - current.major || next.minor - current.minor || next.patch - current.patch) > 0
  );
}

function packageSection(manifest: string): { readonly start: number; readonly end: number } {
  const start = manifest.indexOf("[package]");
  if (start === -1) throw new Error("Cargo.toml has no [package] section");
  const next = manifest.indexOf("\n[", start + 1);
  return { start, end: next === -1 ? manifest.length : next };
}

function packageField(manifest: string, field: string): string {
  const { start, end } = packageSection(manifest);
  const match = new RegExp(`^${field} = "([^"]+)"$`, "m").exec(manifest.slice(start, end));
  if (!match?.[1]) throw new Error(`Cargo.toml's [package] has no ${field}`);
  return match[1];
}

export function manifestVersion(manifest: string): Version {
  return parseVersion(packageField(manifest, "version"));
}

export function manifestRepository(manifest: string): Repository {
  const url = packageField(manifest, "repository");
  const match = /^https:\/\/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?\/?$/.exec(url);
  if (!match?.[1] || !match[2]) throw new Error(`${url} isn't a GitHub repository`);
  return { owner: match[1], name: match[2] };
}

export function withVersion(manifest: string, version: Version): string {
  const { start, end } = packageSection(manifest);
  const section = manifest.slice(start, end);
  const bumped = section.replace(/^version = "[^"]+"$/m, `version = "${formatVersion(version)}"`);
  return manifest.slice(0, start) + bumped + manifest.slice(end);
}

export function remoteFor(remotes: string, repository: Repository): string {
  const wanted = `github.com/${repository.owner}/${repository.name}`.toLowerCase();
  for (const line of remotes.split("\n")) {
    const [name, url] = line.split(/\s+/);
    if (!name || !url) continue;
    const normalized = url
      .toLowerCase()
      .replace(/^git@github\.com:/, "github.com/")
      .replace(/^https:\/\//, "")
      .replace(/\.git$/, "");
    if (normalized === wanted) return name;
  }
  throw new Error(`No git remote points at ${wanted}`);
}

export function checkNotes(notes: string): string {
  const lines = notes
    .trim()
    .split("\n")
    .map((line) => line.trimEnd());
  if (lines.length === 0 || lines[0] === "") throw new Error("The notes file is empty");
  for (const line of lines) {
    if (line === "") continue;
    if (!line.startsWith("- ")) throw new Error(`Each note is a "- " bullet: ${line}`);
    if (line.includes("<details")) throw new Error("Leave the full changelog to the script");
  }
  return lines.join("\n");
}

export function releaseBody(notes: string, changes: readonly Change[]): string {
  const log = changes.map((change) => `- ${change.subject} (${change.sha.slice(0, 8)})`);
  return [
    checkNotes(notes),
    "",
    "<details>",
    "<summary>Full changelog</summary>",
    "",
    ...(log.length > 0 ? log : ["- No commits since the last release."]),
    "",
    "</details>",
    "",
  ].join("\n");
}

export type Plan = "bump" | "resume";

export function plan(
  version: Version,
  current: Version,
  head: string,
  latest: string | null,
): Plan {
  const name = formatVersion(version);
  const resumable = name === formatVersion(current) && head === `release: Mira ${name}`;
  if (latest === `v${name}`) {
    if (resumable) return "resume";
    throw new Error(`${name} is already published`);
  }
  if (latest !== null && !isNewer(version, parseVersion(latest.slice(1)))) {
    throw new Error(`${name} isn't newer than the published ${latest}`);
  }
  if (isNewer(version, current)) return "bump";
  if (resumable) return "resume";
  throw new Error(`${name} isn't newer than ${formatVersion(current)}`);
}

export function parseChanges(log: string): Change[] {
  return log
    .split("\x1e")
    .map((record) => record.trim())
    .filter((record) => record !== "")
    .map((record) => {
      const [sha = "", subject = "", body = ""] = record.split("\x1f");
      return { sha, subject, body: body.trim() };
    })
    .filter((change) => !change.subject.startsWith("release:"));
}

function run(command: string, args: readonly string[], inherit = false): string {
  return execFileSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: inherit ? "inherit" : ["ignore", "pipe", "inherit"],
  });
}

function latestTag(repository: Repository): string | null {
  try {
    const tag = execFileSync(
      "gh",
      ["api", `repos/${repository.owner}/${repository.name}/releases/latest`, "--jq", ".tag_name"],
      { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    ).trim();
    return tag || null;
  } catch (error) {
    const stderr = String((error as { stderr?: unknown }).stderr ?? "");
    if (stderr.includes("HTTP 404")) return null;
    throw new Error(`Couldn't read the latest release: ${stderr.trim() || String(error)}`);
  }
}

type Published = { readonly draft: boolean; readonly assets: readonly string[] };

function published(slug: string, name: string): Published | null {
  try {
    const view = execFileSync(
      "gh",
      ["release", "view", name, "--repo", slug, "--json", "isDraft,assets"],
      { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
    const release = JSON.parse(view) as {
      isDraft: boolean;
      assets: readonly { name: string; state: string }[];
    };
    const uploaded = release.assets.filter((asset) => asset.state === "uploaded");
    return { draft: release.isDraft, assets: uploaded.map((asset) => asset.name) };
  } catch (error) {
    const stderr = String((error as { stderr?: unknown }).stderr ?? "");
    if (stderr.includes("release not found")) return null;
    throw new Error(`Couldn't read release ${name}: ${stderr.trim() || String(error)}`);
  }
}

function changesSince(remote: string, tag: string | null): Change[] {
  const range = ["HEAD"];
  if (tag) {
    run("git", ["fetch", "--quiet", "--no-tags", remote, `refs/tags/${tag}`]);
    range.unshift("^FETCH_HEAD");
  }
  return parseChanges(run("git", ["log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", ...range]));
}

async function context() {
  const manifest = await readFile(manifestPath, "utf8");
  const repository = manifestRepository(manifest);
  const remote = remoteFor(run("git", ["remote", "-v"]), repository);
  const tag = latestTag(repository);
  return { manifest, repository, remote, tag, changes: changesSince(remote, tag) };
}

async function changes(): Promise<void> {
  const { manifest, tag, changes } = await context();
  console.log(`Mira ${formatVersion(manifestVersion(manifest))}, last release ${tag ?? "none"}`);
  for (const change of changes) {
    console.log(`\n${change.sha.slice(0, 8)} ${change.subject}`);
    if (change.body) console.log(change.body.replace(/^/gm, "  "));
  }
}

async function publish(versionText: string, notesPath: string, dryRun: boolean): Promise<void> {
  const { manifest, repository, remote, tag, changes } = await context();
  const version = parseVersion(versionText);
  const name = `v${formatVersion(version)}`;
  const head = run("git", ["log", "-1", "--format=%s"]).trim();
  const step = plan(version, manifestVersion(manifest), head, tag);
  const slug = `${repository.owner}/${repository.name}`;
  const existing = published(slug, name);
  if (existing && step === "bump") {
    throw new Error(
      `${name} already has a release. Resume from its release commit or delete it first`,
    );
  }
  const body = releaseBody(await readFile(notesPath, "utf8"), changes);
  const branch = run("gh", ["api", `repos/${slug}`, "--jq", ".default_branch"]).trim();
  const action = step === "resume" ? "Resume" : "Release";
  console.log(`${action} ${name} to ${slug} (${remote}/${branch}), previous ${tag ?? "none"}\n`);
  if (!existing) console.log(body);
  if (dryRun) return;
  if (run("git", ["status", "--porcelain", "--untracked-files=no"]).trim()) {
    throw new Error("Commit or stash your changes before releasing");
  }
  if (step === "bump") {
    await writeFile(manifestPath, withVersion(manifest, version));
    run("cargo", ["update", "--workspace", "--offline", "--quiet"]);
    run("git", [
      "commit",
      "--quiet",
      "-m",
      `release: Mira ${formatVersion(version)}`,
      "--",
      "Cargo.toml",
      "Cargo.lock",
    ]);
  }
  const sha = run("git", ["rev-parse", "HEAD"]).trim();
  run("scripts/package-app", [], true);
  run("git", ["push", "--quiet", remote, `HEAD:refs/heads/${branch}`], true);
  const out = join(root, "target", "releases", formatVersion(version));
  const assets = ["Mira.dmg", "Mira.zip"];
  if (existing) {
    const missing = assets.filter((asset) => !existing.assets.includes(asset));
    if (missing.length > 0) {
      const files = missing.map((asset) => join(out, asset));
      run("gh", ["release", "upload", name, "--repo", slug, "--clobber", ...files], true);
    }
    if (existing.draft) {
      run("gh", ["release", "edit", name, "--repo", slug, "--draft=false", "--latest"], true);
    }
    return;
  }
  const scratch = await mkdtemp(join(tmpdir(), "mira-release-"));
  try {
    const notes = join(scratch, "notes.md");
    await writeFile(notes, body);
    run(
      "gh",
      [
        "release",
        "create",
        name,
        "--repo",
        slug,
        "--target",
        sha,
        "--title",
        `Mira ${formatVersion(version)}`,
        "--notes-file",
        notes,
        "--latest",
        ...assets.map((asset) => join(out, asset)),
      ],
      true,
    );
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

async function main(): Promise<void> {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: { notes: { type: "string" }, "dry-run": { type: "boolean", default: false } },
  });
  const [command, version, ...extra] = positionals;
  if (command === "changes" && version === undefined) return changes();
  if (command === "publish" && version !== undefined && extra.length === 0 && values.notes) {
    return publish(version, values.notes, values["dry-run"]);
  }
  throw new Error(usage);
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
