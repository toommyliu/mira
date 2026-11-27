import { fileURLToPath } from "node:url";

import mdx from "@astrojs/mdx";
import { satteri } from "@astrojs/markdown-satteri";
import { defineConfig } from "astro/config";

import { features, highlightPlugin } from "./src/markdown.ts";

const sources = [
  fileURLToPath(new URL("../mira/src/", import.meta.url)),
  fileURLToPath(new URL("../../script-packages/", import.meta.url)),
];
const project = fileURLToPath(new URL("src/reference/project.ts", import.meta.url));

export default defineConfig({
  site: "https://usemira.pages.dev",
  server: { allowedHosts: [".ts.net"] },
  markdown: {
    syntaxHighlight: false,
    processor: satteri({ hastPlugins: [highlightPlugin], features }),
  },
  integrations: [mdx()],
  vite: {
    plugins: [
      {
        name: "mira:reload-reference",
        configureServer(server) {
          server.watcher.add(sources);
          server.watcher.on("all", (_event, file) => {
            if (sources.some((source) => file.startsWith(source))) {
              server.watcher.emit("change", project);
            }
          });
        },
      },
    ],
  },
});
