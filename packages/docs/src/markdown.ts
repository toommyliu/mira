import { createSatteriMarkdownProcessor, satteriHighlightPlugin } from "@astrojs/markdown-satteri";
import { codeToTokens, isSupportedLanguage } from "@pierre/highlights";
import { codeActions as actions, svgAttributes } from "./icons.ts";

type Highlight = Parameters<typeof satteriHighlightPlugin>[0];
type Content = Extract<Awaited<ReturnType<Highlight>>, { type: "element" }>["children"][number];

const palette = { name: "mira", appearance: "light", style: {}, cssVariables: true } as const;

function line(children: Content[], text: string, last: boolean): Content {
  return {
    type: "element",
    tagName: "span",
    properties: { className: ["line"], style: `--indent: ${/^ */.exec(text)?.[0].length ?? 0}` },
    children: last ? children : [...children, { type: "text", value: "\n" }],
  };
}

function tokens(code: string, lang: string): Content[] {
  if (!isSupportedLanguage(lang)) {
    const rows = code.split("\n");
    return rows.map((row, index) =>
      line([{ type: "text", value: row }], row, index === rows.length - 1),
    );
  }
  const rows = codeToTokens(code, { lang, theme: palette }).tokens;
  return rows.map((row, index) =>
    line(
      row.map((token): Content => ({
        type: "element",
        tagName: "span",
        properties: token.color === undefined ? {} : { style: `color:${token.color}` },
        children: [{ type: "text", value: token.content }],
      })),
      row.map((token) => token.content).join(""),
      index === rows.length - 1,
    ),
  );
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
