import type { APIRoute, GetStaticPaths } from "astro";

import { markdownOf } from "../mirror.ts";
import { type Route, routes } from "../routes.ts";

export const getStaticPaths = (async () =>
  (await routes()).map((route) => ({
    params: { path: route.markdown.slice(1, -".md".length) },
    props: { route },
  }))) satisfies GetStaticPaths;

export const GET: APIRoute<{ route: Route }> = async ({ props, site }) => {
  if (site === undefined) throw new Error("astro.config.ts needs a site to write absolute links");
  return new Response(await markdownOf(props.route, site), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
};
