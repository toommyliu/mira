import type { APIRoute } from "astro";

export const GET: APIRoute = ({ site }) => {
  if (site === undefined) throw new Error("astro.config.ts needs a site to write absolute links");
  return new Response(`Sitemap: ${new URL("/sitemap.xml", site).href}\n`, {
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
};
