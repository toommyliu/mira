import { getCollection } from "astro:content";

import { load } from "./reference/project.ts";

export type NavItem = {
  readonly title: string;
  readonly href: string;
  readonly kind: "page" | "package" | "namespace";
};
export type NavSection = { readonly title: string; readonly items: readonly NavItem[] };

export async function navigation(): Promise<readonly NavSection[]> {
  const docs = (await getCollection("docs")).toSorted(
    (a, b) => a.data.order - b.data.order || a.data.title.localeCompare(b.data.title),
  );
  const { pages } = await load();
  return [
    {
      title: "Start",
      items: docs.map((entry) => ({
        title: entry.data.title,
        href: entry.id === "index" ? "/" : `/${entry.id}/`,
        kind: "page",
      })),
    },
    {
      title: "Reference",
      items: pages.map((page) => ({
        title: page.title,
        href: page.slug === "" ? "/reference/" : `/reference/${page.slug}/`,
        kind: page.binding?.kind === "named" ? "namespace" : "package",
      })),
    },
  ];
}
