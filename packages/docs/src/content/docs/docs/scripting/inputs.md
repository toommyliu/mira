---
title: Script inputs
order: 2
---

Script inputs are values a script asks you for, such as a monster name or a kill count. Mira saves what you enter in **Scripts** and passes it to every run.

## Define inputs

Export `inputs` to ask for values before a script runs. Read the values with [`Script.inputs`](/reference/script/#inputs).

```ts
import { Script, type Inputs } from "mira";

export const inputs = {
  name: { type: "string", label: "Name", required: true, default: "Frogzard" },
  count: { type: "number", label: "Count", integer: true, min: 1, max: 100, default: 5 },
  loot: { type: "boolean", label: "Pick up drops", default: true },
  mode: { type: "select", label: "Mode", options: ["Farm", "Quest"], default: "Farm" },
  items: {
    type: "select",
    label: "Items",
    options: ["Sword", "Helm"],
    multiple: true,
    default: ["Helm"],
  },
} as const satisfies Inputs;

export default function* () {
  const values = Script.inputs<typeof inputs>();
  console.log(values.name, values.count, values.loot, values.mode, values.items);
}
```

## Choose input types

Every input has a `label` and an optional `description`.

Strings accept `required` and `default`. Numbers accept `integer`, `min`, `max`, and `default`. Booleans accept `default`.

Select inputs require a nonempty `options` list. Set `multiple: true` for a list of selections and an array default. See [`Input`](/reference/script/#Input) for the types.
