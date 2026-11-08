import type * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";

import { send } from "./native.ts";

const call = <A>(request: object): Effect.Effect<A> =>
  Effect.callback<A>((resume) => {
    const cancel = send(request, (settled) => {
      resume(
        "ok" in settled ? Effect.succeed(settled.ok as A) : Effect.die(new Error(settled.error)),
      );
    });
    return Effect.sync(cancel);
  });

const player = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "player", op, ...args });

const world = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "world", op, ...args });

const combat = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "combat", op, ...args });

const inventory = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "inventory", op, ...args });

const tempInventory = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "tempInventory", op, ...args });

const bank = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "bank", op, ...args });

const quests = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "quests", op, ...args });

const drops = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "drops", op, ...args });

const shops = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "shops", op, ...args });

export type State = "dead" | "idle" | "combat";
export type Position = { readonly x: number; readonly y: number };
export type Faction = {
  readonly id: number;
  readonly name: string;
  readonly rank: number;
  readonly rep: number;
};
export type Boost = "gold" | "xp" | "rep" | "classPoints";
export type CellOptions = { readonly cell?: string; readonly pad?: string };
export type PadOptions = { readonly pad?: string };
export type WalkOptions = { readonly speed?: number };
export type RestOptions = { readonly untilFull?: boolean };
export type OutfitOptions = { readonly keepColors?: boolean };

export const Player = {
  isReady: () => player<boolean>("ready"),
  getMap: () => player<string | null>("map"),
  getCell: () => player<string | null>("cell"),
  getPad: () => player<string | null>("pad"),
  getPosition: () => player<Position | null>("position"),
  getHp: () => player<number>("hp"),
  getMaxHp: () => player<number>("maxHp"),
  getMp: () => player<number>("mp"),
  getMaxMp: () => player<number>("maxMp"),
  getState: () => player<State | null>("state"),
  isAlive: () => player<boolean>("alive"),
  getLevel: () => player<number>("level"),
  getGold: () => player<number>("gold"),
  getClassName: () => player<string | null>("className"),
  getUserId: () => player<number>("userId"),
  getCharId: () => player<number>("charId"),
  isMember: () => player<boolean>("member"),
  isAfk: () => player<boolean>("afk"),
  getFactions: () => player<readonly Faction[]>("factions"),
  hasBoost: (kind: Boost) => player<boolean>("hasBoost", { kind }),
  joinMap: (map: string, options: CellOptions = {}) =>
    player<boolean>("joinMap", { map, ...options }),
  jumpToCell: (cell: string, options: PadOptions = {}) =>
    player<boolean>("jumpToCell", { cell, ...options }),
  walkTo: (x: number, y: number, options: WalkOptions = {}) =>
    player<boolean>("walkTo", { x, y, ...options }),
  rest: (options: RestOptions = {}) => player<boolean>("rest", options),
  outfits: {
    getNames: () => player<readonly string[]>("outfits"),
    equip: (name: string, options: OutfitOptions = {}) =>
      player<boolean>("equipOutfit", { name, ...options }),
    wear: (name: string, options: OutfitOptions = {}) =>
      player<boolean>("wearOutfit", { name, ...options }),
  },
};

export type MonsterQuery = string | { readonly monMapId: number };
export type Monster = {
  readonly monMapId: number;
  readonly monsterId: number;
  readonly name: string;
  readonly race: string;
  readonly cell: string;
  readonly hp: number;
  readonly maxHp: number;
  readonly level: number;
  readonly state: State | null;
};
export type Drop = {
  readonly itemId: number;
  readonly name: string;
  readonly quantity: number;
  readonly rate: number | null;
  readonly boostedRate: number | null;
  readonly eventDrop: boolean;
  readonly questGated: boolean;
  readonly requiredQuestIds: readonly number[];
};
export type MonsterOptions = { readonly cell?: string };

export const World = {
  getAreaId: () => world<number>("areaId"),
  getRoomNumber: () => world<number | null>("roomNumber"),
  getCells: () => world<readonly string[]>("cells"),
  getCellPads: () => world<readonly string[]>("cellPads"),
  getMonsters: (options: MonsterOptions = {}) => world<readonly Monster[]>("monsters", options),
  getMonster: (query: MonsterQuery) => world<Monster | null>("monster", { query }),
  isMonsterAvailable: (query: MonsterQuery) => world<boolean>("monsterAvailable", { query }),
  getMonsterDrops: (query: MonsterQuery) =>
    world<readonly Drop[] | null>("monsterDrops", { query }),
  setSpawnPoint: (options: CellOptions = {}) => world<boolean>("setSpawnPoint", options),
  getMapItem: (itemId: number) => world<boolean>("mapItem", { itemId }),
  loadSwf: (swf: string) => world<boolean>("loadSwf", { swf }),
  reload: () => world<boolean>("reload"),
};

export type Skill = 0 | 1 | 2 | 3 | 4 | 5;
export type PlayerTarget = {
  readonly name: string;
  readonly username: string;
  readonly cell: string;
  readonly pad: string;
  readonly hp: number;
  readonly maxHp: number;
  readonly mp: number;
  readonly maxMp: number;
  readonly level: number;
  readonly state: State | null;
  readonly afk: boolean;
};
export type Target =
  | ({ readonly type: "monster" } & Monster)
  | ({ readonly type: "player" } & PlayerTarget);
export type SkillOptions = {
  readonly target?: MonsterQuery;
  readonly force?: boolean;
  readonly waitUntilReady?: boolean;
};

export const Combat = {
  getTarget: () => combat<Target | null>("target"),
  hasTarget: () => combat<boolean>("hasTarget"),
  attack: (query: MonsterQuery) => combat<boolean>("attack", { query }),
  useSkill: (skill: Skill, options: SkillOptions = {}) =>
    combat<boolean>("useSkill", { skill, ...options }),
  canUseSkill: (skill: Skill) => combat<boolean>("canUseSkill", { skill }),
  /** Milliseconds until the skill and the global cooldown are both ready. */
  getSkillCooldown: (skill: Skill) => combat<number>("skillCooldown", { skill }),
  cancelTarget: () => combat<void>("cancelTarget"),
  cancelAutoAttack: () => combat<void>("cancelAutoAttack"),
  kill: (query: MonsterQuery) => combat<boolean>("kill", { query }),
};

export type ItemQuery = string | { readonly itemId: number };
export type InventoryPool = "bag" | "misc";
export type ItemPool = InventoryPool | "class" | "house";
export type Enhancement = {
  readonly id: number;
  readonly level: number;
  readonly patternId: number | null;
  readonly procId: number | null;
  readonly dps: number | null;
  readonly range: number | null;
  readonly rarity: number | null;
};
export type Item = {
  readonly itemId: number;
  readonly charItemId: number | null;
  readonly name: string;
  readonly description: string;
  readonly quantity: number;
  readonly maxStack: number;
  readonly category: string;
  readonly slot: string;
  readonly link: string;
  readonly meta: string;
  readonly file: string;
  readonly equipped: boolean;
  readonly wearable: boolean;
  readonly worn: boolean;
  readonly member: boolean;
  readonly coins: boolean;
  readonly pool: ItemPool;
  readonly classRank: number | null;
  readonly enhancement: Enhancement | null;
};
export type TempItem = {
  readonly itemId: number;
  readonly name: string;
  readonly quantity: number;
  readonly maxStack: number;
};
export type EquipOptions = { readonly wear?: boolean };

export const Inventory = {
  getItems: () => inventory<readonly Item[]>("items"),
  getItem: (query: ItemQuery) => inventory<Item | null>("item", { query }),
  getQuantity: (query: ItemQuery) => inventory<number>("quantity", { query }),
  contains: (query: ItemQuery, quantity?: number) =>
    inventory<boolean>("contains", { query, quantity }),
  isEquipped: (query: ItemQuery) => inventory<boolean>("equipped", { query }),
  /** Each pool has this many slots. */
  getSlots: () => inventory<number>("slots"),
  getUsedSlots: (pool: InventoryPool = "bag") => inventory<number>("usedSlots", { pool }),
  getAvailableSlots: (pool: InventoryPool = "bag") => inventory<number>("availableSlots", { pool }),
  equip: (query: ItemQuery, options: EquipOptions = {}) =>
    inventory<boolean>("equip", { query, ...options }),
  unequip: (query: ItemQuery) => inventory<boolean>("unequip", { query }),
  wear: (query: ItemQuery) => inventory<boolean>("wear", { query }),
  use: (query: ItemQuery) => inventory<boolean>("use", { query }),
};

export const TempInventory = {
  getItems: () => tempInventory<readonly TempItem[]>("items"),
  getItem: (query: ItemQuery) => tempInventory<TempItem | null>("item", { query }),
  getQuantity: (query: ItemQuery) => tempInventory<number>("quantity", { query }),
  contains: (query: ItemQuery, quantity?: number) =>
    tempInventory<boolean>("contains", { query, quantity }),
};

export const Bank = {
  /** Reads load the bank on first use too. Returns false if it doesn't load. */
  load: () => bank<boolean>("load"),
  getItems: () => bank<readonly Item[]>("items"),
  getItem: (query: ItemQuery) => bank<Item | null>("item", { query }),
  getQuantity: (query: ItemQuery) => bank<number>("quantity", { query }),
  contains: (query: ItemQuery, quantity?: number) => bank<boolean>("contains", { query, quantity }),
  /** AC items don't take bank slots. */
  getSlots: () => bank<number>("slots"),
  getUsedSlots: () => bank<number>("usedSlots"),
  getAvailableSlots: () => bank<number>("availableSlots"),
  isOpen: () => bank<boolean>("isOpen"),
  open: () => bank<boolean>("open"),
  deposit: (query: ItemQuery) => bank<boolean>("deposit", { query }),
  withdraw: (query: ItemQuery) => bank<boolean>("withdraw", { query }),
  swap: (inventoryQuery: ItemQuery, bankQuery: ItemQuery) =>
    bank<boolean>("swap", { inventoryQuery, bankQuery }),
};

export type QuestCadence = "none" | "daily" | "weekly" | "monthly";
export type ItemCount = {
  readonly itemId: number;
  readonly name: string;
  readonly quantity: number;
};
/** `choice` rewards need `rewardItemId` when completing. `random` gives one of its group. */
export type QuestRewardKind = "guaranteed" | "choice" | "roll" | "random";
export type QuestReward = ItemCount & {
  readonly kind: QuestRewardKind;
  /** Percent. */
  readonly dropChance: number;
};
export type Quest = {
  readonly id: number;
  readonly name: string;
  readonly cadence: QuestCadence;
  readonly once: boolean;
  readonly requirements: readonly ItemCount[];
  readonly rewards: readonly QuestReward[];
};
/** Quests load through the quest log unless `silent`. */
export type QuestLoadOptions = { readonly silent?: boolean };
export type CompleteOptions = { readonly turnIns?: number; readonly rewardItemId?: number };

export const Quests = {
  load: (questId: number, options: QuestLoadOptions = {}) =>
    quests<boolean>("load", { questId, ...options }),
  /** One result per quest, in order. Missing quests load together, up to 30 per request. */
  loadBatch: (questIds: readonly number[], options: QuestLoadOptions = {}) =>
    quests<readonly boolean[]>("loadBatch", { questIds, ...options }),
  /** Reads see only loaded quests. */
  get: (questId: number) => quests<Quest | null>("get", { questId }),
  getAccepted: () => quests<readonly Quest[]>("accepted"),
  isInProgress: (questId: number) => quests<boolean>("inProgress", { questId }),
  isAvailable: (questId: number) => quests<boolean>("available", { questId }),
  canComplete: (questId: number) => quests<boolean>("canComplete", { questId }),
  getMaxTurnIns: (questId: number) => quests<number>("maxTurnIns", { questId }),
  accept: (questId: number, options: QuestLoadOptions = {}) =>
    quests<boolean>("accept", { questId, ...options }),
  /** Loads the quests together, then accepts each in turn. One result per quest, in order. */
  acceptBatch: (questIds: readonly number[], options: QuestLoadOptions = {}) =>
    quests<readonly boolean[]>("acceptBatch", { questIds, ...options }),
  /** Item rewards land in the drop list. */
  complete: (questId: number, options: CompleteOptions = {}) =>
    quests<boolean>("complete", { questId, ...options }),
  abandon: (questId: number) => quests<boolean>("abandon", { questId }),
};

/** Items waiting in the drop list, with `quantity` as the amount waiting. */
export const Drops = {
  getItems: () => drops<readonly Item[]>("items"),
  getItem: (query: ItemQuery) => drops<Item | null>("item", { query }),
  contains: (query: ItemQuery, quantity?: number) =>
    drops<boolean>("contains", { query, quantity }),
  /** Also claims a drop the drop list lost or hides, if the client has seen the item. */
  accept: (query: ItemQuery) => drops<boolean>("accept", { query }),
  reject: (query: ItemQuery) => drops<boolean>("reject", { query }),
};

export type ShopItemQuery = ItemQuery | { readonly shopItemId: number };
export type ShopItem = {
  readonly shopItemId: number;
  readonly itemId: number;
  readonly name: string;
  readonly description: string;
  readonly category: string;
  /** How many one purchase gives. */
  readonly quantity: number;
  readonly maxStack: number;
  readonly cost: number;
  /** `cost` is in ACs rather than gold. */
  readonly coins: boolean;
  readonly member: boolean;
  readonly level: number;
  /** Reputation needed with a faction, in the same units as `Faction.rep`. */
  readonly faction: { readonly id: number; readonly name: string; readonly rep: number } | null;
  /** Items a merge shop takes for each purchase. */
  readonly requirements: readonly ItemCount[];
};
export type Shop = {
  readonly id: number;
  readonly name: string;
  readonly merge: boolean;
  readonly limited: boolean;
  readonly house: boolean;
  readonly items: readonly ShopItem[];
};
export type QuantityOptions = { readonly quantity?: number };

export const Shops = {
  load: (shopId: number) => shops<boolean>("load", { shopId }),
  getCurrent: () => shops<Shop | null>("current"),
  getItems: () => shops<readonly ShopItem[]>("items"),
  getItem: (query: ShopItemQuery) => shops<ShopItem | null>("item", { query }),
  isOpen: (shopId?: number) => shops<boolean>("isOpen", { shopId }),
  close: (shopId?: number) => shops<boolean>("close", { shopId }),
  canBuy: (query: ShopItemQuery, options: QuantityOptions = {}) =>
    shops<boolean>("canBuy", { query, ...options }),
  getMaxBuyQuantity: (query: ShopItemQuery) => shops<number>("maxBuyQuantity", { query }),
  buy: (query: ShopItemQuery, options: QuantityOptions = {}) =>
    shops<boolean>("buy", { query, ...options }),
  /** Sells the whole stack unless `quantity` is given. */
  sell: (query: ItemQuery, options: QuantityOptions = {}) =>
    shops<boolean>("sell", { query, ...options }),
};

export const Script = {
  sleep: (duration: Duration.Input) => Effect.sleep(duration),
};
