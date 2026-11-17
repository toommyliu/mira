import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightLinksValidator from "starlight-links-validator";
import starlightTypeDoc, { typeDocSidebarGroup } from "starlight-typedoc";

export default defineConfig({
  site: "https://usemira.pages.dev",
  integrations: [
    starlight({
      title: "Mira",
      plugins: [
        starlightLinksValidator(),
        starlightTypeDoc({
          entryPoints: ["../mira/src/mira.ts", "../mira/src/http.ts", "../mira/src/schema.ts"],
          tsconfig: "../mira/tsconfig.json",
          typeDoc: { name: "API reference", disableSources: true, entryFileName: "index" },
          sidebar: { label: "API reference", collapsed: true },
        }),
      ],
      sidebar: [{ label: "Start here", items: ["index"] }, typeDocSidebarGroup],
    }),
  ],
});
