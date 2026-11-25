import type { APIRoute } from "astro";

import { type Route, routes } from "../routes.ts";

export const GET: APIRoute = async ({ site }) => {
  if (site === undefined) throw new Error("astro.config.ts needs a site to write absolute links");
  const all = await routes();
  const section = (name: Route["section"], title: string) => {
    const links = all
      .filter((route) => route.section === name)
      .map(
        (route) =>
          `- [${route.title}](${new URL(route.markdown, site).href}): ${route.description}`,
      );
    return links.length === 0 ? [] : [[`## ${title}`, ...links].join("\n")];
  };
  const body = [
    "# Mira",
    "> Mira is an automation client for AdventureQuest Worlds on Apple silicon Macs. These docs cover writing Mira scripts in TypeScript and the full scripting API.",
    "Each link below is the Markdown version of a page. Drop `.md` for the HTML page.",
    ...section("App", "App"),
    ...section("Scripting", "Scripting"),
    ...section("API", "API reference"),
    ...section("Packages", "Bundled script packages"),
  ].join("\n\n");
  return new Response(`${body}\n`, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
