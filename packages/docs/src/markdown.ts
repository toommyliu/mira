import { createSatteriMarkdownProcessor, satteriHighlightPlugin } from "@astrojs/markdown-satteri";
import { codeToTokens, isSupportedLanguage } from "@pierre/highlights";
import icons from "lucide-static/icon-nodes.json";

type Highlight = Parameters<typeof satteriHighlightPlugin>[0];
type Content = Extract<Awaited<ReturnType<Highlight>>, { type: "element" }>["children"][number];

const palette = { name: "mira", appearance: "light", style: {}, cssVariables: true } as const;

function tokens(code: string, lang: string): Content[] {
  if (!isSupportedLanguage(lang)) return [{ type: "text", value: code }];
  return codeToTokens(code, { lang, theme: palette }).tokens.flatMap((line, index) => [
    ...(index === 0 ? [] : [{ type: "text", value: "\n" } as const]),
    ...line.map((token): Content => ({
      type: "element",
      tagName: "span",
      properties: token.color === undefined ? {} : { style: `color:${token.color}` },
      children: [{ type: "text", value: token.content }],
    })),
  ]);
}

export const wrapIcon = icons["text-wrap"].flatMap(([tag, attributes]) =>
  tag === "path" && typeof attributes === "object" ? [attributes.d] : [],
);

const wrapButton: Content = {
  type: "element",
  tagName: "button",
  properties: {
    className: ["icon-button", "code-wrap"],
    type: "button",
    dataCodeWrap: "",
    ariaPressed: "false",
    ariaLabel: "Wrap lines",
  },
  children: [
    {
      type: "element",
      tagName: "svg",
      properties: {
        viewBox: "0 0 24 24",
        fill: "none",
        stroke: "currentColor",
        strokeWidth: "2",
        strokeLinecap: "round",
        strokeLinejoin: "round",
        ariaHidden: "true",
      },
      children: wrapIcon.map((d): Content => ({
        type: "element",
        tagName: "path",
        properties: { d },
        children: [],
      })),
    },
  ],
};

export const highlight: Highlight = async (code, lang) => ({
  type: "element",
  tagName: "div",
  properties: { className: ["code-block"] },
  children: [
    {
      type: "element",
      tagName: "pre",
      properties: { className: ["code"], dataLanguage: lang },
      children: [
        { type: "element", tagName: "code", properties: {}, children: tokens(code, lang) },
      ],
    },
    wrapButton,
  ],
});

export const highlightPlugin = satteriHighlightPlugin(highlight, []);

export const features = { smartPunctuation: false };

const comments = createSatteriMarkdownProcessor({
  syntaxHighlight: false,
  hastPlugins: [highlightPlugin],
  features,
});

export async function renderMarkdown(source: string): Promise<string> {
  return (await (await comments).render(source)).code;
}
