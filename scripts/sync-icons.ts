import { mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const ICONS = [
  "arrow-down",
  "arrow-left",
  "arrow-right",
  "arrow-up",
  "check",
  "chevron-down",
  "chevron-right",
  "chevron-up",
  "minus",
  "plus",
  "x",
] as const;

const root = fileURLToPath(new URL("../", import.meta.url));
const lucide = dirname(createRequire(import.meta.url).resolve("lucide-static/package.json"));
const output = join(root, "crates", "ui", "assets", "icons", "lucide");

function sources(): ReadonlyMap<string, string> {
  return new Map([
    ["LICENSE", join(lucide, "LICENSE")],
    ...ICONS.map((icon) => [`${icon}.svg`, join(lucide, "icons", `${icon}.svg`)] as const),
  ]);
}

async function main(): Promise<void> {
  const { values } = parseArgs({ options: { check: { type: "boolean", default: false } } });
  const wanted = sources();
  const present = await readdir(output).catch(() => []);
  const stale: string[] = [];
  for (const name of present) {
    if (wanted.has(name)) continue;
    stale.push(name);
    if (!values.check) await rm(join(output, name));
  }
  if (!values.check) await mkdir(output, { recursive: true });
  for (const [name, source] of wanted) {
    const content = await readFile(source, "utf8");
    const existing = await readFile(join(output, name), "utf8").catch(() => null);
    if (existing === content) continue;
    stale.push(name);
    if (!values.check) await writeFile(join(output, name), content);
  }
  if (values.check && stale.length) {
    const paths = stale.map((name) => relative(root, join(output, name)));
    console.error(`Out of date, run pnpm icons:\n${paths.join("\n")}`);
    process.exitCode = 1;
    return;
  }
  console.log(`${ICONS.length} icons`);
}

if (import.meta.main) await main();
