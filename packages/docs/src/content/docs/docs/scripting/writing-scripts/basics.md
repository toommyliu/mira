---
title: Basics
order: 1
---

A script is a TypeScript or JavaScript file that controls a game tab with calls from the `mira` module. Save it in `~/Mira/scripts` as a `.ts`, `.mts`, `.js`, or `.mjs` file to list it in **Scripts**. Scripts are ES modules, so use `import` and `export`, not `require`.

Mira runs the script's default export, a generator function. Put `yield*` before a call to wait for its result. A few calls, such as `Script.inputs`, return their value directly and don't take `yield*`.

```ts
import { Player } from "mira";

export default function* () {
  const map = yield* Player.getMap();
  console.log(map);
}
```

## Set up your editor

Mira adds a `tsconfig.json` to `~/Mira/scripts`, each folder you add in **Scripts**, and each installed package. Editors such as VS Code and Zed read it to find the types for `mira`, `effect`, and installed packages. Mira keeps the types in sync with its version and your packages.

If a folder already has a `tsconfig.json`, Mira leaves it alone. To use Mira's types, add `extends` to it:

```json
{
  "extends": "/Users/you/Library/Application Support/Mira/types/tsconfig.json"
}
```

## Name the script

Export `meta` to give the script a name and description. Mira shows the name in place of the file name everywhere it lists the script, such as the game bar and **Manager**. **Scripts** shows the description under the name. Write both values as string literals, because Mira reads them without running the script.

```ts
import type { Meta } from "mira";

export const meta = {
  name: "Hunt",
  description: "Kills a monster until you reach the kill count.",
} satisfies Meta;
```

## Run the script

Select the script in **Scripts**, then click **Run** or **Add to queue**.

## Read script output

The **Log** page in **Scripts** shows what scripts on the selected tab print with `console.log`, and when each run starts and ends. If a run fails, the log shows the error.

## Next steps

- Look up calls in the [scripting API reference](/reference/).
- Define [script inputs](/docs/scripting/inputs/) to pass values into a script from **Scripts**.
- Share scripts and the code they have in common as [script packages](/docs/scripting/packages/).
- Read [Advanced](/docs/scripting/writing-scripts/advanced/) for cleanup, missing APIs, and limits.
