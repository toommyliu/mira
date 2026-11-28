import { appendFile, readFile, writeFile } from "node:fs/promises";
import {
  changelog,
  parseChanges,
  manifestVersion,
  published,
  publishAssets,
  releaseCommit,
  run,
  withVersion,
  type Version,
} from "./release.ts";

const nightlyTag = /^v\d+\.\d+\.\d+-nightly\.\d{8}\.[1-9]\d*$/;
const stableTag = /^v\d+\.\d+\.\d+$/;
const sixHours = 6 * 60 * 60 * 1000;

function nightlyVersion(base: Version, createdAt: string, runNumber: string): string {
  const date = new Date(createdAt).toISOString().slice(0, 10).replaceAll("-", "");
  if (!/^[1-9]\d*$/.test(runNumber)) throw new Error("Missing workflow run number");
  return `${base.major}.${base.minor}.${base.patch + 1}-nightly.${date}.${runNumber}`;
}

type Release = {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  target_commitish: string;
};

type History = { nightly?: Release; stable?: Release; draft?: Release };

const publishedAt = (release: Release | undefined) =>
  release?.published_at ? Date.parse(release.published_at) : -Infinity;

const newest = (current: Release | undefined, release: Release) =>
  current && publishedAt(current) >= publishedAt(release) ? current : release;

export function history(releases: readonly Release[], sha: string): History {
  const found: History = {};
  for (const release of releases) {
    if (release.prerelease && nightlyTag.test(release.tag_name)) {
      if (!release.draft) found.nightly = newest(found.nightly, release);
      else if (release.target_commitish === sha) found.draft ??= release;
    } else if (!release.draft && !release.prerelease && stableTag.test(release.tag_name)) {
      found.stable = newest(found.stable, release);
    }
  }
  return found;
}

export function planNightly(
  base: Version,
  createdAt: string,
  runNumber: string,
  { nightly, stable, draft }: History,
  now: number,
) {
  if (now - publishedAt(nightly) < sixHours && publishedAt(stable) <= publishedAt(nightly)) return;
  return {
    version: draft?.tag_name.slice(1) ?? nightlyVersion(base, createdAt, runNumber),
    previous: (nightly ?? stable)?.tag_name,
  };
}

function releases(slug: string, sha: string): History {
  const seen: Release[] = [];
  for (let page = 1; ; page++) {
    const batch: Release[] = JSON.parse(
      run("gh", [
        "api",
        `repos/${slug}/releases?page=${page}`,
        "--jq",
        "map({tag_name,draft,prerelease,published_at,target_commitish})",
      ]),
    );
    seen.push(...batch);
    const found = history(seen, sha);
    if ((found.nightly && found.stable) || batch.length < 30) return found;
  }
}

function testedCommit(sha: string): string {
  const subject = run("git", ["log", "-1", "--format=%s", sha]).trim();
  if (!releaseCommit.test(subject)) return sha;
  const files = run("git", ["diff", "--name-only", `${sha}^`, sha])
    .trim()
    .split("\n");
  if (!files.every((file) => file === "Cargo.toml" || file === "Cargo.lock")) return sha;
  return run("git", ["rev-parse", `${sha}^`]).trim();
}

async function prepare(slug: string, sha: string) {
  const branch = process.env["GITHUB_REF_NAME"]!;
  const tip = run("git", ["ls-remote", "origin", `refs/heads/${branch}`]).split("\t")[0];
  if (tip !== sha) return;
  const conclusion = run("gh", [
    "run",
    "list",
    "--repo",
    slug,
    "--workflow",
    "ci.yml",
    "--event",
    "push",
    "--commit",
    testedCommit(sha),
    "--branch",
    branch,
    "--limit",
    "1",
    "--json",
    "conclusion",
    "--jq",
    ".[0].conclusion // empty",
  ]).trim();
  if (conclusion !== "success") return;

  const manifest = await readFile("Cargo.toml", "utf8");
  const base = manifestVersion(manifest);
  const created = run("gh", [
    "api",
    `repos/${slug}/actions/runs/${process.env["GITHUB_RUN_ID"]}`,
    "--jq",
    ".created_at",
  ]).trim();
  const plan = planNightly(
    base,
    created,
    process.env["GITHUB_RUN_NUMBER"]!,
    releases(slug, sha),
    Date.now(),
  );
  if (!plan) return;
  const { version, previous } = plan;
  // A rerun can pick up a release published after this checkout fetched its tags.
  if (previous) {
    run("git", ["fetch", "--quiet", "origin", `refs/tags/${previous}:refs/tags/${previous}`]);
    const ancestor = run("git", ["merge-base", previous, sha]).trim();
    if (ancestor !== run("git", ["rev-parse", `${previous}^{commit}`]).trim() || ancestor === sha)
      return;
  }
  const range = previous ? `${previous}..${sha}` : sha;
  const changes = parseChanges(
    run("git", ["log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", range]),
  );
  if (!changes.length) return;
  const body = `Nightly build of ${branch} at ${sha.slice(0, 8)}.\n\n${changelog(changes)}`;
  return { version, manifest, body };
}

async function main() {
  const [mode] = process.argv.slice(2);
  const slug = process.env["GITHUB_REPOSITORY"];
  const sha = process.env["GITHUB_SHA"];
  if (
    !slug ||
    !sha ||
    process.env["GITHUB_EVENT_NAME"] !== "schedule" ||
    run("git", ["rev-parse", "HEAD"]).trim() !== sha
  ) {
    throw new Error("Nightlies run on the scheduled workflow commit");
  }
  const plan = await prepare(slug, sha);
  if (mode === "check") {
    await appendFile(process.env["GITHUB_OUTPUT"]!, `ready=${Boolean(plan)}\n`);
    return;
  }
  if (mode !== "publish") throw new Error("Expected check or publish");
  if (!plan) return;
  const tag = `v${plan.version}`;
  const ref = run("git", ["ls-remote", "origin", `refs/tags/${tag}`]).trim();
  if (ref && ref.split("\t")[0] !== sha) throw new Error(`${tag} points to another commit`);
  const existing = published(slug, tag);
  if (existing && (!existing.draft || existing.target !== sha || !existing.prerelease)) {
    throw new Error(`${tag} already exists outside this nightly build`);
  }
  await writeFile("Cargo.toml", withVersion(plan.manifest, plan.version));
  run("cargo", ["update", "--workspace", "--offline", "--quiet"]);
  run("scripts/package-app", [], true);
  await publishAssets(slug, sha, plan.version, plan.body, existing, true);
}

if (import.meta.main) await main();
