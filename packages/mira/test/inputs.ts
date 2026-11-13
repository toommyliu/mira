import { Script, type Inputs } from "../src/mira.ts";

export const inputs = {
  kills: { type: "number", label: "Kills", default: 5 },
  mode: { type: "select", label: "Mode", options: ["Farm", "Quest"] },
  buy: { type: "select", label: "Buy", options: ["A", "B"], multiple: true },
} as const satisfies Inputs;

// @ts-expect-error a type that isn't one of the four
export const unknownType = { x: { type: "integer", label: "X" } } as const satisfies Inputs;

export const read = () => {
  const values = Script.inputs<typeof inputs>();
  const kills: number = values.kills;
  const modes: "Farm" | "Quest" = values.mode;
  const bought: readonly ("A" | "B")[] = values.buy;
  // @ts-expect-error not an input
  const missing = values.missing;
  // @ts-expect-error not one of the options
  const merge: "Merge" = values.mode;
  // @ts-expect-error a multiple select is a list
  const one: string = values.buy;
  // @ts-expect-error values are read-only
  values.kills = 1;
  const plain: string | number | boolean | readonly string[] | undefined =
    Script.inputs()["anything"];
  return [kills, modes, bought, missing, merge, one, plain];
};
