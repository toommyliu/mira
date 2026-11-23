import type { APIRoute } from "astro";

import { routes } from "../routes.ts";

export const GET: APIRoute = async ({ site }) => {
  if (site === undefined) throw new Error("astro.config.ts needs a site to write absolute links");
  const urls = ["/", ...(await routes()).map((route) => route.href)].map(
    (href) => `  <url><loc>${new URL(href, site).href}</loc></url>`,
  );
  return new Response(
    `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.join("\n")}\n</urlset>\n`,
    { headers: { "content-type": "application/xml; charset=utf-8" } },
  );
};
