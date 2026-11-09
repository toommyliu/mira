import { Inventory, Player, type EnhancementSelector, type EnhancementSlot, type Item } from "lyra";

type Known = {
  readonly slot: EnhancementSlot;
  readonly label: string;
  readonly selector: EnhancementSelector | null;
  readonly patternId: number;
  readonly procId: number;
};

const SLOTS: Readonly<Record<string, EnhancementSlot>> = {
  Weapon: "weapon",
  ar: "class",
  he: "helm",
  ba: "cape",
};

const BASICS = [
  ["Fighter", 2, true],
  ["Thief", 3, true],
  ["Hybrid", 5, true],
  ["Wizard", 6, true],
  ["Healer", 7, true],
  ["Spellbreaker", 8, false],
  ["Lucky", 9, true],
] as const;

const AWE_TRAITS = [
  ["Spiral Carve", 2],
  ["Awe Blast", 3],
  ["Health Vamp", 4],
  ["Mana Vamp", 5],
  ["Powerword DIE", 6],
] as const;

const FORGE_WEAPONS = [
  ["Forge", "Lacerate", 10, 7],
  ["Forge", "Smite", 10, 8],
  ["Forge", "Valiance", 10, 9],
  ["Forge", "Arcana's Concerto", 10, 10],
  ["Depths", "Acheron", 23, 11],
  ["Wizard", "Elysium", 6, 12],
  ["Forge", "Praxis", 10, 13],
  ["Fighter", "Dauntless", 2, 14],
  ["Forge", "Ravenous", 10, 15],
] as const;

const FORGE_HELMS = [
  ["Vim", "Ether", 25],
  ["Examen", "Ether", 26],
  ["Pneuma", "Clairvoyance", 27],
  ["Anima", "Clairvoyance", 28],
  ["Grimskull", "Hearty", 32],
] as const;

const FORGE_CAPES = [
  ["Absolution", 11],
  ["Avarice", 12],
  ["Vainglory", 24],
  ["Penitence", 29],
  ["Lament", 30],
] as const;

const UNSOLD = [
  ["Adventurer", 1],
  ["Armsman", 4],
] as const;

const known = (
  slot: EnhancementSlot,
  name: string,
  special: string | null,
  patternId: number,
  procId = 0,
): Known => ({
  slot,
  label: special === null ? name : `${name}, ${special}`,
  selector:
    special === null
      ? { enhancement: name.toLowerCase() }
      : { enhancement: name.toLowerCase(), special: special.toLowerCase() },
  patternId,
  procId,
});

const KNOWN: readonly Known[] = [
  ...(["weapon", "class", "helm", "cape"] as const).flatMap((slot) =>
    BASICS.map(([name, patternId]) => known(slot, name, null, patternId)),
  ),
  ...BASICS.filter(([, , awe]) => awe).flatMap(([name, patternId]) =>
    AWE_TRAITS.map(([trait, procId]) => known("weapon", name, trait, patternId, procId)),
  ),
  known("weapon", "Forge", null, 10),
  ...FORGE_WEAPONS.map(([name, special, patternId, procId]) =>
    known("weapon", name, special, patternId, procId),
  ),
  known("helm", "Forge", null, 10),
  ...FORGE_HELMS.map(([name, special, patternId]) => known("helm", name, special, patternId)),
  known("cape", "Forge", null, 10),
  ...FORGE_CAPES.map(([special, patternId]) => known("cape", "Forge", special, patternId)),
  ...(["weapon", "class", "helm", "cape"] as const).flatMap((slot) =>
    UNSOLD.map(([label, patternId]) => ({ slot, label, selector: null, patternId, procId: 0 })),
  ),
];

const describe = (selector: EnhancementSelector | null) =>
  selector === null
    ? "(no shop sells it)"
    : selector.special === undefined
      ? `{ enhancement: "${selector.enhancement}" }`
      : `{ enhancement: "${selector.enhancement}", special: "${selector.special}" }`;

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in first.");
    return;
  }

  const matches = new Map<Known, Item[]>();
  const unknown: Item[] = [];
  let unenhanced = 0;
  for (const item of yield* Inventory.getItems()) {
    const slot = SLOTS[item.slot];
    if (slot === undefined) continue;
    const enhancement = item.enhancement;
    if (enhancement === null) {
      unenhanced++;
      continue;
    }
    const patternId = enhancement.patternId ?? 0;
    const procId = enhancement.procId ?? 0;
    const match = KNOWN.find(
      (entry) => entry.slot === slot && entry.patternId === patternId && entry.procId === procId,
    );
    if (match === undefined) unknown.push(item);
    else matches.set(match, [...(matches.get(match) ?? []), item]);
  }

  for (const slot of ["weapon", "class", "helm", "cape"] as const) {
    console.log(`== ${slot} ==`);
    const missing: string[] = [];
    for (const entry of KNOWN.filter((entry) => entry.slot === slot)) {
      const items = [...(matches.get(entry) ?? [])].sort(
        (left, right) => (right.enhancement?.level ?? 0) - (left.enhancement?.level ?? 0),
      );
      if (items.length === 0) {
        if (entry.selector !== null) missing.push(entry.label);
        continue;
      }
      const list = items
        .map(
          (item, index) =>
            `${item.name} (lvl ${item.enhancement?.level ?? 0})${index === 0 && entry.selector !== null ? " *" : ""}`,
        )
        .join(", ");
      console.log(`${entry.label} ${describe(entry.selector)}: ${list}`);
    }
    if (missing.length > 0) console.log(`missing: ${missing.join(", ")}`);
  }

  for (const item of unknown) {
    console.log(
      `unrecognized: ${item.name} [${item.slot}] pattern ${item.enhancement?.patternId ?? 0}, proc ${item.enhancement?.procId ?? 0}, lvl ${item.enhancement?.level ?? 0}`,
    );
  }
  console.log(`${unenhanced} weapon, class, helm, or cape items have no enhancement.`);
  console.log("* is the item Inventory.equipByEnhancement picks for that selector with that slot.");
}
