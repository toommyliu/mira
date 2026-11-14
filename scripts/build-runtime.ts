import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs, promisify } from "node:util";

import { build } from "esbuild";

const root = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(root, "packages", "mira");
const effect = join(runtime, "node_modules", "effect");

const miraModules = [
  { specifier: "mira", source: "mira", out: "mira/index" },
  { specifier: "mira/autorelogin", source: "autorelogin", out: "mira/autorelogin" },
  { specifier: "mira/http", source: "http", out: "mira/http" },
  { specifier: "mira/schema", source: "schema", out: "mira/schema" },
] as const;

async function effectModules(): Promise<string[]> {
  const files = await readdir(join(effect, "dist"));
  return files
    .filter((file) => file.endsWith(".js") && file !== "index.js")
    .map((file) => file.slice(0, -".js".length))
    .sort();
}

function table(name: string, files: readonly string[], base: string): string {
  const lines = files.map(
    (path) =>
      `    (${JSON.stringify(relative(base, path))}, include_str!(${JSON.stringify(path)})),`,
  );
  return `pub(super) static ${name}: &[(&str, &str)] = &[\n${lines.join("\n")}\n];\n`;
}

async function filesUnder(dir: string): Promise<string[]> {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .sort();
}

async function declarationClosure(dir: string, entries: readonly string[]): Promise<string[]> {
  const seen = new Set<string>();
  const queue = [...entries];
  while (queue.length > 0) {
    const file = normalize(queue.pop()!);
    if (seen.has(file)) continue;
    seen.add(file);
    const text = await readFile(join(dir, file), "utf8");
    for (const [, path] of text.matchAll(/(?:from |import\()"(\.\.?\/[^"]+)"/g)) {
      queue.push(join(dirname(file), path!.replace(/\.(ts|js)$/, ".d.ts")));
    }
  }
  return [...seen].sort();
}

async function emitMiraDeclarations(): Promise<string> {
  const outdir = await mkdtemp(join(tmpdir(), "mira-declarations-"));
  const tsgo = join(runtime, "node_modules", ".bin", "effect-tsgo");
  const args = ["tsrs", "-p", "tsconfig.json", "--noEmit", "false", "--declaration"];
  args.push("--emitDeclarationOnly", "--rootDir", "src", "--outDir", outdir);
  await promisify(execFile)(tsgo, args, { cwd: runtime });
  return outdir;
}

function editorConfig(effectNames: readonly string[]): string {
  const paths: Record<string, string[]> = {};
  for (const module of miraModules) {
    paths[module.specifier] = [`./mira/${module.source}.d.ts`];
  }
  paths["effect"] = ["./effect/index.d.ts"];
  for (const name of effectNames) paths[`effect/${name}`] = [`./effect/${name}.d.ts`];
  const config = {
    compilerOptions: {
      target: "es2023",
      lib: ["es2023"],
      module: "esnext",
      moduleResolution: "bundler",
      strict: true,
      verbatimModuleSyntax: true,
      allowImportingTsExtensions: true,
      allowJs: true,
      noEmit: true,
      skipLibCheck: true,
      typeRoots: ["./ambient"],
      types: ["mira-globals"],
      paths,
    },
  };
  return `${JSON.stringify(config, null, 2)}\n`;
}

async function buildTypes(outdir: string, effectNames: readonly string[]): Promise<void> {
  await rm(outdir, { recursive: true, force: true });
  const declarations = await emitMiraDeclarations();
  try {
    const sources = [
      ...(
        await declarationClosure(
          declarations,
          miraModules.map((module) => `${module.source}.d.ts`),
        )
      ).map((file) => ({ from: join(declarations, file), to: join("mira", file) })),
      ...(
        await declarationClosure(join(effect, "dist"), [
          "index.d.ts",
          ...effectNames.map((name) => `${name}.d.ts`),
        ])
      ).map((file) => ({ from: join(effect, "dist", file), to: join("effect", file) })),
      { from: join(effect, "LICENSE"), to: join("effect", "LICENSE") },
      { from: join(runtime, "globals.d.ts"), to: join("ambient", "mira-globals", "index.d.ts") },
    ];
    for (const { from, to } of sources) {
      await mkdir(dirname(join(outdir, to)), { recursive: true });
      await cp(from, join(outdir, to));
    }
  } finally {
    await rm(declarations, { recursive: true, force: true });
  }
  await writeFile(join(outdir, "tsconfig.json"), editorConfig(effectNames));
}

async function version(dir: string, files: readonly string[]): Promise<string> {
  const hash = createHash("sha256");
  for (const file of files) {
    hash.update(relative(dir, file)).update("\0");
    hash.update(await readFile(file)).update("\0");
  }
  return hash.digest("hex");
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      outdir: { type: "string" },
      table: { type: "string" },
      types: { type: "string" },
      "types-table": { type: "string" },
    },
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
      { in: "./src/prelude.ts", out: "mira/prelude" },
      { in: "./src/bootstrap.ts", out: "mira/bootstrap" },
      ...miraModules.map((module) => ({ in: `./src/${module.source}.ts`, out: module.out })),
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
  if (values.table !== undefined) {
    const scripts = (await filesUnder(outdir)).filter((file) => file.endsWith(".js"));
    await writeFile(resolve(values.table), table("ASSETS", scripts, outdir));
  }
  if (values.types !== undefined) {
    const types = resolve(values.types);
    await buildTypes(types, modules);
    if (values["types-table"] !== undefined) {
      const files = await filesUnder(types);
      await writeFile(
        resolve(values["types-table"]),
        `pub(super) const VERSION: &str = ${JSON.stringify(await version(types, files))};\n${table("FILES", files, types)}`,
      );
    }
  }
}

if (import.meta.main) await main();
