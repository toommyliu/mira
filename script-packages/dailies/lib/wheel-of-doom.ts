import type { Effect } from "effect";
import { Bank, Inventory, Player, Quests, Wait } from "mira";
import * as S from "mira/schema";

export type Spin =
  | { readonly status: "spun"; readonly won: readonly string[]; readonly banked: boolean }
  | { readonly status: "already-spun" }
  | { readonly status: "members-only" }
  | { readonly status: "needs-gear"; readonly gears: number };

export type WheelOfDoomResult = { readonly daily: Spin; readonly weekly: Spin };

export type WheelOfDoomOptions = { readonly bankRewards?: boolean };

const DAILY = 3075;
const WEEKLY = 3076;
const GEAR_OF_DOOM = { itemId: 45739 };
const GEARS_PER_SPIN = 3;
const XP_BOOST = 19189;

const Reward = S.object({ sName: S.string().min(1) });
const DropItems = S.record(S.unknown());

const rewardNames = (data: Readonly<Record<string, unknown>>): string[] => {
  const drops = data["dropItems"];
  const boost = DropItems.is(drops) ? drops[String(XP_BOOST)] : undefined;
  const names = [boost, data["Item"]].filter(Reward.is).map(({ sName }) => sName);
  return [...new Set(names)];
};

function* spin(
  quest: number,
  bankRewards: boolean,
): Generator<Effect.Effect<unknown, unknown>, Spin, unknown> {
  if (!(yield* Quests.load(quest, { silent: true }))) {
    throw new Error(`Quest ${quest} didn't load.`);
  }
  if (!(yield* Quests.isAvailable(quest)) && !(yield* Quests.isInProgress(quest))) {
    return { status: "already-spun" };
  }
  if (!(yield* Player.joinMap("doom"))) throw new Error("Couldn't join doom.");
  if (!(yield* Quests.accept(quest))) throw new Error(`Couldn't accept quest ${quest}.`);
  if (!(yield* Quests.canComplete(quest))) throw new Error(`Can't complete quest ${quest}.`);

  const wheel = yield* Wait.forPacket(
    { from: "server", cmd: "Wheel" },
    { trigger: () => Quests.complete(quest), timeout: "5 seconds" },
  );
  if (wheel === null) throw new Error(`The wheel didn't spin for quest ${quest}.`);
  const won = wheel.format === "json" ? rewardNames(wheel.data) : [];
  if (!bankRewards) return { status: "spun", won, banked: false };

  const deposit: string[] = [];
  for (const name of won) {
    if ((yield* Inventory.getItem(name)) !== null) deposit.push(name);
    else if (!(yield* Bank.contains(name))) throw new Error(`${name} isn't in your inventory.`);
  }
  const deposited = yield* Bank.depositBatch(deposit);
  const failed = deposit.filter((_, index) => !deposited[index]);
  if (failed.length > 0) throw new Error(`Couldn't bank ${failed.join(", ")}.`);
  return { status: "spun", won, banked: true };
}

export function* wheelOfDoom({ bankRewards = false }: WheelOfDoomOptions = {}): Generator<
  Effect.Effect<unknown, unknown>,
  WheelOfDoomResult,
  unknown
> {
  const daily: Spin = (yield* Player.isMember())
    ? yield* spin(DAILY, bankRewards)
    : { status: "members-only" };

  if (
    !(yield* Inventory.contains(GEAR_OF_DOOM, GEARS_PER_SPIN)) &&
    (yield* Bank.contains(GEAR_OF_DOOM))
  ) {
    yield* Bank.withdraw(GEAR_OF_DOOM);
  }
  const weekly: Spin = (yield* Inventory.contains(GEAR_OF_DOOM, GEARS_PER_SPIN))
    ? yield* spin(WEEKLY, bankRewards)
    : { status: "needs-gear", gears: yield* Inventory.getQuantity(GEAR_OF_DOOM) };

  return { daily, weekly };
}
