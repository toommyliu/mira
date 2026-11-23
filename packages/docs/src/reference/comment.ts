import type { JSONOutput as J } from "typedoc";

import type { Comment } from "./model.ts";

type Links = ReadonlyMap<number, string>;

function text(parts: readonly J.CommentDisplayPart[], links: Links): string {
  return parts
    .map((part) => {
      if (part.kind !== "inline-tag") return part.text;
      const href = typeof part.target === "number" ? links.get(part.target) : undefined;
      return href === undefined ? `\`${part.text}\`` : `[\`${part.text}\`](${href})`;
    })
    .join("");
}

function inline(markdown: string): string {
  const fenced = /^```\w*\n([^\n]*)\n```$/.exec(markdown.trim());
  return fenced?.[1] === undefined ? markdown : `\`${fenced[1]}\``;
}

export function commentMarkdown(comment: Comment, links: Links): string {
  const blocks = [text(comment.summary, links)];
  for (const tag of comment.blockTags ?? []) {
    const content = text(tag.content, links);
    if (tag.tag === "@example") blocks.push(content);
    if (tag.tag === "@defaultValue" || tag.tag === "@default")
      blocks.push(`Default: ${inline(content)}`);
    if (tag.tag === "@deprecated") blocks.push(`**Deprecated.** ${content}`);
  }
  return blocks.filter((block) => block.trim() !== "").join("\n\n");
}
