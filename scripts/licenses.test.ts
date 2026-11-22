import assert from "node:assert/strict";
import { test } from "node:test";

import { crateNotices, render, vendoredNotices } from "./licenses.ts";

const registry = "registry+https://github.com/rust-lang/crates.io-index";
const regex = { name: "regex", version: "1.12.2", source: registry };
const mira = { name: "mira", version: "0.1.0", source: null };

test("lists dependencies and leaves out Mira's own crates", () => {
  const report = {
    licenses: [
      {
        name: "MIT License",
        text: "Copyright (c) Someone\r\n\r\nPermission is hereby granted\r\n",
        used_by: [{ crate: regex }, { crate: mira }],
      },
      {
        name: "SIL Open Font License 1.1",
        text: "Copyright Mira's own font\n",
        used_by: [{ crate: mira }],
      },
    ],
    crates: [{ package: regex }, { package: mira }],
  };
  assert.equal(
    render(crateNotices(report)),
    `# Third-party licenses

Mira includes the following third-party software.

## MIT License

- regex 1.12.2

\`\`\`
Copyright (c) Someone

Permission is hereby granted
\`\`\`
`,
  );
  const unlicensed = { name: "unlicensed", version: "0.1.0", source: registry };
  assert.throws(
    () => crateNotices({ ...report, crates: [...report.crates, { package: unlicensed }] }),
    /no license for unlicensed 0\.1\.0/,
  );
});

test("keeps the copyright comments Effect vendors", () => {
  const source = `/**
 * Vendored from \`@noble/ciphers\`.
 *
 * Copyright (c) 2022 Paul Miller
 */
/** Encrypts a message. */
const a = 1
/*////////
This is a derivative work copyright (c) 2018, William P. "Mac" McMeans.
////////*/`;
  assert.deepEqual(vendoredNotices("effect/dist/Random.js", source), [
    {
      license: "Notice vendored in effect",
      users: ["effect/dist/Random.js"],
      text: "Vendored from `@noble/ciphers`.\n\nCopyright (c) 2022 Paul Miller",
    },
    {
      license: "Notice vendored in effect",
      users: ["effect/dist/Random.js"],
      text: 'This is a derivative work copyright (c) 2018, William P. "Mac" McMeans.',
    },
  ]);
});
