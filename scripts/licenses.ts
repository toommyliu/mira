import { execFileSync, spawnSync } from "node:child_process";
import { copyFile, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const effect = join(root, "packages", "mira", "node_modules", "effect");
const RUST_NOTICES = "RUST_STANDARD_LIBRARY_LICENSES.html";

const BUNDLED = [
  { license: "MIT License", user: "effect", path: join(effect, "LICENSE") },
  {
    license: "ISC License",
    user: "Lucide icons",
    path: join(root, "crates", "ui", "assets", "icons", "lucide", "LICENSE"),
  },
  {
    license: "SIL Open Font License 1.1",
    user: "Noto Sans, embedded by Ruffle",
    path: join(root, "scripts", "licenses", "noto-sans.txt"),
  },
] as const;

export type Notice = {
  readonly license: string;
  readonly users: readonly string[];
  readonly text: string;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function dependencyName(crate: unknown): string | null {
  if (!isRecord(crate) || typeof crate.name !== "string" || typeof crate.version !== "string") {
    throw new Error("cargo-about listed a crate without a name and version");
  }
  return crate.source === null ? null : `${crate.name} ${crate.version}`;
}

export function crateNotices(report: unknown): Notice[] {
  if (!isRecord(report) || !Array.isArray(report.licenses) || !Array.isArray(report.crates)) {
    throw new Error("cargo-about's report has no licenses");
  }
  const notices = report.licenses.flatMap((license: unknown) => {
    if (
      !isRecord(license) ||
      typeof license.name !== "string" ||
      typeof license.text !== "string" ||
      !Array.isArray(license.used_by)
    ) {
      throw new Error("cargo-about listed a malformed license");
    }
    const users = license.used_by
      .map((user: unknown) => dependencyName(isRecord(user) ? user.crate : null))
      .filter((name) => name !== null);
    return users.length ? [{ license: license.name, users, text: license.text }] : [];
  });
  const listed = new Set(notices.flatMap((notice) => notice.users));
  const missing = report.crates
    .map((crate: unknown) => dependencyName(isRecord(crate) ? crate.package : null))
    .filter((name) => name !== null && !listed.has(name));
  if (missing.length) throw new Error(`cargo-about found no license for ${missing.join(", ")}`);
  return notices;
}

export function vendoredNotices(file: string, source: string): Notice[] {
  return [...source.matchAll(/\/\*[\s\S]*?\*\//g)]
    .map(([comment]) => comment)
    .filter((comment) => /copyright/i.test(comment))
    .map((comment) => ({
      license: "Notice vendored in effect",
      users: [file],
      text: comment
        .slice(2, -2)
        .split("\n")
        .map((line) => line.replace(/^\s*\*+ ?/, ""))
        .filter((line) => !/^\/+$/.test(line.trim()))
        .join("\n")
        .trim(),
    }));
}

export function render(notices: readonly Notice[]): string {
  const sections = notices.map(
    ({ license, users, text }) =>
      `## ${license}\n\n${users.map((user) => `- ${user}\n`).join("")}\n\`\`\`\n${text.replaceAll("\r\n", "\n").trimEnd()}\n\`\`\`\n`,
  );
  return [
    "# Third-party licenses\n\nMira includes the following third-party software.\n",
    ...sections,
  ].join("\n");
}

function cargoAbout(): unknown {
  const result = spawnSync(
    "cargo",
    ["about", "generate", "--locked", "--format", "json", "-c", "scripts/licenses/about.toml"],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    throw new Error(
      `cargo about failed. Install it with \`cargo install cargo-about --locked --features cli\` if it's missing.\n${result.stderr.trim()}`,
    );
  }
  const stale = result.stderr.split("\n").find((line) => line.includes("failed to validate"));
  if (stale) throw new Error(`Update scripts/licenses/about.toml: ${stale}`);
  return JSON.parse(result.stdout);
}

async function effectNotices(): Promise<Notice[]> {
  const dist = join(effect, "dist");
  const files = (await readdir(dist, { recursive: true }))
    .filter((file) => file.endsWith(".js"))
    .sort();
  const notices = await Promise.all(
    files.map(async (file) =>
      vendoredNotices(`effect/dist/${file}`, await readFile(join(dist, file), "utf8")),
    ),
  );
  return notices.flat();
}

async function main(): Promise<void> {
  const [output, ...extra] = process.argv.slice(2);
  if (output === undefined || extra.length) {
    throw new Error("Usage: node scripts/licenses.ts <output>");
  }
  const sysroot = execFileSync("rustc", ["--print", "sysroot"], { cwd: root, encoding: "utf8" });
  await copyFile(
    join(sysroot.trim(), "share", "doc", "rust", "COPYRIGHT-library.html"),
    join(dirname(output), RUST_NOTICES),
  );
  const bundled = await Promise.all(
    BUNDLED.map(async ({ license, user, path }) => ({
      license,
      users: [user],
      text: await readFile(path, "utf8"),
    })),
  );
  const std = {
    license: "Rust standard library",
    users: ["std"],
    text: `The Rust standard library's notices are in ${RUST_NOTICES}, next to this file.`,
  };
  await writeFile(
    output,
    render([...crateNotices(cargoAbout()), std, ...bundled, ...(await effectNotices())]),
  );
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
