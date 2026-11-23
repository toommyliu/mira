import nodes from "lucide-static/icon-nodes.json";

export type IconNode = {
  readonly tag: string;
  readonly attributes: Readonly<Record<string, string>>;
};

function isAttributes(value: unknown): value is Record<string, string> {
  return (
    typeof value === "object" &&
    value !== null &&
    Object.values(value).every((entry) => typeof entry === "string")
  );
}

export function icon(name: keyof typeof nodes): readonly IconNode[] {
  const list: readonly (readonly unknown[])[] = nodes[name];
  return list.flatMap(([tag, attributes]) =>
    typeof tag === "string" && isAttributes(attributes) ? [{ tag, attributes }] : [],
  );
}

export const svgAttributes = {
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  "stroke-width": "2",
  "stroke-linecap": "round",
  "stroke-linejoin": "round",
  "aria-hidden": "true",
} as const;

export const codeActions = [
  { kind: "copy", label: "Copy code", icons: [icon("copy"), icon("check")] },
  { kind: "wrap", label: "Wrap lines", icons: [icon("text-wrap")] },
] as const;
