import type { APIRoute } from "astro";
import { getCollection, render } from "astro:content";

import { commentText } from "../reference/comment.ts";
import { load } from "../reference/project.ts";

export type Group = "namespace" | "function" | "type" | "doc";

export type Entry = {
  readonly title: string;
  readonly kind: string;
  readonly group: Group;
  readonly href: string;
  readonly context: string;
  readonly code: boolean;
  readonly text: string;
};

function plain(markdown: string): string {
  return markdown
    .replace(/^```.*$/gm, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_#>|]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export const GET: APIRoute = async () => {
  const entries: Entry[] = [];
  for (const doc of await getCollection("docs")) {
    const href = doc.id === "index" ? "/" : `/${doc.id}/`;
    entries.push({
      title: doc.data.title,
      kind: "page",
      group: "doc",
      href,
      context: "",
      code: false,
      text: plain(doc.body ?? ""),
    });
    const { headings } = await render(doc);
    for (const heading of headings.filter((heading) => heading.depth <= 3)) {
      entries.push({
        title: heading.text,
        kind: "section",
        group: "doc",
        href: `${href}#${heading.slug}`,
        context: doc.data.title,
        code: false,
        text: "",
      });
    }
  }
  const { pages } = await load();
  for (const page of pages) {
    const href = page.slug === "" ? "/reference/" : `/reference/${page.slug}/`;
    const owner = page.binding?.name;
    entries.push({
      title: page.title,
      kind: page.binding?.kind === "named" ? "namespace" : "package",
      group: "namespace",
      href,
      context: page.binding?.kind === "named" ? page.specifier : "",
      code: true,
      text: commentText(page.comment),
    });
    for (const member of page.members) {
      entries.push({
        title: owner === undefined ? member.path : `${owner}.${member.path}`,
        kind: member.body.kind === "call" ? "function" : "property",
        group: "function",
        href: `${href}#${member.anchor}`,
        context: page.specifier,
        code: true,
        text: commentText(member.comment),
      });
    }
    for (const type of page.types) {
      entries.push({
        title: type.name,
        kind: type.keyword,
        group: "type",
        href: `${href}#${type.anchor}`,
        context: page.title,
        code: true,
        text: commentText(type.comment),
      });
    }
  }
  return new Response(JSON.stringify(entries), {
    headers: { "content-type": "application/json" },
  });
};
