import type { Effect } from "effect";
import { Bank, Combat, Drops, Inventory, Player, Quests, Script, TempInventory, Wait } from "mira";

export type EldersBloodResult =
  | { readonly status: "completed"; readonly quantity: number; readonly maxStack: number | null }
  | { readonly status: "maxed"; readonly quantity: number; readonly maxStack: number }
  | { readonly status: "unavailable" };

const QUEST = 802;
const ELDERS_BLOOD = { itemId: 5586 };
const SLAIN_GORILLAPHANT = { itemId: 5574 };
const MAP = "arcangrove";
const CELL = { cell: "LeftBack", pad: "Left" };
const MONSTER = "Gorillaphant";

export function* eldersBlood(): Generator<
  Effect.Effect<unknown, unknown>,
  EldersBloodResult,
  unknown
> {
  if (!(yield* Quests.load(QUEST, { silent: true }))) {
    throw new Error(`Quest ${QUEST} didn't load.`);
  }
  const quest = yield* Quests.get(QUEST);
  const need = quest?.requirements.find(({ itemId }) => itemId === SLAIN_GORILLAPHANT.itemId);
  if (quest === null || need === undefined) {
    throw new Error(`Quest ${QUEST} no longer asks for Slain Gorillaphant.`);
  }

  const held = yield* Inventory.getItem(ELDERS_BLOOD);
  const banked = held === null ? yield* Bank.getItem(ELDERS_BLOOD) : null;
  const owned = held ?? banked;
  const maxStack =
    quest.rewards.find(({ itemId }) => itemId === ELDERS_BLOOD.itemId)?.maxStack ??
    owned?.maxStack ??
    null;
  if (owned !== null && maxStack !== null && owned.quantity >= maxStack) {
    return { status: "maxed", quantity: owned.quantity, maxStack };
  }
  if (!(yield* Quests.isAvailable(QUEST)) && !(yield* Quests.isInProgress(QUEST))) {
    return { status: "unavailable" };
  }
  if (banked !== null && !(yield* Bank.withdraw(ELDERS_BLOOD))) {
    throw new Error("Couldn't take Elders' Blood out of the bank.");
  }

  if (!(yield* Player.joinMap(MAP, CELL))) {
    throw new Error(`Couldn't join ${MAP}.`);
  }
  if (!(yield* Quests.accept(QUEST))) {
    throw new Error(`Couldn't accept quest ${QUEST}.`);
  }
  while (!(yield* TempInventory.contains(SLAIN_GORILLAPHANT, need.quantity))) {
    const target = yield* Combat.hunt(MONSTER);
    if (target !== null) {
      yield* Combat.kill({ monMapId: target.monMapId });
      continue;
    }
    if ((yield* Player.getMap()) !== MAP) {
      yield* Player.joinMap(MAP, CELL);
    }
    yield* Script.sleep("1 second");
  }

  if (owned === null) {
    const drop = yield* Wait.forEvent(
      { type: "itemDrop", itemId: ELDERS_BLOOD.itemId },
      { trigger: () => Quests.complete(QUEST), timeout: "5 seconds" },
    );
    if (drop === null) {
      throw new Error(`Couldn't complete quest ${QUEST}.`);
    }
    yield* Drops.accept(ELDERS_BLOOD);
    if (!(yield* Wait.until(() => Inventory.contains(ELDERS_BLOOD), { timeout: "5 seconds" }))) {
      throw new Error("Couldn't pick up Elders' Blood.");
    }
  } else if (!(yield* Quests.complete(QUEST))) {
    throw new Error(`Couldn't complete quest ${QUEST}.`);
  }
  return {
    status: "completed",
    quantity: yield* Inventory.getQuantity(ELDERS_BLOOD),
    maxStack,
  };
}
