import { readFileSync } from "node:fs";
import { join } from "node:path";

import { manifestRepository } from "../../../scripts/release.ts";

const root = join(process.cwd(), "..", "..");
const { owner, name } = manifestRepository(readFileSync(join(root, "Cargo.toml"), "utf8"));

export const repositoryUrl = `https://github.com/${owner}/${name}`;
