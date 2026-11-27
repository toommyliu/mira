import { type JSONOutput as J, ReflectionKind } from "typedoc";

export type TypeNode = J.SomeType;
export type Comment = J.Comment;

export type Signature = {
  readonly typeParameters: readonly J.TypeParameterReflection[];
  readonly parameters: readonly J.ParameterReflection[];
  readonly comment: Comment | undefined;
  readonly result:
    | { readonly kind: "yields"; readonly type: TypeNode; readonly error: TypeNode | undefined }
    | { readonly kind: "returns"; readonly type: TypeNode };
};

export type Member = {
  readonly anchor: string;
  readonly path: string;
  readonly comment: Comment | undefined;
  readonly body:
    | { readonly kind: "call"; readonly signatures: readonly Signature[] }
    | { readonly kind: "value"; readonly type: TypeNode; readonly optional: boolean };
};

export type Variant = {
  readonly anchor: string;
  readonly name: string;
  readonly comment: Comment | undefined;
  readonly type: J.ReflectionType;
  readonly properties: readonly Member[];
};

export type Declaration = {
  readonly anchor: string;
  readonly name: string;
  readonly keyword: "type" | "interface" | "class";
  readonly comment: Comment | undefined;
  readonly typeParameters: readonly J.TypeParameterReflection[];
  readonly body:
    | { readonly kind: "alias"; readonly type: TypeNode }
    | {
        readonly kind: "variants";
        readonly type: J.UnionType;
        readonly variants: readonly Variant[];
      }
    | {
        readonly kind: "shape";
        readonly properties: readonly Member[];
        readonly methods: readonly Member[];
      };
};

export type Module = {
  readonly specifier: string;
  readonly slug: string;
  readonly origin: "runtime" | "vendored";
  readonly description: string | undefined;
};

export type Page = {
  readonly slug: string;
  readonly origin: Module["origin"];
  readonly title: string;
  readonly specifier: string;
  readonly binding:
    | { readonly kind: "named"; readonly name: string }
    | { readonly kind: "namespace"; readonly name: string }
    | undefined;
  readonly comment: Comment | undefined;
  readonly namespaces: readonly { readonly title: string; readonly slug: string }[];
  readonly members: readonly Member[];
  readonly types: readonly Declaration[];
};

export type Reference = {
  readonly pages: readonly Page[];
  readonly links: ReadonlyMap<number, string>;
  readonly declarations: ReadonlyMap<number, Declaration>;
};

const typeKinds = ReflectionKind.TypeAlias | ReflectionKind.Interface | ReflectionKind.Class;

function isNamespace(
  child: J.DeclarationReflection,
): child is J.DeclarationReflection & { type: J.ReflectionType } {
  return (
    child.kind === ReflectionKind.Variable &&
    child.type?.type === "reflection" &&
    child.type.declaration.children !== undefined &&
    child.type.declaration.signatures === undefined
  );
}

function isReference(type: J.ReferenceType, packageName: string, qualifiedName: string) {
  return (
    typeof type.target === "object" &&
    type.target.packageName === packageName &&
    type.target.qualifiedName === qualifiedName
  );
}

function yielded(type: TypeNode): readonly (TypeNode | undefined)[] {
  if (type.type !== "reference") return [];
  if (isReference(type, "effect", "Effect")) return type.typeArguments ?? [];
  if (isReference(type, "typescript", "Generator")) return [type.typeArguments?.[1]];
  return [];
}

function named(parameters: readonly J.ParameterReflection[]): J.ParameterReflection[] {
  const taken = new Set(parameters.map((parameter) => parameter.name));
  return parameters.map((parameter) => {
    if (parameter.name !== "__namedParameters") return parameter;
    const typeName = parameter.type?.type === "reference" ? parameter.type.name : "value";
    const fallback = typeName.charAt(0).toLowerCase() + typeName.slice(1);
    const name = taken.has("options") ? fallback : "options";
    taken.add(name);
    return { ...parameter, name };
  });
}

function signature(sig: J.SignatureReflection): Signature {
  const type = sig.type ?? { type: "intrinsic", name: "void" };
  const [value, error] = yielded(type);
  return {
    typeParameters: sig.typeParameters ?? [],
    parameters: named(sig.parameters ?? []),
    comment: sig.comment,
    result:
      value === undefined
        ? { kind: "returns", type }
        : {
            kind: "yields",
            type: value,
            error: error?.type === "intrinsic" && error.name === "never" ? undefined : error,
          },
  };
}

function callSignatures(
  child: J.DeclarationReflection,
): readonly J.SignatureReflection[] | undefined {
  if (child.signatures !== undefined) return child.signatures;
  if (child.type?.type === "reflection" && child.type.declaration.children === undefined) {
    return child.type.declaration.signatures;
  }
  return undefined;
}

function members(children: readonly J.DeclarationReflection[], prefix: string): Member[] {
  return children
    .filter((child) => !child.flags.isExternal && child.kind !== ReflectionKind.Constructor)
    .flatMap((child): Member[] => {
      const anchor = prefix === "" ? child.name : `${prefix}.${child.name}`;
      const signatures = callSignatures(child);
      if (signatures !== undefined) {
        return [
          {
            anchor,
            path: anchor,
            comment: child.comment ?? signatures[0]?.comment,
            body: { kind: "call", signatures: signatures.map(signature) },
          },
        ];
      }
      if (child.type?.type === "reflection" && child.type.declaration.children !== undefined) {
        return members(child.type.declaration.children, anchor);
      }
      return [
        {
          anchor,
          path: anchor,
          comment: child.comment,
          body: {
            kind: "value",
            type: child.type ?? { type: "intrinsic", name: "unknown" },
            optional: child.flags.isOptional === true,
          },
        },
      ];
    });
}

function tagged(objects: readonly J.ReflectionType[], key: string) {
  const pairs = objects.flatMap((object) => {
    const field = object.declaration.children?.find((child) => child.name === key);
    return field?.type?.type === "literal" && typeof field.type.value === "string"
      ? [{ object, name: field.type.value }]
      : [];
  });
  const distinct = new Set(pairs.map((pair) => pair.name)).size === objects.length;
  return pairs.length === objects.length && distinct ? pairs : undefined;
}

function aliasBody(type: TypeNode, owner: string): Declaration["body"] {
  if (type.type !== "union") return { kind: "alias", type };
  const objects = type.types.filter((member) => member.type === "reflection");
  if (objects.length !== type.types.length) return { kind: "alias", type };
  for (const { name: key } of objects[0]?.declaration.children ?? []) {
    const pairs = tagged(objects, key);
    if (pairs === undefined) continue;
    return {
      kind: "variants",
      type,
      variants: pairs.map(({ object, name }, index) => {
        const summary = type.elementSummaries?.[index] ?? [];
        return {
          anchor: `${owner}.${name}`,
          name,
          comment: summary.length === 0 ? undefined : { summary },
          type: object,
          properties: members(
            (object.declaration.children ?? []).filter((child) => child.name !== key),
            "",
          ),
        };
      }),
    };
  }
  return { kind: "alias", type };
}

function declaration(child: J.DeclarationReflection): Declaration {
  const keyword =
    child.kind === ReflectionKind.Class
      ? "class"
      : child.kind === ReflectionKind.Interface
        ? "interface"
        : "type";
  const shape = members(child.children ?? [], "");
  return {
    anchor: child.name,
    name: child.name,
    keyword,
    comment: child.comment,
    typeParameters: child.typeParameters ?? [],
    body:
      child.type === undefined
        ? {
            kind: "shape",
            properties: shape.filter((member) => member.body.kind === "value"),
            methods: shape.filter((member) => member.body.kind === "call"),
          }
        : aliasBody(child.type, child.name),
  };
}

export function* targets(node: unknown): Generator<number> {
  if (Array.isArray(node)) {
    for (const item of node) yield* targets(item);
    return;
  }
  if (typeof node !== "object" || node === null) return;
  if ("type" in node && node.type === "reference" && "target" in node) {
    if (typeof node.target === "number") yield node.target;
  }
  for (const [key, value] of Object.entries(node)) {
    if (key !== "comment") yield* targets(value);
  }
}

function owners(
  namespaces: readonly J.DeclarationReflection[],
  types: readonly J.DeclarationReflection[],
): Map<number, string> {
  const byId = new Map<number, J.DeclarationReflection>(types.map((type) => [type.id, type]));
  const users = new Map<number, Set<string>>();
  for (const namespace of namespaces) {
    const queue = [...targets(namespace.type)];
    const seen = new Set<number>();
    for (let id = queue.pop(); id !== undefined; id = queue.pop()) {
      const type = byId.get(id);
      if (seen.has(id) || type === undefined) continue;
      seen.add(id);
      users.set(id, (users.get(id) ?? new Set()).add(namespace.name));
      queue.push(...targets(type));
    }
  }
  const owned = new Map<number, string>();
  for (const [id, names] of users) {
    const [only, ...rest] = names;
    if (only !== undefined && rest.length === 0) owned.set(id, only);
  }
  for (const type of types) {
    const group = type.comment?.blockTags?.find((tag) => tag.tag === "@group");
    if (group === undefined) continue;
    const title = group.content
      .map((part) => part.text)
      .join("")
      .trim();
    if (!namespaces.some((namespace) => namespace.name === title)) {
      throw new Error(`${type.name} is in @group ${title}, which names no namespace`);
    }
    owned.set(type.id, title);
  }
  return owned;
}

function bindingOf(specifier: string): string {
  const last = specifier.split("/").at(-1) ?? specifier;
  return last.charAt(0).toUpperCase() + last.slice(1);
}

export function reference(project: J.ProjectReflection, modules: readonly Module[]): Reference {
  const pages: Page[] = [];
  const links = new Map<number, string>();
  const declarations = new Map<number, Declaration>();
  const converted = project.children ?? [];

  for (const { specifier, slug: moduleSlug, origin, description } of modules) {
    const module = converted.find((candidate) => candidate.name === specifier);
    if (module === undefined) throw new Error(`TypeDoc produced no module named ${specifier}`);
    const children = module.children ?? [];
    const namespaces = children
      .filter(isNamespace)
      .toSorted((a, b) => a.name.localeCompare(b.name));
    const types = children.filter((child) => (child.kind & typeKinds) !== 0);
    const values = children.filter(
      (child) => !isNamespace(child) && (child.kind & typeKinds) === 0,
    );
    const owned = owners(namespaces, types);
    const namespaceSlug = (namespace: string) =>
      moduleSlug === "" ? namespace.toLowerCase() : `${moduleSlug}/${namespace.toLowerCase()}`;
    const declared = types.map((type) => ({ id: type.id, declaration: declaration(type) }));
    for (const type of declared) declarations.set(type.id, type.declaration);

    pages.push({
      slug: moduleSlug,
      origin,
      title: specifier,
      specifier,
      binding: values.length === 0 ? undefined : { kind: "namespace", name: bindingOf(specifier) },
      comment:
        module.comment ??
        (description === undefined
          ? undefined
          : { summary: [{ kind: "text", text: description }] }),
      namespaces: namespaces.map((namespace) => ({
        title: namespace.name,
        slug: namespaceSlug(namespace.name),
      })),
      members: members(values, ""),
      types: declared.filter((type) => !owned.has(type.id)).map((type) => type.declaration),
    });
    for (const namespace of namespaces) {
      const slug = namespaceSlug(namespace.name);
      pages.push({
        slug,
        origin,
        title: namespace.name,
        specifier,
        binding: { kind: "named", name: namespace.name },
        comment: namespace.comment,
        namespaces: [],
        members: members(namespace.type.declaration.children ?? [], ""),
        types: declared
          .filter((type) => owned.get(type.id) === namespace.name)
          .map((type) => type.declaration),
      });
      links.set(namespace.id, `/reference/${slug}/`);
    }

    const pageOf = (child: J.DeclarationReflection) => {
      const owner = owned.get(child.id);
      return owner === undefined ? moduleSlug : namespaceSlug(owner);
    };
    for (const type of types) links.set(type.id, href(pageOf(type), type.name));
    for (const value of values) links.set(value.id, href(moduleSlug, value.name));
  }

  const slugs = new Set<string>();
  for (const page of pages) {
    if (slugs.has(page.slug)) throw new Error(`Two reference pages share the slug "${page.slug}"`);
    slugs.add(page.slug);
    const anchors = new Set<string>();
    for (const anchor of [...page.members, ...page.types].map((entry) => entry.anchor)) {
      if (anchors.has(anchor)) {
        throw new Error(`/reference/${page.slug} has two entries anchored at "${anchor}"`);
      }
      anchors.add(anchor);
    }
  }
  return { pages, links, declarations };
}

function href(slug: string, anchor: string): string {
  return `/reference/${slug === "" ? "" : `${slug}/`}#${anchor}`;
}
