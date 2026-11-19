import { execFileSync, spawnSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

export type Param = { readonly name: string; readonly type: string };

export type Method = {
  readonly name: string;
  readonly params: readonly Param[];
  readonly returns: string;
};

type Rev = string & { readonly __brand: "Rev" };

type Package = {
  readonly name: string;
  readonly source: string | null;
  readonly manifestPath: string;
};

type Checkout = { readonly gpui: string; readonly gpuiApple: string; readonly zed: string };

const zedUrl = "https://github.com/zed-industries/zed";
const root = fileURLToPath(new URL("../", import.meta.url));
const manifestPath = join(root, "Cargo.toml");
const platformDir = join(root, "crates", "platform");
const forwardingPath = join(platformDir, "src", "platform.rs");
const copiedSources = ["text_system.rs", "open_type.rs"];
const fontKitPin =
  /(font-kit = \{ git = "https:\/\/github\.com\/zed-industries\/font-kit", rev = ")(\w+)(")/;

const outsideGpui = new Map([
  ["Arc", "std::sync::Arc"],
  ["OsString", "std::ffi::OsString"],
  ["Path", "std::path::Path"],
  ["PathBuf", "std::path::PathBuf"],
  ["Rc", "std::rc::Rc"],
  ["Result", "anyhow::Result"],
  ["SmallVec", "smallvec::SmallVec"],
]);
const prelude = new Set(["Box", "Fn", "FnMut", "FnOnce", "Option", "Self", "String", "Vec"]);
const builtInCrates = new Set(["alloc", "core", "crate", "self", "std", "super"]);

export function parsePlatformTrait(source: string): Method[] {
  const lines = source.split("\n");
  const start = lines.findIndex((line) => line.startsWith("pub trait Platform: 'static"));
  if (start === -1) throw new Error("gpui no longer defines `pub trait Platform: 'static`");
  const methods: Method[] = [];
  let cfg: string | undefined;
  for (let i = start + 1; ; i++) {
    const line = lines[i];
    if (line === undefined) throw new Error("The Platform trait never closes");
    if (line === "}") return methods;
    if (/^ {4}#\[cfg/.test(line)) {
      cfg = line.trim();
      continue;
    }
    if (!/^ {4}[^\s/]/.test(line) || !/\bfn \w+/.test(line)) continue;
    if (!line.startsWith("    fn ")) throw new Error(`Can't forward \`${line.trim()}\``);
    let signature = line.trim();
    while (!/(;|\{\}?)$/.test(signature)) {
      const next = lines[++i];
      if (next === undefined) throw new Error(`Unfinished signature \`${signature}\``);
      signature += ` ${next.trim()}`;
    }
    const method = parseSignature(signature.replace(/\s*(;|\{\}?)$/, ""));
    if (cfg === undefined || targetsMacos(cfg, method.name)) methods.push(method);
    cfg = undefined;
  }
}

function parseSignature(signature: string): Method {
  const head = /^fn (\w+)\(/.exec(signature);
  const name = head?.[1];
  if (head === null || name === undefined) {
    throw new Error(`Can't read Platform method \`${signature}\``);
  }
  const open = head[0].length - 1;
  const close = closingParen(signature, open);
  const [receiver, ...params] = splitTopLevel(signature.slice(open + 1, close));
  if (receiver !== "&self") throw new Error(`Platform::${name} doesn't take &self`);
  return {
    name,
    params: params.map((param) => parseParam(name, param)),
    returns: signature.slice(close + 1).trim(),
  };
}

function parseParam(method: string, param: string): Param {
  const colon = param.indexOf(":");
  const name = param.slice(0, colon).trim().replace(/^_/, "");
  if (colon === -1 || !/^\w+$/.test(name)) {
    throw new Error(`Can't forward Platform::${method} parameter \`${param}\``);
  }
  return { name, type: param.slice(colon + 1).trim() };
}

function closingParen(text: string, open: number): number {
  let depth = 0;
  for (const [i, char] of Array.from(text).entries()) {
    if (i < open) continue;
    if (char === "(") depth++;
    if (char === ")" && --depth === 0) return i;
  }
  throw new Error(`Unbalanced parentheses in \`${text}\``);
}

function splitTopLevel(list: string): string[] {
  const parts = [""];
  let depth = 0;
  for (const [i, char] of Array.from(list).entries()) {
    if ("<([".includes(char)) depth++;
    if (")]".includes(char) || (char === ">" && list[i - 1] !== "-")) depth--;
    if (char === "," && depth === 0) parts.push("");
    else parts[parts.length - 1] += char;
  }
  return parts.map((part) => part.trim()).filter(Boolean);
}

function targetsMacos(cfg: string, method: string): boolean {
  const systems = [...cfg.matchAll(/target_os = "(\w+)"/g)].map((match) => match[1]);
  if (systems.length === 0 || /\b(not|all)\(|feature/.test(cfg)) {
    throw new Error(`Platform::${method} has ${cfg}, which this script can't evaluate`);
  }
  return systems.includes("macos");
}

export function renderPlatform(methods: readonly Method[]): string {
  if (!methods.some((method) => method.name === "text_system")) {
    throw new Error("Platform has no text_system method to replace");
  }
  const types = methods.flatMap((method) => [method.returns, ...method.params.map((p) => p.type)]);
  const paths = ["crate::MiraPlatform"];
  const fromGpui = ["Platform"];
  for (const name of new Set(types.flatMap((type) => type.match(/(?<!::)\b[A-Z]\w*/g) ?? []))) {
    const path = outsideGpui.get(name);
    if (path !== undefined) paths.push(path);
    else if (!prelude.has(name)) fromGpui.push(name);
  }
  if (types.some((type) => /\boneshot::/.test(type))) paths.push("futures::channel::oneshot");
  const forwards = methods.map(({ name, params, returns }) => {
    const declared = params.map((param) => `, ${param.name}: ${param.type}`).join("");
    const body =
      name === "text_system"
        ? "self.text_system.clone()"
        : `self.mac.${name}(${params.map((param) => param.name).join(", ")})`;
    return `fn ${name}(&self${declared}) ${returns} { ${body} }`;
  });
  return [
    "// AUTO-GENERATED FILE. DO NOT EDIT.",
    ...paths.map((path) => `use ${path};`),
    `use gpui::{${fromGpui.join(", ")}};`,
    "impl Platform for MiraPlatform {",
    ...forwards,
    "}",
    "",
  ].join("\n");
}

function isRev(text: string): text is Rev {
  return /^[0-9a-f]{40}$/.test(text);
}

const short = (rev: Rev) => rev.slice(0, 8);

async function pinnedRev(): Promise<Rev> {
  const manifest = await readFile(manifestPath, "utf8");
  const pins = /git = "https:\/\/github\.com\/zed-industries\/zed", rev = "(\w+)"/g;
  const revs = new Set([...manifest.matchAll(pins)].flatMap((match) => match[1] ?? []));
  const [rev, ...others] = revs;
  if (rev === undefined || others.length > 0 || !isRev(rev)) {
    throw new Error(
      `Cargo.toml should pin one Zed revision, found ${[...revs].join(", ") || "none"}`,
    );
  }
  return rev;
}

function resolveRev(ref: string): Rev {
  if (isRev(ref)) return ref;
  const names = ref.startsWith("refs/") ? [ref] : [`refs/heads/${ref}`, `refs/tags/${ref}`];
  const listing = execFileSync("git", ["ls-remote", zedUrl, ...names], { encoding: "utf8" });
  const refs = new Map(
    listing.split("\n").flatMap((line): [string, string][] => {
      const [rev, name] = line.split("\t");
      return rev !== undefined && name !== undefined ? [[name, rev]] : [];
    }),
  );
  for (const name of names.flatMap((name) => [`${name}^{}`, name])) {
    const rev = refs.get(name);
    if (rev !== undefined && isRev(rev)) return rev;
  }
  throw new Error(`${ref} isn't a commit, branch or tag in ${zedUrl}`);
}

function parsePackages(json: string): Package[] {
  const metadata: unknown = JSON.parse(json);
  if (typeof metadata !== "object" || metadata === null || !("packages" in metadata)) {
    throw new Error("cargo metadata printed no package list");
  }
  const { packages } = metadata;
  if (!Array.isArray(packages)) throw new Error("cargo metadata printed no package list");
  return packages.map((entry: unknown) => {
    if (typeof entry !== "object" || entry === null)
      throw new Error("cargo metadata package isn't an object");
    const name = "name" in entry ? entry.name : undefined;
    const source = "source" in entry ? entry.source : undefined;
    const manifest = "manifest_path" in entry ? entry.manifest_path : undefined;
    if (typeof name !== "string" || typeof manifest !== "string") {
      throw new Error("cargo metadata package has no name or manifest path");
    }
    if (source !== null && typeof source !== "string") {
      throw new Error(`cargo metadata package ${name} has an unreadable source`);
    }
    return { name, source, manifestPath: manifest };
  });
}

function zedCheckout(rev: Rev, cargoFlags: readonly string[]): Checkout {
  const json = execFileSync("cargo", ["metadata", "--format-version", "1", ...cargoFlags], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  });
  const packages = parsePackages(json);
  const crateDir = (name: string) => {
    const found = packages.find((pkg) => pkg.name === name && pkg.source?.endsWith(`#${rev}`));
    if (found === undefined) throw new Error(`cargo metadata has no ${name} at ${short(rev)}`);
    return dirname(found.manifestPath);
  };
  const gpuiApple = crateDir("gpui_apple");
  return { gpui: crateDir("gpui"), gpuiApple, zed: join(gpuiApple, "..", "..") };
}

function rustfmt(source: string): string {
  return execFileSync("rustfmt", ["--edition", "2024", "--emit", "stdout"], {
    input: source,
    encoding: "utf8",
  });
}

async function platformMethods(checkout: Checkout): Promise<Method[]> {
  return parsePlatformTrait(await readFile(join(checkout.gpui, "src", "platform.rs"), "utf8"));
}

export function referencedCrates(methods: readonly Method[]): string[] {
  const roots = methods
    .flatMap((method) => [method.returns, ...method.params.map((param) => param.type)])
    .flatMap((type) => [...type.matchAll(/(?<![\w:])([a-z_]\w*)::/g)].flatMap((m) => m[1] ?? []))
    .map((root) => (root === "oneshot" ? "futures" : root));
  return [...new Set(roots)].filter((root) => !builtInCrates.has(root));
}

function section(toml: string, header: string): string {
  const start = toml.indexOf(`\n${header}\n`);
  if (start === -1) return "";
  const end = toml.indexOf("\n[", start + header.length + 1);
  return toml.slice(start, end === -1 ? undefined : end);
}

function versions(dependencies: string): Map<string, string> {
  const entry = /^([\w-]+) = (?:"([^"]+)"|\{[^\n}]*?\bversion = "([^"]+)")/gm;
  return new Map(
    [...dependencies.matchAll(entry)].flatMap((match): [string, string][] => {
      const name = match[1];
      const version = match[2] ?? match[3];
      return name !== undefined && version !== undefined ? [[name, version]] : [];
    }),
  );
}

function addDependency(manifest: string, line: string): string {
  const lines = manifest.split("\n");
  const header = lines.indexOf("[dependencies]");
  if (header === -1) throw new Error("crates/platform/Cargo.toml has no [dependencies]");
  let at = header + 1;
  while (at < lines.length) {
    const current = lines[at] ?? "";
    if (current === "" || current.startsWith("[") || current > line) break;
    at++;
  }
  lines.splice(at, 0, line);
  return lines.join("\n");
}

async function syncPlatformDependencies(
  checkout: Checkout,
  methods: readonly Method[],
): Promise<string[]> {
  const zedManifest = await readFile(join(checkout.zed, "Cargo.toml"), "utf8");
  const fontKitRev = fontKitPin.exec(zedManifest)?.[2];
  if (fontKitRev === undefined)
    throw new Error("Zed's Cargo.toml no longer pins font-kit by git rev");
  const path = join(platformDir, "Cargo.toml");
  let ours = await readFile(path, "utf8");
  if (!fontKitPin.test(ours))
    throw new Error("crates/platform/Cargo.toml no longer pins font-kit by git rev");
  ours = ours.replace(fontKitPin, `$1${fontKitRev}$3`);
  const zedDependencies = section(zedManifest, "[workspace.dependencies]");
  const declared = new Set(
    [...section(ours, "[dependencies]").matchAll(/^([\w-]+)(?:\.workspace)?\s*=/gm)].flatMap(
      (match) => match[1]?.replaceAll("-", "_") ?? [],
    ),
  );
  const unresolved: string[] = [];
  for (const name of referencedCrates(methods)) {
    if (declared.has(name)) continue;
    const spec = new RegExp(`^${name.replaceAll("_", "[-_]")} = (.+)$`, "m").exec(zedDependencies);
    if (spec === null || /\b(path|git) =|\{[^}]*$/.test(spec[0])) unresolved.push(name);
    else ours = addDependency(ours, spec[0]);
  }
  await writeFile(path, ours);
  const wanted = versions(zedDependencies);
  for (const [name, version] of versions(section(ours, "[dependencies]"))) {
    const zed = wanted.get(name);
    if (zed !== undefined && zed !== version) {
      console.warn(`${name}: Zed uses ${zed}, crates/platform/Cargo.toml has ${version}`);
    }
  }
  return unresolved;
}

async function update(target: Rev): Promise<void> {
  const pinned = await pinnedRev();
  if (pinned === target) {
    console.log(`Already at ${short(target)}`);
    return;
  }
  const touched = [
    manifestPath,
    join(root, "Cargo.lock"),
    join(platformDir, "Cargo.toml"),
    forwardingPath,
    ...copiedSources.map((file) => join(platformDir, "src", file)),
  ];
  const saved = await Promise.all(
    touched.map(async (path): Promise<[string, string]> => [path, await readFile(path, "utf8")]),
  );
  let left: Leftovers;
  try {
    left = await bump(pinned, target);
  } catch (error) {
    await Promise.all(saved.map(([path, text]) => writeFile(path, text)));
    throw error;
  }
  console.log(`Zed ${short(pinned)} -> ${short(target)}`);
  if (left.conflicts.length > 0) {
    console.error(`Resolve the conflict markers in:\n${left.conflicts.join("\n")}`);
    process.exitCode = 1;
  }
  if (left.dependencies.length > 0) {
    console.error(
      `Add these crates to crates/platform/Cargo.toml by hand:\n${left.dependencies.join("\n")}`,
    );
    process.exitCode = 1;
  }
}

type Leftovers = {
  readonly conflicts: readonly string[];
  readonly dependencies: readonly string[];
};

async function bump(pinned: Rev, target: Rev): Promise<Leftovers> {
  const before = zedCheckout(pinned, ["--locked"]);
  const manifest = await readFile(manifestPath, "utf8");
  await writeFile(manifestPath, manifest.replaceAll(pinned, target));
  const after = zedCheckout(target, []);
  const conflicts: string[] = [];
  for (const file of copiedSources) {
    const ours = join(platformDir, "src", file);
    const text = await readFile(ours, "utf8");
    await writeFile(ours, text.replace(`at ${short(pinned)}`, `at ${short(target)}`));
    const merge = spawnSync(
      "git",
      [
        "merge-file",
        "-L",
        "mira",
        "-L",
        short(pinned),
        "-L",
        short(target),
        ours,
        join(before.gpuiApple, "src", file),
        join(after.gpuiApple, "src", file),
      ],
      { encoding: "utf8" },
    );
    if (merge.status === null || merge.status > 127) {
      throw new Error(`git merge-file failed on ${file}: ${merge.stderr}`);
    }
    if (merge.status > 0) conflicts.push(relative(root, ours));
  }
  const methods = await platformMethods(after);
  const dependencies = await syncPlatformDependencies(after, methods);
  await writeFile(forwardingPath, rustfmt(renderPlatform(methods)));
  zedCheckout(target, []);
  return { conflicts, dependencies };
}

async function main(): Promise<void> {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: { check: { type: "boolean", default: false } },
  });
  const [ref, ...extra] = positionals;
  if (extra.length > 0 || (values.check && ref !== undefined)) {
    throw new Error("Usage: node scripts/update-gpui.ts [<zed commit, branch or tag> | --check]");
  }
  if (ref !== undefined) return update(resolveRev(ref));
  const methods = await platformMethods(zedCheckout(await pinnedRev(), ["--locked"]));
  const expected = rustfmt(renderPlatform(methods));
  const existing = await readFile(forwardingPath, "utf8").catch(() => null);
  if (existing === expected) return;
  if (values.check) {
    console.error(`Out of date, run pnpm update:gpui:\n${relative(root, forwardingPath)}`);
    process.exitCode = 1;
    return;
  }
  await writeFile(forwardingPath, expected);
}

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
