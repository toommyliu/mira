import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const AREAS: Record<string, string> = {
  ScriptBank: "Bank",
  ScriptCombat: "Combat",
  ScriptFaction: "Reputation",
  ScriptHouseInv: "House",
  ScriptInventory: "Inventory",
  ScriptLite: "Lite",
  ScriptMap: "Map",
  ScriptMonster: "Monsters",
  ScriptOption: "Options",
  ScriptPlayer: "Player",
  ScriptQuest: "Quests",
  ScriptServers: "Servers",
  ScriptShop: "Shops",
  ScriptSkill: "Skills",
  ScriptTempInv: "TempInv",
};

type Binding =
  | {
      readonly kind: "object";
      readonly paths: readonly string[];
      readonly static: boolean;
      readonly get: boolean;
      readonly set: boolean;
      readonly fallback: string | null;
      readonly requireNotNull: string | null;
      readonly select: string | null;
    }
  | { readonly kind: "call" | "json"; readonly function: string; readonly fallback: string | null }
  | { readonly kind: "method"; readonly function: string; readonly game: boolean; readonly parameters: number };

const ATTRIBUTE = /\[(ObjectBinding|CallBinding|JsonCallBinding|MethodCallBinding)\((.*)\)\]\s*\n\s*(?:private|public|protected|internal)\s+([^=;(]+?)\s+_(\w+)\s*([=;(])/g;

function named(args: string, name: string): string | null {
  const match = new RegExp(`\\b${name}\\s*=\\s*("(?:[^"\\\\]|\\\\.)*"|true|false)`).exec(args);
  if (!match) return null;
  const value = match[1]!;
  return value.startsWith('"') ? JSON.parse(value) : value;
}

function parameterCount(source: string, open: number): number {
  let depth = 0;
  let count = 0;
  let empty = true;
  for (const character of source.slice(open)) {
    if (character === ")" && depth === 0) break;
    if (character === "<" || character === "(" || character === "[") depth++;
    else if (character === ">" || character === ")" || character === "]") depth--;
    else if (character === "," && depth === 0) count++;
    else if (empty && !/\s/.test(character)) empty = false;
  }
  return empty ? 0 : count + 1;
}

function positional(args: string): string[] {
  return [...args.replaceAll(/\b\w+\s*=\s*("(?:[^"\\]|\\.)*"|\w+)/g, "").matchAll(/"((?:[^"\\]|\\.)*)"/g)].map(
    (match) => JSON.parse(`"${match[1]}"`),
  );
}

export function parseBindings(area: string, source: string): Record<string, Binding> {
  const bindings: Record<string, Binding> = {};
  for (const match of source.matchAll(ATTRIBUTE)) {
    const [, attribute, args, , field, after] = match;
    const member = `${area}.${field![0]!.toUpperCase()}${field!.slice(1)}`;
    const fallback = named(args!, "Default");
    if (attribute === "ObjectBinding") {
      bindings[member] = {
        kind: "object",
        paths: positional(args!),
        static: named(args!, "IsStatic") === "true",
        get: named(args!, "Get") !== "false",
        set: named(args!, "HasSetter") === "true",
        fallback,
        requireNotNull: named(args!, "RequireNotNull"),
        select: named(args!, "Select"),
      };
    } else if (attribute === "MethodCallBinding") {
      if (after !== "(" || named(args!, "RunMethodPre") === "true" || named(args!, "RunMethodPost") === "true") continue;
      bindings[member] = {
        kind: "method",
        function: positional(args!)[0]!,
        game: named(args!, "GameFunction") === "true",
        parameters: parameterCount(source, match.index + match[0].length),
      };
    } else {
      bindings[member] = { kind: attribute === "CallBinding" ? "call" : "json", function: positional(args!)[0]!, fallback };
    }
  }
  return bindings;
}

async function main() {
  const skua = process.argv[2];
  if (!skua) throw new Error("Usage: node scripts/skua-bindings.ts <Skua checkout>");
  const folder = join(skua, "Skua.Core", "Scripts");
  const bindings: Record<string, Binding> = {};
  for (const file of (await readdir(folder, { recursive: true })).sort()) {
    const area = AREAS[file.split("/").pop()!.replace(/\.cs$/, "")];
    if (area) Object.assign(bindings, parseBindings(area, await readFile(join(folder, file), "utf8")));
  }
  const output = fileURLToPath(new URL("../skua/Bot/bindings.json", import.meta.url));
  await writeFile(output, `${JSON.stringify(bindings, null, 2)}\n`);
  console.log(`${Object.keys(bindings).length} bindings`);
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
