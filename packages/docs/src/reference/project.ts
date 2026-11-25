import { existsSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { join, normalize } from "node:path";

import { Application, Converter, type JSONOutput, normalizePath } from "typedoc";
import ts from "typescript";

import { modules } from "../../../mira/src/modules.ts";
import { type Module, type Reference, reference } from "./model.ts";

type Entry = { readonly specifier: string; readonly path: string };

export async function convert(
  entries: readonly Entry[],
  tsconfig: string,
): Promise<JSONOutput.ProjectReflection> {
  const entryPoints = entries.map((entry) => entry.path);
  const app = await Application.bootstrap({
    entryPoints,
    tsconfig,
    readme: "none",
    excludeExternals: true,
    excludePrivate: true,
    excludeInternal: true,
    sort: ["source-order"],
    logLevel: "Warn",
  });
  // TypeDoc converts a constraint from the checker's type, which drops aliases such as GameEvent["type"].
  app.converter.on(Converter.EVENT_CREATE_TYPE_PARAMETER, (context, parameter) => {
    const constraint = context
      .getSymbolFromReflection(parameter)
      ?.declarations?.find(ts.isTypeParameterDeclaration)?.constraint;
    if (constraint !== undefined) {
      parameter.type = context.converter.convertType(context.withScope(parameter), constraint);
    }
  });
  const project = await app.convert();
  if (project === undefined) throw new Error(`TypeDoc could not convert ${entryPoints.join(", ")}`);
  for (const module of project.children ?? []) {
    const file = module.sources?.[0]?.fullFileName;
    const entry = entries.find((candidate) => normalize(candidate.path) === file);
    if (entry !== undefined) module.name = entry.specifier;
  }
  app.validate(project);
  if (app.logger.hasErrors() || app.logger.hasWarnings()) {
    throw new Error(
      `TypeDoc reported problems in ${entryPoints.join(", ")}; see its warnings above`,
    );
  }
  return app.serializer.projectToObject(project, normalizePath(process.cwd()));
}

const runtime = join(process.cwd(), "..", "mira");
const scriptPackages = join(process.cwd(), "..", "..", "script-packages");

type Source = Module & Entry;

function manifest(path: string, json: unknown) {
  const fields = new Map<string, unknown>(
    typeof json === "object" && json !== null ? Object.entries(json) : [],
  );
  const field = (key: string): string | undefined => {
    const value = fields.get(key);
    if (value === undefined || typeof value === "string") return value;
    throw new Error(`${path} has a ${key} that isn't a string`);
  };
  const name = field("name");
  if (name === undefined) throw new Error(`${path} has no name`);
  return { name, main: field("main"), description: field("description")?.trim() };
}

async function vendored(): Promise<Source[]> {
  if (!existsSync(scriptPackages)) return [];
  const folders = await readdir(scriptPackages, { withFileTypes: true });
  const sources: Source[] = [];
  for (const folder of folders.filter((entry) => entry.isDirectory())) {
    const dir = join(scriptPackages, folder.name);
    const path = join(dir, "package.json");
    const { name, main, description } = manifest(path, JSON.parse(await readFile(path, "utf8")));
    if (main === undefined) continue;
    sources.push({
      specifier: name,
      slug: `packages/${folder.name}`,
      origin: "vendored",
      description,
      path: join(dir, main),
    });
  }
  return sources.toSorted((a, b) => a.specifier.localeCompare(b.specifier));
}

async function build(): Promise<Reference> {
  const sources: Source[] = [
    ...modules.map((module): Source => ({
      specifier: module.specifier,
      slug: module.specifier === "mira" ? "" : module.specifier.replace(/^mira\//, ""),
      origin: "runtime",
      description: undefined,
      path: join(runtime, "src", `${module.source}.ts`),
    })),
    ...(await vendored()),
  ];
  const project = await convert(sources, join(process.cwd(), "reference.tsconfig.json"));
  return reference(project, sources);
}

let loaded: Promise<Reference> | undefined;

export function load(): Promise<Reference> {
  loaded ??= build();
  return loaded;
}
