import { createSatteriMarkdownProcessor, satteriHighlightPlugin } from "@astrojs/markdown-satteri";
import { codeToTokens, isSupportedLanguage } from "@pierre/highlights";
import { codeActions as actions, svgAttributes } from "./icons.ts";

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

const codeActions: Content = {
  type: "element",
  tagName: "div",
  properties: { className: ["code-actions"] },
  children: actions.map((action): Content => ({
    type: "element",
    tagName: "button",
    properties: {
      className: ["icon-button"],
      type: "button",
      dataCodeAction: action.kind,
      ariaLabel: action.label,
      ...(action.kind === "wrap" ? { ariaPressed: "false" } : {}),
    },
    children: action.icons.map((nodes): Content => ({
      type: "element",
      tagName: "svg",
      properties: { ...svgAttributes },
      children: nodes.map(({ tag, attributes }): Content => ({
        type: "element",
        tagName: tag,
        properties: { ...attributes },
        children: [],
      })),
    })),
  })),
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
    codeActions,
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
