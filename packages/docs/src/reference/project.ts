import { join } from "node:path";

import { Application, Converter, type JSONOutput, normalizePath } from "typedoc";
import ts from "typescript";

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
  app.validate(project);
  if (app.logger.hasErrors() || app.logger.hasWarnings()) {
    throw new Error(
      `TypeDoc reported problems in ${entryPoints.join(", ")}; see its warnings above`,
    );
  }
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
