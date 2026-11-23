import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { manifestRepository } from "../../../scripts/release.ts";

const root = join(process.cwd(), "..", "..");
const { owner, name } = manifestRepository(readFileSync(join(root, "Cargo.toml"), "utf8"));

export const repositoryUrl = `https://github.com/${owner}/${name}`;

function revision(): string {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  } catch {
    return "main";
  }
}

const sourceRevision = revision();

export function sourceUrl(source: { readonly file: string; readonly line: number }): string {
  return `${repositoryUrl}/blob/${sourceRevision}/${source.file}#L${source.line}`;
}
