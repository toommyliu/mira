import type { Route } from "./routes.ts";
import { commentMarkdown } from "./reference/comment.ts";
import type { Comment, Declaration, Member, Page } from "./reference/model.ts";
import { load } from "./reference/project.ts";
import {
  previewTokens,
  signatureTokens,
  type Token,
  typeTokens,
  valueTokens,
} from "./reference/tokens.ts";

const text = (tokens: readonly Token[]) => tokens.map((token) => token.text).join("");
const fence = (code: string) => `\`\`\`ts\n${code}\n\`\`\``;

function comment(value: Comment | undefined, links: ReadonlyMap<number, string>): string[] {
  const markdown = value === undefined ? "" : commentMarkdown(value, links);
  return markdown === "" ? [] : [markdown];
}

function member(entry: Member, owner: string | undefined, links: ReadonlyMap<number, string>) {
  const path = owner === undefined ? entry.path : `${owner}.${entry.path}`;
  const body = entry.body;
  const code =
    body.kind === "call"
      ? body.signatures.map((signature) => text(signatureTokens(path, signature, links))).join("\n")
      : text(valueTokens({ ...entry, path, body }, links));
  const errors =
    body.kind === "call"
      ? body.signatures.flatMap((signature) =>
          signature.result.kind === "yields" && signature.result.error !== undefined
            ? [`Fails with \`${text(typeTokens(signature.result.error, links))}\`.`]
            : [],
        )
      : [];
  return [`### ${entry.path}`, fence(code), ...comment(entry.comment, links), ...errors];
}

function fields(properties: readonly Member[], links: ReadonlyMap<number, string>): string[] {
  const described = properties.flatMap((property) => {
    const markdown = comment(property.comment, links);
    return markdown.length === 0 ? [] : [`- \`${property.path}\`: ${markdown.join(" ")}`];
  });
  return described.length === 0 ? [] : [described.join("\n")];
}

function declaration(entry: Declaration, links: ReadonlyMap<number, string>) {
  const head = [`### ${entry.name}`];
  if (entry.body.kind === "variants") {
    return [
      ...head,
      ...comment(entry.comment, links),
      ...entry.body.variants.flatMap((variant) => [
        `#### ${variant.name}`,
        fence(text(typeTokens(variant.type, links))),
        ...comment(variant.comment, links),
        ...fields(variant.properties, links),
      ]),
    ];
  }
  return [
    ...head,
    fence(text(previewTokens(entry, links))),
    ...comment(entry.comment, links),
    ...(entry.body.kind === "shape" ? fields(entry.body.properties, links) : []),
  ];
}

function reference(page: Page, links: ReadonlyMap<number, string>, site: URL): string {
  const binding =
    page.binding === undefined
      ? []
      : [
          fence(
            page.binding.kind === "named"
              ? `import { ${page.binding.name} } from ${JSON.stringify(page.specifier)};`
              : `import * as ${page.binding.name} from ${JSON.stringify(page.specifier)};`,
          ),
        ];
  const namespaces = page.namespaces.map(
    (namespace) =>
      `- [${namespace.title}](${new URL(`/reference/${namespace.slug}.md`, site).href})`,
  );
  const allCalls = page.members.every((entry) => entry.body.kind === "call");
  return [
    `# ${page.title}`,
    page.binding?.kind === "named"
      ? `Namespace in \`${page.specifier}\`.`
      : `Package \`${page.specifier}\`.`,
    ...binding,
    ...comment(page.comment, links),
    ...(namespaces.length === 0 ? [] : ["## Namespaces", namespaces.join("\n")]),
    ...(page.members.length === 0
      ? []
      : [
          allCalls ? "## Functions" : "## Members",
          ...page.members.flatMap((entry) => member(entry, page.binding?.name, links)),
        ]),
    ...(page.types.length === 0
      ? []
      : ["## Types", ...page.types.flatMap((entry) => declaration(entry, links))]),
  ].join("\n\n");
}

export async function markdownOf(route: Route, site: URL): Promise<string> {
  if (route.content.kind === "doc") {
    return `# ${route.title}\n\n${route.content.entry.body ?? ""}`.trim();
  }
  const { links } = await load();
  const absolute = new Map([...links].map(([id, href]) => [id, new URL(href, site).href]));
  return reference(route.content.page, absolute, site);
}
