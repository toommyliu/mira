import { execFileSync } from "node:child_process";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

export type Source = { readonly path: string; readonly text: string };

type Fallback =
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "number"; readonly value: number }
  | { readonly kind: "boolean"; readonly value: boolean }
  | { readonly kind: "null" };

type Parameter =
  | {
      readonly kind: "required";
      readonly name: string;
      readonly type: string;
      readonly rustType?: string;
    }
  | {
      readonly kind: "optional";
      readonly name: string;
      readonly type: string;
      readonly fallback: Fallback;
    }
  | { readonly kind: "rest"; readonly name: string };

type Entry = {
  readonly name: string;
  readonly className: string;
  readonly classFqn: string;
  readonly methodName: string;
  readonly parameters: readonly Parameter[];
  readonly returnType: string;
  readonly location: string;
};

export type Bridge = { readonly exports: readonly Entry[]; readonly events: readonly Entry[] };

type Metadata = { readonly name: string; readonly arg: string | null };

type Method = {
  readonly className: string;
  readonly classFqn: string;
  readonly namespace: string | null;
  readonly name: string;
  readonly parameters: readonly Parameter[];
  readonly returnType: string;
  readonly isStatic: boolean;
  readonly metadata: readonly Metadata[];
  readonly location: string;
};

const RUST_KEYWORDS = new Set([
  "as",
  "async",
  "await",
  "box",
  "break",
  "const",
  "continue",
  "crate",
  "dyn",
  "else",
  "enum",
  "extern",
  "false",
  "fn",
  "for",
  "gen",
  "if",
  "impl",
  "in",
  "let",
  "loop",
  "match",
  "mod",
  "move",
  "mut",
  "pub",
  "ref",
  "return",
  "self",
  "static",
  "struct",
  "super",
  "trait",
  "true",
  "type",
  "unsafe",
  "use",
  "where",
  "while",
  "yield",
]);

const LOCALS = new Set(["loader", "call_args", "reply"]);

function parseMetadata(line: string): Metadata | null {
  const match = line.trim().match(/^\[(\w+)(?:\("([^"]*)"\))?\]$/);
  return match?.[1] ? { name: match[1], arg: match[2] ?? null } : null;
}

function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let current = "";
  let depth = 0;
  let quote: string | null = null;
  for (const [index, char] of [...text].entries()) {
    if (quote) {
      current += char;
      if (char === quote && text[index - 1] !== "\\") quote = null;
      continue;
    }
    if (char === '"' || char === "'") quote = char;
    if ("([{".includes(char)) depth += 1;
    if (")]}".includes(char)) depth -= 1;
    if (char === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
      continue;
    }
    current += char;
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}

function parseFallback(raw: string, location: string): Fallback {
  const value = raw.trim();
  if (value === "null") return { kind: "null" };
  if (value === "true" || value === "false") return { kind: "boolean", value: value === "true" };
  if (value === "NaN") return { kind: "number", value: Number.NaN };
  if (/^-?\d+(\.\d+)?$/.test(value)) return { kind: "number", value: Number(value) };
  const quoted = value.match(/^"([^"\\]*)"$|^'([^'\\]*)'$/);
  if (quoted) return { kind: "string", value: quoted[1] ?? quoted[2] ?? "" };
  throw new Error(`Unsupported default value ${value} in ${location}`);
}

function parseParameters(raw: string, location: string): Parameter[] {
  return splitTopLevel(raw.replace(/\/\*[\s\S]*?\*\//g, "")).map((chunk): Parameter => {
    if (chunk.startsWith("...")) return { kind: "rest", name: chunk.slice(3).trim() };
    const match = chunk.match(/^([A-Za-z_]\w*)\s*(?::\s*([^=]+?))?\s*(?:=\s*(.+))?$/);
    if (!match?.[1]) throw new Error(`Unable to parse parameter "${chunk}" in ${location}`);
    const type = (match[2] ?? "*").trim();
    return match[3] === undefined
      ? { kind: "required", name: match[1], type }
      : { kind: "optional", name: match[1], type, fallback: parseFallback(match[3], location) };
  });
}

function parseMethods({ path, text }: Source): Method[] {
  const packageName = text.match(/package\s+([\w.]+)\s*\{/)?.[1] ?? "";
  const methods: Method[] = [];
  let pending: Metadata[] = [];
  let className: string | null = null;
  let namespace: string | null = null;
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    const location = `${path}:${index + 1}`;
    const metadata = parseMetadata(line);
    if (metadata) {
      pending.push(metadata);
      continue;
    }
    const classMatch = line.match(/^\s*public\s+class\s+([A-Za-z_]\w*)/);
    if (classMatch?.[1]) {
      className = classMatch[1];
      namespace = pending.find((entry) => entry.name === "BridgeNamespace")?.arg ?? null;
      pending = [];
      continue;
    }
    const methodMatch = line.match(
      /^\s*public\s+(static\s+)?function\s+([A-Za-z_]\w*)\s*\(([^)]*)\)\s*(?::\s*([\w*]+))?/,
    );
    if (methodMatch?.[2]) {
      if (!className) throw new Error(`Found a method before its class in ${location}`);
      methods.push({
        className,
        classFqn: `${packageName}.${className}`,
        namespace,
        name: methodMatch[2],
        parameters: parseParameters(methodMatch[3] ?? "", location),
        returnType: methodMatch[4] ?? "void",
        isStatic: methodMatch[1] !== undefined,
        metadata: pending,
        location,
      });
      pending = [];
      continue;
    }
    const trimmed = line.trim();
    if (trimmed && !trimmed.startsWith("//")) pending = [];
  }
  return methods;
}

function withRustTypes(method: Method): Parameter[] {
  const types = new Map<string, string>();
  for (const metadata of method.metadata) {
    if (metadata.name !== "BridgeRustType") continue;
    const [name, type] = (metadata.arg ?? "").split(":").map((part) => part.trim());
    if (!name || !type) {
      throw new Error(`BridgeRustType needs "parameter: Type" in ${method.location}`);
    }
    types.set(name, type);
  }
  const parameters = method.parameters.map((parameter): Parameter => {
    const type = types.get(parameter.name);
    if (type === undefined) return parameter;
    types.delete(parameter.name);
    if (parameter.kind !== "required") {
      throw new Error(`BridgeRustType only applies to required parameters in ${method.location}`);
    }
    return { ...parameter, rustType: type };
  });
  const [unknown] = types.keys();
  if (unknown !== undefined) {
    throw new Error(`BridgeRustType names unknown parameter ${unknown} in ${method.location}`);
  }
  return parameters;
}

export function parseBridge(sources: readonly Source[]): Bridge {
  const exports: Entry[] = [];
  const events: Entry[] = [];
  for (const method of sources.flatMap(parseMethods)) {
    const find = (name: string) => method.metadata.find((entry) => entry.name === name);
    const exported = find("BridgeExport");
    const ignored = find("BridgeIgnore");
    const event = find("BridgeEvent");
    const entry = {
      className: method.className,
      classFqn: method.classFqn,
      methodName: method.name,
      parameters: withRustTypes(method),
      returnType: method.returnType,
      location: method.location,
    };
    if (event) {
      if (!event.arg) throw new Error(`BridgeEvent needs a name in ${method.location}`);
      events.push({ ...entry, name: event.arg.trim() });
    }
    if (!method.isStatic || !method.namespace) {
      if (exported || ignored) {
        throw new Error(
          `BridgeExport and BridgeIgnore need a static method in a BridgeNamespace class: ${method.location}`,
        );
      }
      continue;
    }
    if (exported && ignored) {
      throw new Error(`A method can't have both BridgeExport and BridgeIgnore: ${method.location}`);
    }
    if (!exported && !ignored) {
      throw new Error(
        `Static method ${method.name} needs BridgeExport or BridgeIgnore: ${method.location}`,
      );
    }
    if (exported) {
      exports.push({
        ...entry,
        name: `${method.namespace}.${exported.arg?.trim() || method.name}`,
      });
    }
  }
  for (const [kind, entries] of [
    ["export", exports],
    ["event", events],
  ] as const) {
    const seen = new Map<string, Entry>();
    for (const entry of entries) {
      const existing = seen.get(entry.name);
      if (existing) {
        throw new Error(
          `Duplicate bridge ${kind} "${entry.name}" in ${existing.location} and ${entry.location}`,
        );
      }
      seen.set(entry.name, entry);
    }
    entries.sort((a, b) => a.name.localeCompare(b.name));
  }
  return { exports, events };
}

export function renderAs3(bridge: Bridge): string {
  const imports = [...new Set(bridge.exports.map((entry) => entry.classFqn))].sort((a, b) =>
    a.localeCompare(b),
  );
  return [
    "// AUTO-GENERATED FILE. DO NOT EDIT.",
    "package mira.generated",
    "{",
    "  import mira.Externalizer;",
    ...imports.map((fqn) => `  import ${fqn};`),
    "",
    "  public class BridgeRegistryGenerated",
    "  {",
    "    public static function register(external:Externalizer):void",
    "    {",
    ...bridge.exports.map(
      (entry) =>
        `      external.externalize("${entry.name}", ${entry.className}.${entry.methodName});`,
    ),
    "    }",
    "  }",
    "}",
    "",
  ].join("\n");
}

function snakeCase(name: string, location: string): string {
  const snake = name
    .replace(/([a-z\d])([A-Z])/g, "$1_$2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .toLowerCase();
  if (!RUST_KEYWORDS.has(snake)) return snake;
  if (["crate", "self", "super"].includes(snake)) {
    throw new Error(`${name} can't be a Rust identifier: ${location}`);
  }
  return `r#${snake}`;
}

function pascalCase(name: string): string {
  const base = /^on[A-Z]/.test(name) ? name.slice(2) : name;
  return base[0]?.toUpperCase() + base.slice(1);
}

function rustReturnType(type: string): string {
  switch (type) {
    case "void":
      return "()";
    case "Boolean":
      return "bool";
    case "int":
      return "i32";
    case "uint":
      return "u32";
    case "Number":
      return "f64";
    case "String":
      return "Option<String>";
    default:
      return "Value";
  }
}

function rustEventFieldType(type: string): string {
  return type === "String" ? "String" : rustReturnType(type);
}

type Scalar = { readonly type: string; readonly encode: (expression: string) => string };

function scalar(type: string): Scalar {
  switch (type) {
    case "String":
      return { type: "&str", encode: (value) => `Value::String(${value}.to_owned())` };
    case "Boolean":
      return { type: "bool", encode: (value) => `Value::Bool(${value})` };
    case "int":
      return { type: "i32", encode: (value) => `Value::Number(f64::from(${value}))` };
    case "uint":
      return { type: "u32", encode: (value) => `Value::Number(f64::from(${value}))` };
    case "Number":
      return { type: "f64", encode: (value) => `Value::Number(${value})` };
    default:
      return { type: "Value", encode: (value) => value };
  }
}

function rustFallback(type: string, fallback: Fallback, location: string): string {
  switch (fallback.kind) {
    case "string":
      return JSON.stringify(fallback.value);
    case "boolean":
      return String(fallback.value);
    case "number":
      if (Number.isNaN(fallback.value)) return "f64::NAN";
      return type === "Number" && Number.isInteger(fallback.value)
        ? `${fallback.value}.0`
        : String(fallback.value);
    case "null":
      throw new Error(`null default needs a nullable type in ${location}`);
  }
}

function rustArgument(
  parameter: Parameter,
  location: string,
): { signature: string; value: string } {
  const name = snakeCase(parameter.name, location);
  if (parameter.kind === "rest") return { signature: `${name}: Vec<Value>`, value: name };
  if (parameter.kind === "required" && parameter.rustType !== undefined) {
    return {
      signature: `${name}: crate::bridge::${parameter.rustType}`,
      value: `Value::from(${name})`,
    };
  }
  const { type, encode } = scalar(parameter.type);
  if (parameter.kind === "required") return { signature: `${name}: ${type}`, value: encode(name) };
  if (parameter.fallback.kind === "null") {
    const nullable = type === "&str" || type === "Value";
    if (!nullable) throw new Error(`null default needs a nullable type in ${location}`);
    const value =
      type === "Value"
        ? `${name}.unwrap_or(Value::Null)`
        : `${name}.map_or(Value::Null, |value| ${encode("value")})`;
    return { signature: `${name}: Option<${type}>`, value };
  }
  const fallback = rustFallback(parameter.type, parameter.fallback, location);
  return { signature: `${name}: Option<${type}>`, value: encode(`${name}.unwrap_or(${fallback})`) };
}

function renderRustExport(entry: Entry): string {
  const name = snakeCase(entry.name.slice(entry.name.indexOf(".") + 1), entry.location);
  const rest = entry.parameters.find((parameter) => parameter.kind === "rest");
  const fixed = entry.parameters.filter((parameter) => parameter.kind !== "rest");
  const args = fixed.map((parameter) => rustArgument(parameter, entry.location));
  const restArgument = rest ? rustArgument(rest, entry.location) : null;
  const signature = ["loader: &impl Loader", ...args.map((arg) => arg.signature)];
  if (restArgument) signature.push(restArgument.signature);
  const reserved = entry.parameters
    .map((parameter) => snakeCase(parameter.name, entry.location))
    .find((parameter) => LOCALS.has(parameter));
  if (reserved)
    throw new Error(`Parameter ${reserved} clashes with generated code: ${entry.location}`);
  const values = `vec![${args.map((arg) => arg.value).join(", ")}]`;
  const body = restArgument
    ? [`let mut call_args = ${values};`, `call_args.extend(${restArgument.value});`]
    : [`let call_args = ${values};`];
  return [
    `pub(crate) fn ${name}(${signature.join(", ")}) -> Result<${rustReturnType(entry.returnType)}, CallError> {`,
    ...body,
    `let reply = loader.call(${JSON.stringify(entry.name)}, call_args)?;`,
    `Ok(decode(${JSON.stringify(entry.name)}, reply)?)`,
    "}",
  ].join("\n");
}

function renderRustEvents(events: readonly Entry[]): string {
  const variants = events.map((event) => {
    const fields = event.parameters.map((parameter) => {
      if (parameter.kind !== "required") {
        throw new Error(`Event parameters must be required: ${event.location}`);
      }
      return `${snakeCase(parameter.name, event.location)}: ${rustEventFieldType(parameter.type)}`;
    });
    return fields.length
      ? `${pascalCase(event.name)} { ${fields.join(", ")} },`
      : `${pascalCase(event.name)},`;
  });
  const arms = events.map((event) => {
    const fields = event.parameters.map(
      (parameter, index) =>
        `${snakeCase(parameter.name, event.location)}: decode(${JSON.stringify(event.name)}, arg(args, ${index}))?`,
    );
    const value = fields.length
      ? `Self::${pascalCase(event.name)} { ${fields.join(", ")} }`
      : `Self::${pascalCase(event.name)}`;
    return `${JSON.stringify(event.name)} => ${value},`;
  });
  return [
    "#[derive(Debug, Clone, PartialEq)]",
    "pub(crate) enum Event {",
    ...variants,
    "}",
    "",
    "impl Event {",
    "pub(crate) fn parse(name: &str, args: &[Value]) -> Result<Option<Self>, DecodeError> {",
    "Ok(Some(match name {",
    ...arms,
    "_ => return Ok(None),",
    "}))",
    "}",
    "}",
  ].join("\n");
}

export function renderRust(bridge: Bridge): string {
  const namespaces = Map.groupBy(bridge.exports, (entry) =>
    entry.name.slice(0, entry.name.indexOf(".")),
  );
  const modules = [...namespaces].map(([namespace, entries]) =>
    [
      `pub(crate) mod ${snakeCase(namespace, entries[0]?.location ?? namespace)} {`,
      "use crate::bridge::{CallError, Loader, Value, decode};",
      "",
      entries.map(renderRustExport).join("\n\n"),
      "}",
    ].join("\n"),
  );
  const source = [
    "// AUTO-GENERATED FILE. DO NOT EDIT.",
    "#![allow(dead_code)]",
    "",
    "use super::{DecodeError, Value, arg, decode};",
    "",
    renderRustEvents(bridge.events),
    "",
    modules.join("\n\n"),
    "",
  ].join("\n");
  return execFileSync("rustfmt", ["--edition", "2024", "--emit", "stdout"], {
    input: source,
    encoding: "utf8",
  });
}

const root = fileURLToPath(new URL("../", import.meta.url));
const as3Root = join(root, "as3", "src", "mira");
const outputs = {
  as3: join(as3Root, "generated", "BridgeRegistryGenerated.as"),
  rust: join(root, "src", "bridge", "generated.rs"),
};

async function readSources(): Promise<Source[]> {
  const entries = await readdir(as3Root, { recursive: true, withFileTypes: true });
  const paths = entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".as"))
    .map((entry) => join(entry.parentPath, entry.name))
    .filter((path) => path !== outputs.as3)
    .sort((a, b) => a.localeCompare(b));
  return Promise.all(
    paths.map(async (path) => ({ path: relative(root, path), text: await readFile(path, "utf8") })),
  );
}

async function main(): Promise<void> {
  const { values } = parseArgs({ options: { check: { type: "boolean", default: false } } });
  const bridge = parseBridge(await readSources());
  const rendered = [
    [outputs.as3, renderAs3(bridge)],
    [outputs.rust, renderRust(bridge)],
  ] as const;
  const stale: string[] = [];
  for (const [path, content] of rendered) {
    const existing = await readFile(path, "utf8").catch(() => null);
    if (existing === content) continue;
    if (values.check) stale.push(relative(root, path));
    else await writeFile(path, content);
  }
  if (stale.length) {
    console.error(`Out of date, run pnpm gen:bridge:\n${stale.join("\n")}`);
    process.exitCode = 1;
    return;
  }
  console.log(`${bridge.exports.length} exports, ${bridge.events.length} events`);
}

if (import.meta.main) await main();
