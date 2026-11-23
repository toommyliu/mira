import { type CollectionEntry, getCollection } from "astro:content";

import { commentText } from "./reference/comment.ts";
import type { Page } from "./reference/model.ts";
import { load } from "./reference/project.ts";

export type Route = {
  readonly section: "App" | "Scripting" | "Reference";
  readonly group: string | null;
  readonly kind: "page" | "package" | "namespace";
  readonly title: string;
  readonly href: string;
  readonly markdown: string;
  readonly description: string;
  readonly content:
    | { readonly kind: "doc"; readonly entry: CollectionEntry<"docs"> }
    | { readonly kind: "reference"; readonly page: Page };
};

const LISTED = 6;

export function plainText(markdown: string): string {
  return markdown
    .replace(/^```.*$/gm, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_#>|]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function groupTitle(folder: string): string {
  const words = folder.replaceAll("-", " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function markdownPath(href: string): string {
  return `${href.replace(/\/$/, "")}.md`;
}

function listed(names: readonly string[]): string {
  const shown = names.slice(0, LISTED).join(", ");
  return names.length > LISTED ? `${shown} and ${names.length - LISTED} more` : shown;
}

function referenceDescription(page: Page): string {
  const summary = commentText(page.comment);
  if (summary !== "") return summary;
  if (page.namespaces.length > 0) {
    return `The ${page.specifier} package: ${listed(page.namespaces.map((namespace) => namespace.title))}.`;
  }
  const names = [
    ...page.members.map((member) => member.path),
    ...page.types.map((type) => type.name),
  ];
  return `${page.binding?.kind === "named" ? `${page.title} in ${page.specifier}` : `The ${page.specifier} package`}: ${listed(names)}.`;
}

export async function routes(): Promise<readonly Route[]> {
  const docs = (await getCollection("docs")).toSorted(
    (a, b) => a.data.order - b.data.order || a.data.title.localeCompare(b.data.title),
  );
  const { pages } = await load();
  return [
    ...docs.map((entry): Route => {
      const href = `/${entry.id}/`;
      const folder = entry.id.split("/").slice(2, -1).at(-1);
      return {
        section: entry.id.startsWith("docs/scripting/") ? "Scripting" : "App",
        group: folder === undefined ? null : groupTitle(folder),
        kind: "page",
        title: entry.data.title,
        href,
        markdown: markdownPath(href),
        description:
          entry.data.description ?? plainText(entry.body ?? "").split(/(?<=\.)\s/)[0] ?? "",
        content: { kind: "doc", entry },
      };
    }),
    ...pages.map((page): Route => {
      const href = page.slug === "" ? "/reference/" : `/reference/${page.slug}/`;
      return {
        section: "Reference",
        group: null,
        kind: page.binding?.kind === "named" ? "namespace" : "package",
        title: page.title,
        href,
        markdown: markdownPath(href),
        description: referenceDescription(page),
        content: { kind: "reference", page },
      };
    }),
  ];
}
