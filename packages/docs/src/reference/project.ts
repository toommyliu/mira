import { join } from "node:path";

import { Application, type JSONOutput, normalizePath } from "typedoc";

import { modules } from "../../../mira/src/modules.ts";
import { type Reference, reference } from "./model.ts";

export async function convert(
  entryPoints: readonly string[],
  tsconfig: string,
): Promise<JSONOutput.ProjectReflection> {
  const app = await Application.bootstrap({
    entryPoints: [...entryPoints],
    tsconfig,
    excludeExternals: true,
    excludePrivate: true,
    excludeInternal: true,
    sort: ["source-order"],
    logLevel: "Warn",
  });
  const project = await app.convert();
  if (project === undefined) throw new Error(`TypeDoc could not convert ${entryPoints.join(", ")}`);
  return app.serializer.projectToObject(project, normalizePath(process.cwd()));
}

const runtime = join(process.cwd(), "..", "mira");

let loaded: Promise<Reference> | undefined;

export function load(): Promise<Reference> {
  loaded ??= convert(
    modules.map((module) => join(runtime, "src", `${module.source}.ts`)),
    join(runtime, "tsconfig.json"),
  ).then((project) =>
    reference(
      project,
      modules.map((module) => module.specifier),
    ),
  );
  return loaded;
}
