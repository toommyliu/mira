import mdx from "@astrojs/mdx";
import { satteri } from "@astrojs/markdown-satteri";
import { defineConfig } from "astro/config";

import { features, highlightPlugin } from "./src/markdown.ts";

export default defineConfig({
  site: "https://usemira.pages.dev",
  server: { allowedHosts: [".ts.net"] },
  markdown: {
    syntaxHighlight: false,
    processor: satteri({ hastPlugins: [highlightPlugin], features }),
  },
  integrations: [mdx()],
});
