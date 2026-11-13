import { Script, type Inputs } from "mira";

export const inputs = {
  name: {
    type: "string",
    label: "Name",
    description: "Required text.",
    required: true,
    default: "Frogzard",
  },
  note: { type: "string", label: "Note", description: "Optional text that starts empty." },
  count: {
    type: "number",
    label: "Count",
    description: "A whole number from 1 to 100.",
    integer: true,
    min: 1,
    max: 100,
    default: 5,
  },
  interval: {
    type: "number",
    label: "Interval (minutes)",
    description: "Fractions are fine.",
    min: 0.1,
    default: 0.5,
  },
  offset: {
    type: "number",
    label: "Offset",
    description: "No default, so it has to be filled in.",
  },
  loot: { type: "boolean", label: "Pick up drops", default: true },
  quiet: { type: "boolean", label: "Quiet", description: "Starts off." },
  mode: { type: "select", label: "Mode", options: ["Farm", "Quest", "Merge"], default: "Farm" },
  room: {
    type: "select",
    label: "Room",
    description: "No default, so it has to be chosen.",
    options: ["Public", "Private"],
  },
  buy: {
    type: "select",
    label: "Buy",
    options: ["Sword", "Helm", "Cape"],
    multiple: true,
    default: ["Helm"],
  },
} as const satisfies Inputs;

export default function* () {
  const values = Script.inputs<typeof inputs>();
  for (const [key, value] of Object.entries(values)) {
    console.log(
      `${key} = ${JSON.stringify(value)} (${Array.isArray(value) ? "list" : typeof value})`,
    );
  }
  const buying: readonly ("Sword" | "Helm" | "Cape")[] = values.buy;
  const mode: "Farm" | "Quest" | "Merge" = values.mode;
  console.log(
    `${mode}: buying ${buying.length === 0 ? "nothing" : buying.join(", ")} every ${values.interval * 60} seconds`,
  );
}
