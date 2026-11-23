import type { APIRoute } from "astro";

import { routes } from "../routes.ts";

export const GET: APIRoute = async ({ site }) => {
  if (site === undefined) throw new Error("astro.config.ts needs a site to write absolute links");
  const all = await routes();
  const section = (name: string, title: string) =>
    [
      `## ${title}`,
      ...all
        .filter((route) => route.section === name)
        .map(
          (route) =>
            `- [${route.title}](${new URL(route.markdown, site).href}): ${route.description}`,
        ),
    ].join("\n");
  const body = [
    "# Mira",
    "> Mira is an automation client for AdventureQuest Worlds on Apple silicon Macs. These docs cover writing Mira scripts in TypeScript and the full scripting API.",
    "Each link below is the Markdown version of a page. Drop `.md` for the HTML page.",
    section("App", "App"),
    section("Scripting", "Scripting"),
    section("Reference", "API reference"),
  ].join("\n\n");
  return new Response(`${body}\n`, { headers: { "content-type": "text/plain; charset=utf-8" } });
};
