import { cp, readdir, rm, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { build } from "esbuild";

const root = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(root, "packages", "lyra");
const effect = join(runtime, "node_modules", "effect");

async function effectModules(): Promise<string[]> {
  const files = await readdir(join(effect, "dist"));
  return files
    .filter((file) => file.endsWith(".js") && file !== "index.js")
    .map((file) => file.slice(0, -".js".length))
    .sort();
}

async function writeTable(outdir: string, table: string): Promise<void> {
  const entries = await readdir(outdir, { recursive: true, withFileTypes: true });
  const lines = entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".js"))
    .map((entry) => join(entry.parentPath, entry.name))
    .map(
      (path) =>
        `    (${JSON.stringify(relative(outdir, path))}, include_str!(${JSON.stringify(path)})),`,
    );
  await writeFile(
    table,
    `pub(super) static ASSETS: &[(&str, &str)] = &[\n${lines.join("\n")}\n];\n`,
  );
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: { outdir: { type: "string" }, table: { type: "string" } },
  });
  if (values.outdir === undefined) throw new Error("--outdir is required");
  const outdir = resolve(values.outdir);
  const modules = await effectModules();
  await rm(outdir, { recursive: true, force: true });
  await build({
    absWorkingDir: runtime,
    entryPoints: [
      { in: "effect", out: "effect/index" },
      ...modules.map((module) => ({ in: `effect/${module}`, out: `effect/${module}` })),
      { in: "./src/prelude.ts", out: "lyra/prelude" },
      { in: "./src/bootstrap.ts", out: "lyra/bootstrap" },
      { in: "./src/lyra.ts", out: "lyra/index" },
      { in: "./src/autorelogin.ts", out: "lyra/autorelogin" },
      { in: "./src/autozone.ts", out: "lyra/autozone" },
    ],
    outdir,
    bundle: true,
    splitting: true,
    format: "esm",
    platform: "neutral",
    target: "es2023",
    chunkNames: "chunks/[hash]",
    minify: true,
    legalComments: "none",
    logLevel: "warning",
  });
  await cp(join(effect, "LICENSE"), join(outdir, "effect", "LICENSE"));
  if (values.table !== undefined) await writeTable(outdir, resolve(values.table));
}

if (import.meta.main) await main();
