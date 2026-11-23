import type { JSONOutput as J } from "typedoc";

import type { Declaration, Member, Signature, TypeNode } from "./model.ts";

export type Token = {
  readonly text: string;
  readonly kind?:
    | "keyword"
    | "type"
    | "builtin"
    | "string"
    | "number"
    | "function"
    | "name"
    | "punctuation";
  readonly href?: string | undefined;
  readonly target?: number;
};

type Links = ReadonlyMap<number, string>;

const WIDTH = 72;

const text = (value: string): Token => ({ text: value });
const punctuation = (value: string): Token => ({ text: value, kind: "punctuation" });
const keyword = (value: string): Token => ({ text: value, kind: "keyword" });
const name = (value: string): Token => ({ text: value, kind: "name" });

const length = (tokens: readonly Token[]) =>
  tokens.reduce((sum, token) => sum + token.text.length, 0);

function join(parts: readonly Token[][], separator: Token[]): Token[] {
  return parts.flatMap((part, index) => (index === 0 ? part : [...separator, ...part]));
}

function indented(tokens: readonly Token[], indent: string): Token[] {
  return tokens.map((token) =>
    token.text.includes("\n")
      ? { ...token, text: token.text.replaceAll("\n", `\n${indent}`) }
      : token,
  );
}

function literal(value: J.LiteralType["value"]): Token {
  if (value === null) return { text: "null", kind: "builtin" };
  if (typeof value === "string") return { text: JSON.stringify(value), kind: "string" };
  if (typeof value === "object")
    return { text: `${value.negative ? "-" : ""}${value.value}n`, kind: "number" };
  return { text: String(value), kind: "number" };
}

function needsParens(type: TypeNode): boolean {
  return (
    type.type === "union" ||
    type.type === "intersection" ||
    type.type === "conditional" ||
    (type.type === "reflection" &&
      type.declaration.children === undefined &&
      type.declaration.signatures?.length === 1)
  );
}

function wrapped(type: TypeNode, links: Links): Token[] {
  const tokens = typeTokens(type, links);
  return needsParens(type) ? [punctuation("("), ...tokens, punctuation(")")] : tokens;
}

function typeArguments(args: readonly TypeNode[] | undefined, links: Links): Token[] {
  if (args === undefined || args.length === 0) return [];
  return [
    punctuation("<"),
    ...join(
      args.map((arg) => typeTokens(arg, links)),
      [punctuation(", ")],
    ),
    punctuation(">"),
  ];
}

export function typeParameters(
  parameters: readonly J.TypeParameterReflection[],
  links: Links,
): Token[] {
  if (parameters.length === 0) return [];
  return [
    punctuation("<"),
    ...join(
      parameters.map((parameter) => [
        ...(parameter.flags.isConst ? [keyword("const ")] : []),
        { text: parameter.name, kind: "type" },
        ...(parameter.type ? [keyword(" extends "), ...typeTokens(parameter.type, links)] : []),
        ...(parameter.default ? [punctuation(" = "), ...typeTokens(parameter.default, links)] : []),
      ]),
      [punctuation(", ")],
    ),
    punctuation(">"),
  ];
}

function parameterTokens(parameter: J.ParameterReflection, links: Links): Token[] {
  const optional = parameter.flags.isOptional === true || parameter.defaultValue !== undefined;
  return [
    ...(parameter.flags.isRest ? [punctuation("...")] : []),
    name(parameter.name),
    punctuation(optional ? "?: " : ": "),
    ...typeTokens(parameter.type ?? { type: "intrinsic", name: "unknown" }, links),
  ];
}

function parameterList(
  parameters: readonly J.ParameterReflection[],
  links: Links,
  room: number,
): Token[] {
  const each = parameters.map((parameter) => parameterTokens(parameter, links));
  const inline = [punctuation("("), ...join(each, [punctuation(", ")]), punctuation(")")];
  if (length(inline) <= room) return inline;
  return [
    punctuation("("),
    ...each.flatMap((tokens) => [text("\n  "), ...indented(tokens, "  "), punctuation(",")]),
    text("\n"),
    punctuation(")"),
  ];
}

function propertyTokens(property: J.DeclarationReflection, links: Links): Token[] {
  if (property.signatures !== undefined) {
    return join(
      property.signatures.map((signature) => [
        name(property.name),
        ...functionTokens(signature, links, ": "),
      ]),
      [punctuation("; ")],
    );
  }
  return [
    name(property.name),
    punctuation(property.flags.isOptional ? "?: " : ": "),
    ...typeTokens(property.type ?? { type: "intrinsic", name: "unknown" }, links),
  ];
}

function functionTokens(signature: J.SignatureReflection, links: Links, arrow: string): Token[] {
  return [
    ...typeParameters(signature.typeParameters ?? [], links),
    ...parameterList(signature.parameters ?? [], links, WIDTH),
    punctuation(arrow),
    ...typeTokens(signature.type ?? { type: "intrinsic", name: "void" }, links),
  ];
}

function braces(entries: readonly Token[][]): Token[] {
  if (entries.length === 0) return [punctuation("{}")];
  const inline = [punctuation("{ "), ...join(entries, [punctuation("; ")]), punctuation(" }")];
  if (length(inline) <= WIDTH && !inline.some((token) => token.text.includes("\n"))) return inline;
  return [
    punctuation("{"),
    ...entries.flatMap((entry) => [text("\n  "), ...indented(entry, "  "), punctuation(";")]),
    text("\n"),
    punctuation("}"),
  ];
}

function objectTokens(declaration: J.DeclarationReflection, links: Links): Token[] {
  const signatures = declaration.signatures ?? [];
  if (declaration.children === undefined && signatures.length === 1 && signatures[0]) {
    return functionTokens(signatures[0], links, " => ");
  }
  return braces([
    ...signatures.map((signature) => functionTokens(signature, links, ": ")),
    ...(declaration.indexSignatures ?? []).map((index) => [
      punctuation("["),
      ...(index.parameters ?? []).flatMap((parameter) => parameterTokens(parameter, links)),
      punctuation("]: "),
      ...typeTokens(index.type ?? { type: "intrinsic", name: "unknown" }, links),
    ]),
    ...(declaration.children ?? []).map((child) => propertyTokens(child, links)),
  ]);
}

function externalName(target: J.ReflectionSymbolId, fallback: string): string {
  if (target.packageName !== "effect") return fallback;
  const module = /([^/]+)\.ts$/.exec(target.packagePath)?.[1];
  return module === undefined ? fallback : `${module}.${target.qualifiedName}`;
}

export function typeTokens(type: TypeNode, links: Links): Token[] {
  switch (type.type) {
    case "intrinsic":
      return [{ text: type.name, kind: "builtin" }];
    case "literal":
      return [literal(type.value)];
    case "reference": {
      const label: Token =
        typeof type.target === "number"
          ? { text: type.name, kind: "type", href: links.get(type.target), target: type.target }
          : { text: externalName(type.target, type.name), kind: "type" };
      return [label, ...typeArguments(type.typeArguments, links)];
    }
    case "array":
      return [...wrapped(type.elementType, links), punctuation("[]")];
    case "union": {
      const members = type.types.map((member) => wrapped(member, links));
      const inline = join(members, [punctuation(" | ")]);
      if (length(inline) <= WIDTH && !inline.some((token) => token.text.includes("\n")))
        return inline;
      return members.flatMap((member) => [
        text("\n  "),
        punctuation("| "),
        ...indented(member, "  "),
      ]);
    }
    case "intersection":
      return join(
        type.types.map((member) => wrapped(member, links)),
        [punctuation(" & ")],
      );
    case "tuple":
      return [
        punctuation("["),
        ...join(
          (type.elements ?? []).map((element) => typeTokens(element, links)),
          [punctuation(", ")],
        ),
        punctuation("]"),
      ];
    case "namedTupleMember":
      return [
        name(type.name),
        punctuation(type.isOptional ? "?: " : ": "),
        ...typeTokens(type.element, links),
      ];
    case "optional":
      return [...typeTokens(type.elementType, links), punctuation("?")];
    case "rest":
      return [punctuation("..."), ...typeTokens(type.elementType, links)];
    case "typeOperator":
      return type.operator === "readonly"
        ? typeTokens(type.target, links)
        : [keyword(`${type.operator} `), ...wrapped(type.target, links)];
    case "query":
      return [keyword("typeof "), ...typeTokens(type.queryType, links)];
    case "indexedAccess":
      return [
        ...wrapped(type.objectType, links),
        punctuation("["),
        ...typeTokens(type.indexType, links),
        punctuation("]"),
      ];
    case "conditional":
      return [
        ...wrapped(type.checkType, links),
        keyword(" extends "),
        ...typeTokens(type.extendsType, links),
        punctuation(" ? "),
        ...typeTokens(type.trueType, links),
        punctuation(" : "),
        ...typeTokens(type.falseType, links),
      ];
    case "mapped":
      return [
        punctuation("{ "),
        ...(type.readonlyModifier === "+" ? [keyword("readonly ")] : []),
        punctuation("["),
        { text: type.parameter, kind: "type" },
        keyword(" in "),
        ...typeTokens(type.parameterType, links),
        ...(type.nameType ? [keyword(" as "), ...typeTokens(type.nameType, links)] : []),
        punctuation(type.optionalModifier === "+" ? "]?: " : "]: "),
        ...typeTokens(type.templateType, links),
        punctuation(" }"),
      ];
    case "templateLiteral":
      return [
        { text: `\`${type.head}`, kind: "string" },
        ...type.tail.flatMap(([inner, tail]) => [
          { text: "${", kind: "punctuation" } as const,
          ...typeTokens(inner, links),
          { text: "}", kind: "punctuation" } as const,
          { text: tail, kind: "string" } as const,
        ]),
        { text: "`", kind: "string" },
      ];
    case "predicate":
      return [
        ...(type.asserts ? [keyword("asserts ")] : []),
        name(type.name),
        ...(type.targetType ? [keyword(" is "), ...typeTokens(type.targetType, links)] : []),
      ];
    case "inferred":
      return [keyword("infer "), { text: type.name, kind: "type" }];
    case "reflection":
      return objectTokens(type.declaration, links);
    case "unknown":
      return [text(type.name)];
    default: {
      const exhaustive: never = type;
      return exhaustive;
    }
  }
}

export function signatureTokens(path: string, signature: Signature, links: Links): Token[] {
  const head: Token[] = [
    ...(signature.result.kind === "yields" ? [keyword("yield* ")] : []),
    { text: path, kind: "function" },
    ...typeParameters(signature.typeParameters, links),
  ];
  const result = [punctuation(": "), ...typeTokens(signature.result.type, links)];
  return [
    ...head,
    ...parameterList(signature.parameters, links, WIDTH - length(head) - length(result)),
    ...result,
  ];
}

export function valueTokens(member: Member & { body: { kind: "value" } }, links: Links): Token[] {
  return [
    name(member.path),
    punctuation(member.body.optional ? "?: " : ": "),
    ...typeTokens(member.body.type, links),
  ];
}

export function declarationTokens(declaration: Declaration, links: Links): Token[] {
  const head: Token[] = [
    keyword(`${declaration.keyword} `),
    { text: declaration.name, kind: "type" },
    ...typeParameters(declaration.typeParameters, links),
  ];
  if (declaration.body.kind === "shape") return head;
  const body = typeTokens(declaration.body.type, links);
  return [...head, punctuation(body[0]?.text.startsWith("\n") ? " =" : " = "), ...body];
}

export function previewTokens(declaration: Declaration, links: Links): Token[] {
  if (declaration.body.kind === "alias") return declarationTokens(declaration, links);
  const entries = [...declaration.body.properties, ...declaration.body.methods].flatMap(
    (member): Token[][] =>
      member.body.kind === "value"
        ? [
            [
              name(member.path),
              punctuation(member.body.optional ? "?: " : ": "),
              ...typeTokens(member.body.type, links),
            ],
          ]
        : member.body.signatures.map((signature) => [
            name(member.path),
            ...parameterList(signature.parameters, links, WIDTH),
            punctuation(": "),
            ...typeTokens(signature.result.type, links),
          ]),
  );
  return [
    ...declarationTokens(declaration, links),
    punctuation(declaration.keyword === "type" ? " = " : " "),
    ...braces(entries),
  ];
}

export type Line = { readonly indent: number; readonly tokens: readonly Token[] };

export function lines(tokens: readonly Token[]): Line[] {
  const rows: Token[][] = [[]];
  for (const token of tokens) {
    for (const [index, part] of token.text.split("\n").entries()) {
      if (index > 0) rows.push([]);
      if (part !== "") rows[rows.length - 1]?.push({ ...token, text: part });
    }
  }
  return rows.map((row) => ({
    indent: /^ */.exec(row.map((token) => token.text).join(""))?.[0].length ?? 0,
    tokens: row,
  }));
}
