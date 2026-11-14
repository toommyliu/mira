import * as Cause from "effect/Cause";
import * as Deferred from "effect/Deferred";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Fiber from "effect/Fiber";
import * as Option from "effect/Option";
import type * as Scope from "effect/Scope";

import { inputs } from "./inputs.ts";
import { exited, exiting, failed, stopping } from "./run.ts";
import { ask, call } from "./call.ts";

declare const console: { readonly warn: (message: string) => void };

const army = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "army", op, ...args });

const auth = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "auth", op, ...args });

const autoRelogin = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "autoRelogin", op, ...args });

const player = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "player", op, ...args });

const players = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "players", op, ...args });

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

const house = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "house", op, ...args });

const quests = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "quests", op, ...args });

const drops = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "drops", op, ...args });

const questRegistry = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "questRegistry", op, ...args });

const dropRegistry = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "dropRegistry", op, ...args });

const boostRegistry = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "boostRegistry", op, ...args });

const shops = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "shops", op, ...args });

const packets = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "packets", op, ...args });

const events = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "events", op, ...args });

const settings = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "settings", op, ...args });

const feeds = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "feeds", op, ...args });

const millis = (duration: Duration.Input | undefined): number | undefined =>
  duration === undefined ? undefined : Math.ceil(Duration.toMillis(duration));

const wireStep = <S extends PriorityStep>(step: S) => ({
  ...step,
  pauseAfter: millis(step.pauseAfter),
});

const wireProfile = (profile: Profile) => ({
  ...profile,
  triggers: profile.triggers?.map((trigger) => ({ ...trigger, within: millis(trigger.within) })),
  priority: profile.priority?.map(wireStep),
  steps: profile.steps.map(wireStep),
});

export type Server = {
  readonly name: string;
  readonly players: number;
  readonly maxPlayers: number;
  readonly online: boolean;
  readonly membersOnly: boolean;
};

export type ConnectResult =
  | { readonly ok: true }
  | {
      readonly ok: false;
      readonly reason:
        | "full"
        | "offline"
        | "notFound"
        | "membersOnly"
        | "chatRestricted"
        | "emailUnconfirmed"
        | "testClientOnly"
        | "notReady"
        | "timedOut"
        | "other";
      readonly message: string;
    };

export const Auth = {
  isLoggedIn: () => auth<boolean>("loggedIn"),
  getUsername: () => auth<string | null>("username"),
  getServer: () => auth<string | null>("server"),
  getServers: () => auth<readonly Server[]>("servers"),
  /** Logs this account back in, after waiting out a temporary kick. */
  login: () => auth<boolean>("login"),
  /** Matches the server name exactly, ignoring case. */
  connect: (server: string) => auth<ConnectResult>("connect", { server }),
  logout: () => auth<void>("logout"),
};

export type AutoReloginSettings = {
  readonly enabled: boolean;
  /** `null` logs back into the last server. */
  readonly server: string | null;
  /** Up to 300. */
  readonly delaySeconds: number;
  /** Up to 10. */
  readonly retries: number;
  /** From 1 to 600. */
  readonly backoffSeconds: number;
  /** From `backoffSeconds` to 600. */
  readonly backoffCapSeconds: number;
};

export const AutoRelogin = {
  get: () => autoRelogin<AutoReloginSettings>("get"),
  /** Changes only the given fields and returns the settings as stored, with out-of-range values clamped. */
  update: (patch: Partial<AutoReloginSettings>) =>
    autoRelogin<AutoReloginSettings>("update", patch),
};

export type State = "dead" | "idle" | "combat";
export type Position = { readonly x: number; readonly y: number };
export type Faction = {
  readonly id: number;
  readonly name: string;
  readonly rank: number;
  readonly rep: number;
};
export type Boost = "gold" | "xp" | "rep" | "classPoints";
export type BoostConfig = {
  /** False pauses using boosts. Registrations stay. */
  readonly enabled: boolean;
};
export type CellOptions = { readonly cell?: string; readonly pad?: string };
export type PadOptions = { readonly pad?: string };
export type WalkOptions = { readonly speed?: number };
export type RestOptions = { readonly untilFull?: boolean };
export type OutfitOptions = { readonly keepColors?: boolean };
export type Aura = {
  readonly name: string;
  readonly value: number | string | null;
  readonly stack: number;
  readonly category: string | null;
  readonly icon: string | null;
  readonly persistent: boolean;
  /** Full duration in seconds, or `0` when the aura has no timer. */
  readonly duration: number;
  /** Epoch milliseconds when the game's timer runs out. */
  readonly expiresAt: number | null;
};

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
  getAuras: () => player<readonly Aura[]>("auras"),
  getAura: (name: string) => player<Aura | null>("aura", { name }),
  hasAura: (name: string) => player<boolean>("hasAura", { name }),
  /** Waits to respawn and leaves combat before joining another map. */
  joinMap: (map: string, options: CellOptions = {}) =>
    player<boolean>("joinMap", { map, ...options }),
  jumpToCell: (cell: string, options: PadOptions = {}) =>
    player<boolean>("jumpToCell", { cell, ...options }),
  walkTo: (x: number, y: number, options: WalkOptions = {}) =>
    player<boolean>("walkTo", { x, y, ...options }),
  rest: (options: RestOptions = {}) => player<boolean>("rest", options),
  /** Joins the player's room and cell, wherever they are. Waits to respawn and leaves combat first. */
  goToPlayer: (username: string) => player<boolean>("goToPlayer", { username }),
  /** Registrations and config last until the session closes, after scripts end. */
  boosts: {
    isActive: (kind: Boost) => player<boolean>("hasBoost", { kind }),
    /**
     * Uses a registered item from the inventory whenever its kind of boost isn't active. Items of
     * the same kind are used in the order they were registered.
     */
    register: (items: readonly ItemQuery[]) => boostRegistry<void>("register", { items }),
    unregister: (items: readonly ItemQuery[]) => boostRegistry<void>("unregister", { items }),
    unregisterAll: () => boostRegistry<void>("unregisterAll"),
    getRegistered: () => boostRegistry<readonly ItemQuery[]>("registered"),
    getConfig: () => boostRegistry<BoostConfig>("config"),
    /** Changes only the given fields and returns the config as stored. */
    configure: (changes: Partial<BoostConfig>) => boostRegistry<BoostConfig>("configure", changes),
  },
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
  readonly aggressive: boolean;
  readonly auras: readonly Aura[];
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
export type RoomPlayer = {
  readonly name: string;
  readonly username: string;
  readonly userId: number;
  readonly position: Position | null;
  readonly cell: string;
  readonly pad: string;
  readonly hp: number;
  readonly maxHp: number;
  readonly mp: number;
  readonly maxMp: number;
  readonly level: number;
  readonly state: State | null;
  readonly afk: boolean;
  readonly auras: readonly Aura[];
};
export type Target =
  | ({ readonly type: "monster" } & Monster)
  | ({ readonly type: "player" } & RoomPlayer);
export type SkillOptions = {
  readonly target?: MonsterQuery;
  readonly force?: boolean;
  readonly waitUntilReady?: boolean;
};
/** A number is points, and a string like `"60%"` is a percent of the maximum. */
export type Amount = number | `${number}%`;
type Bound<T> =
  | { readonly atMost: T; readonly atLeast?: never }
  | { readonly atLeast: T; readonly atMost?: never };
export type Condition =
  | ({ readonly type: "hp"; readonly of: "me" | "anyAlly" } & Bound<Amount>)
  | ({ readonly type: "mp" } & Bound<Amount>)
  | ({
      readonly type: "aura";
      readonly on: "me" | "target";
      readonly name: string;
      /** Defaults to `"stacks"`. A missing aura counts as 0 on either measure. */
      readonly measure?: "stacks" | "value";
    } & Bound<number>);
export type Trigger = {
  /** Matched against combat and server messages, ignoring case and extra spaces. */
  readonly includes: string;
  readonly skill: Skill;
  /** A matching message casts the skill once, as soon as it's ready, unless this much time passes after the latest match. Matches heard before the cast share it. */
  readonly within: Duration.Input;
};
export type PriorityStep = {
  readonly skill: Skill;
  /** Every condition must hold. */
  readonly when?: readonly Condition[];
  /** Holds every skill for this long after this one casts. */
  readonly pauseAfter?: Duration.Input;
};
export type Step = PriorityStep & { readonly onCooldown?: "skip" | "wait" };
export type Profile = {
  readonly onCooldown: "skip" | "wait";
  /** Answered before priority and rotation, and not held by a pause. */
  readonly triggers?: readonly Trigger[];
  /** Checked first on every attempt, top to bottom. They never wait for a cooldown. */
  readonly priority?: readonly PriorityStep[];
  /** Cast in turn from where the last cast left off. `"wait"` holds the turn until that skill's cooldown ends. */
  readonly steps: readonly Step[];
};

export type KillOptions = {
  /** Time between skill attempts. Defaults to 200 ms, and anything under 50 ms counts as 50 ms. */
  readonly skillInterval?: Duration.Input;
  /** Attacked first, in this order, while any is alive in the cell. The kill still ends when its own target dies. */
  readonly targetPriority?: readonly MonsterQuery[];
} & (
  | {
      /** Used in this order, skipping any on cooldown. Defaults to `[1, 2, 3, 4]`. */
      readonly skills?: readonly Skill[];
      readonly profile?: never;
    }
  | {
      /** A saved combat profile's name, or a profile. Saving that profile while the kill runs applies the change. */
      readonly profile: string | Profile;
      readonly skills?: never;
    }
);

export type HuntOptions = {
  /** Picks the cell with the most live matches. Ties go to your own cell. */
  readonly preferMostMatches?: boolean;
};

export const Players = {
  getAll: () => players<readonly RoomPlayer[]>("all"),
  get: (username: string) => players<RoomPlayer | null>("get", { username }),
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
  exit: () => combat<boolean>("exit"),
  kill: (query: MonsterQuery, { skillInterval, profile, ...options }: KillOptions = {}) =>
    combat<boolean>("kill", {
      query,
      ...options,
      skillIntervalMs: millis(skillInterval),
      profile: typeof profile === "object" ? wireProfile(profile) : profile,
    }),
  hunt: (query: MonsterQuery, options: HuntOptions = {}) =>
    combat<Monster | null>("hunt", { query, ...options }),
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
export type DamageTarget =
  | "all"
  | "undead"
  | "human"
  | "chaos"
  | "dragonkin"
  | "orc"
  | "drakath"
  | "elemental";
export type GearBoost =
  | { readonly kind: "damage"; readonly against: DamageTarget; readonly percent: number }
  | { readonly kind: Boost; readonly percent: number };
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
  readonly boosts: readonly GearBoost[];
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
/**
 * `enhancement` is the first label the game shows, such as `"lucky"` or `"forge"`, and `special` the
 * second, such as `"spiral carve"`. Labels ignore case and accept common shortcuts like `"dtl"`.
 * A trait unique to one enhancement, like `"dauntless"` or `"vim"`, works on its own.
 */
export type EnhancementSelector = { readonly enhancement: string; readonly special?: string };
export type EnhancementSlot = "weapon" | "class" | "helm" | "cape";
export type EquipEnhancementSelector = EnhancementSelector & { readonly slot?: EnhancementSlot };

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
  /**
   * Equips the highest-level item already carrying the enhancement. A bare `"forge"` needs `slot` or
   * `special`, since every Forge slot shares it.
   */
  equipByEnhancement: (selector: EquipEnhancementSelector, options: EquipOptions = {}) =>
    inventory<boolean>("equipByEnhancement", { ...selector, ...options }),
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

const moveEach = (op: "deposit" | "withdraw", queries: readonly ItemQuery[]) =>
  Effect.forEach(queries, (query, index) => {
    const moved = bank<boolean>(op, { query });
    return index === 0 ? moved : Effect.andThen(Effect.sleep("150 millis"), moved);
  });

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
  /** Deposits each item in turn, even after one fails. One result per item, in order. */
  depositBatch: (queries: readonly ItemQuery[]) => moveEach("deposit", queries),
  /** Withdraws each item in turn, even after one fails. One result per item, in order. */
  withdrawBatch: (queries: readonly ItemQuery[]) => moveEach("withdraw", queries),
  swap: (inventoryQuery: ItemQuery, bankQuery: ItemQuery) =>
    bank<boolean>("swap", { inventoryQuery, bankQuery }),
};

export const House = {
  getItems: () => house<readonly Item[]>("items"),
  getItem: (query: ItemQuery) => house<Item | null>("item", { query }),
  getQuantity: (query: ItemQuery) => house<number>("quantity", { query }),
  contains: (query: ItemQuery, quantity?: number) =>
    house<boolean>("contains", { query, quantity }),
  getSlots: () => house<number>("slots"),
  getUsedSlots: () => house<number>("usedSlots"),
  getAvailableSlots: () => house<number>("availableSlots"),
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
export type QuestRegistration =
  | number
  | { readonly questId: number; readonly rewardItemId?: number };
export type RegisteredQuest = { readonly questId: number; readonly rewardItemId: number | null };
export type QuestConfig = {
  /** False pauses accepting and completing. Registrations stay. */
  readonly enabled: boolean;
  /** Registered quests' rewards count as registered drops. */
  readonly acceptRewards: boolean;
  /** Registered quests' requirements count as registered drops. */
  readonly acceptRequirements: boolean;
};

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
  /**
   * Accepts each quest whenever it's available and completes it with as many turn-ins as the items
   * allow. Registering a quest again replaces its `rewardItemId`; a quest with `choice` rewards
   * isn't completed without one. Registrations and config last until the session closes, after
   * scripts end.
   */
  register: (quests: readonly QuestRegistration[]) =>
    questRegistry<void>("register", {
      quests: quests.map((quest) => (typeof quest === "number" ? { questId: quest } : quest)),
    }),
  unregister: (questIds: readonly number[]) => questRegistry<void>("unregister", { questIds }),
  unregisterAll: () => questRegistry<void>("unregisterAll"),
  getRegistered: () => questRegistry<readonly RegisteredQuest[]>("registered"),
  getConfig: () => questRegistry<QuestConfig>("config"),
  /** Changes only the given fields and returns the config as stored. */
  configure: (changes: Partial<QuestConfig>) => questRegistry<QuestConfig>("configure", changes),
};

/** Rules for drops that aren't registered. `Ac` matches an `Item`'s `coins`, `MemberOnly` its `member`. */
export type DropConfig = {
  /** False pauses accepting and rejecting. Registrations stay. */
  readonly enabled: boolean;
  readonly acceptAc: boolean;
  readonly acceptAcMemberOnly: boolean;
  readonly acceptNonAc: boolean;
  readonly acceptNonAcMemberOnly: boolean;
  /** Rejects every drop that isn't registered or accepted above. */
  readonly rejectOthers: boolean;
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
  /**
   * Accepts these items whenever they're in the drop list. Registrations and config last until the
   * session closes, after scripts end.
   */
  register: (items: readonly ItemQuery[]) => dropRegistry<void>("register", { items }),
  unregister: (items: readonly ItemQuery[]) => dropRegistry<void>("unregister", { items }),
  unregisterAll: () => dropRegistry<void>("unregisterAll"),
  getRegistered: () => dropRegistry<readonly ItemQuery[]>("registered"),
  getConfig: () => dropRegistry<DropConfig>("config"),
  /** Changes only the given fields and returns the config as stored. */
  configure: (changes: Partial<DropConfig>) => dropRegistry<DropConfig>("configure", changes),
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
  /**
   * Opens the shop that sells the enhancement, joining /forge or /museum first when the shop is
   * there, and applies the highest level the player can buy, skipping listings locked by level,
   * membership, quest, faction rep, or gold. Returns true without buying when the item already has
   * that enhancement at the best level on offer.
   */
  enhanceItem: (query: ItemQuery, selector: EnhancementSelector) =>
    shops<boolean>("enhanceItem", { query, ...selector }),
};

/**
 * `"interface-only"` leaves only the interface visible while hiding the map and everyone in it.
 * `"minimal"` also stops drawing the game and holds it at 2 FPS; the game and scripts keep running.
 */
export type RenderingMode = "full" | "interface-only" | "minimal";

export type GameSettings = {
  readonly infiniteRange: boolean;
  readonly provokeCell: boolean;
  readonly enemyMagnet: boolean;
  readonly skipCutscenes: boolean;
  readonly hidePlayers: boolean;
  readonly rendering: RenderingMode;
  readonly animations: boolean;
  readonly collisions: boolean;
  /** `false` hides the death screen ad and keeps the Game Menu's news carousel from opening on its own. */
  readonly ads: boolean;
  /** From 1 to 99. The game's own speed is 8. */
  readonly walkSpeed: number;
  /**
   * The game's current frame rate, from 1 to 60, or `null` before the game loads.
   * Setting 30, 48, 60 or the game's own rate also changes its Max FPS option;
   * other rates last until the game or Mira changes it again.
   * While rendering is `"minimal"` this reads 2 and setting it has no effect.
   */
  readonly frameRate: number | null;
  /** Shown only to this client. `null` restores the character's own name. */
  readonly customName: string | null;
  /** Shown only to this client. `null` restores the character's own guild. */
  readonly customGuild: string | null;
  /**
   * On by default. Stops attacking a monster from its counter attack warning until the counter fades,
   * and holds skills while Sun's Heat is on you.
   * `Combat.attack`, `useSkill` and `kill` skip a countering monster, and auto attack waits it out.
   */
  readonly antiCounter: boolean;
  /** Clears AFK 1 to 1.5 seconds after the server marks you AFK, so you stay active while idle. */
  readonly antiAfk: boolean;
  /**
   * Walks to the safe side when a boss calls a zone in ledgermayne, moreskulls, ultradage,
   * darkcarnax, astralshrine, queeniona and magnumopus. Does nothing in other maps.
   */
  readonly autoZone: boolean;
};

export type SettingsPatch = Partial<Omit<GameSettings, "frameRate">> & {
  readonly frameRate?: number;
};

export const Settings = {
  get: () => settings<GameSettings>("get"),
  /** Changes only the given fields and returns the settings as stored, with out-of-range values clamped. */
  update: (patch: SettingsPatch) => settings<GameSettings>("update", patch),
};

export type ExitOptions = { readonly logout?: boolean; readonly closeTab?: boolean };

type InputCommon = { readonly label: string; readonly description?: string };
type InputOptions = readonly [string, ...string[]];

export type Input =
  | (InputCommon & {
      readonly type: "string";
      readonly required?: boolean;
      readonly default?: string;
    })
  | (InputCommon & {
      readonly type: "number";
      readonly integer?: boolean;
      readonly min?: number;
      readonly max?: number;
      readonly default?: number;
    })
  | (InputCommon & { readonly type: "boolean"; readonly default?: boolean })
  | (InputCommon & {
      readonly type: "select";
      readonly options: InputOptions;
      readonly multiple?: false;
      readonly default?: string;
    })
  | (InputCommon & {
      readonly type: "select";
      readonly options: InputOptions;
      readonly multiple: true;
      readonly default?: readonly string[];
    });

export type Inputs = { readonly [key: string]: Input };

type InputValue<F> = F extends { readonly type: "number" }
  ? number
  : F extends { readonly type: "boolean" }
    ? boolean
    : F extends {
          readonly type: "select";
          readonly multiple: true;
          readonly options: readonly (infer O)[];
        }
      ? readonly O[]
      : F extends { readonly type: "select"; readonly options: readonly (infer O)[] }
        ? O
        : string;

export type InputValues<D extends Inputs> = { readonly [K in keyof D]: InputValue<D[K]> };

/** The title defaults to the script's name. */
export type DialogOptions = { readonly title?: string };
export type ConfirmOptions = DialogOptions & {
  readonly confirm?: string;
  readonly cancel?: string;
};
/** `default` fills the box; `placeholder` only hints at what to type. */
export type PromptOptions = DialogOptions & {
  readonly default?: string;
  readonly placeholder?: string;
};

export const Script = {
  /** Waits until the user dismisses it. Interrupting the effect closes the dialog. */
  alert: (message: string, { title }: DialogOptions = {}) =>
    ask<null>({ op: "alert", message, title }).pipe(Effect.asVoid),
  /** True when confirmed, false when cancelled. Interrupting the effect closes the dialog. */
  confirm: (message: string, { title, confirm, cancel }: ConfirmOptions = {}) =>
    ask<boolean>({ op: "confirm", message, title, confirm, cancel }),
  /** The text as entered, or null when cancelled. Interrupting the effect closes the dialog. */
  prompt: (message: string, { title, default: initial, placeholder }: PromptOptions = {}) =>
    ask<string | null>({ op: "prompt", message, title, default: initial, placeholder }),
  /** The values this run started with. Pass `typeof inputs` to type them; Mira checks the values, not the type. */
  inputs: <D extends Inputs = Inputs>(): InputValues<D> => inputs.values as InputValues<D>,
  sleep: (duration: Duration.Input) => Effect.sleep(duration),
  /** Aborts when the script is stopped or exits, before cleanup runs, or when it ends. */
  signal: stopping.signal,
  /** Ends the script from any fiber. Cleanup runs first, then the logout, then the tab closes a second later. */
  exit: ({ logout = false, closeTab = false }: ExitOptions = {}): Effect.Effect<never> =>
    Effect.sync(() => {
      exiting.request ??= { logout, closeTab };
      stopping.abort();
    }).pipe(Effect.andThen(Deferred.succeed(exited, undefined)), Effect.andThen(Effect.never)),
};

export type Subscription<A> = { readonly next: Effect.Effect<A> };

/** A generator function or a function returning an Effect, like the default export. */
export type Handler<A> = (value: A) => Effect.Effect<unknown, unknown> | HandlerGenerator;
type HandlerGenerator = Generator<Effect.Effect<unknown, unknown>, unknown, never>;

export type ClientPacket = {
  readonly from: "client";
  readonly cmd: string;
  /** Excludes the room id. */
  readonly args: readonly string[];
};
export type ServerPacket =
  | {
      readonly from: "server";
      readonly format: "json";
      readonly cmd: string;
      /** The packet's `dataObj`, with `cmd`. */
      readonly data: Readonly<Record<string, unknown>>;
    }
  | {
      readonly from: "server";
      readonly format: "str";
      readonly cmd: string;
      /** Excludes the room id. */
      readonly args: readonly string[];
    };
export type Packet = ClientPacket | ServerPacket;

/** Omitted fields match every packet. */
export type PacketFilter = {
  readonly from?: "client" | "server";
  readonly cmd?: string | readonly string[];
};
export type PacketFor<F extends PacketFilter> = F extends { readonly from: "client" }
  ? ClientPacket
  : F extends { readonly from: "server" }
    ? ServerPacket
    : Packet;

const lost: unique symbol = Symbol("lost");
type Lost = typeof lost;
type Batch<I> = {
  readonly items: readonly (I | { readonly lost: true })[];
  readonly lagged: boolean;
};
type Read<I, A> = (item: I) => A | undefined;

let lastFeed = 0;

const openFeed = <I extends object, A>(
  subscribe: (id: number) => Effect.Effect<unknown>,
  read: Read<I, A>,
): Effect.Effect<Subscription<A | Lost>, never, Scope.Scope> =>
  Effect.gen(function* () {
    const id = ++lastFeed;
    let closed = false;
    yield* Effect.acquireRelease(subscribe(id), () =>
      Effect.suspend(() => {
        closed = true;
        return feeds<null>("close", { id });
      }),
    );
    const scope = yield* Effect.scope;
    const buffer: (A | Lost)[] = [];
    let pulling: Deferred.Deferred<void> | undefined;
    const pull = feeds<Batch<I>>("next", { id }).pipe(
      Effect.map(({ items, lagged }) => {
        if (lagged) console.warn("A subscription fell behind and dropped what it was holding.");
        for (const item of items) {
          const value = "lost" in item ? lost : read(item as I);
          if (value !== undefined) buffer.push(value);
        }
      }),
      Effect.ensuring(
        Effect.sync(() => {
          pulling = undefined;
        }),
      ),
    );
    const next: Effect.Effect<A | Lost> = Effect.suspend(() => {
      if (closed) return Effect.die(new Error("the subscription is closed"));
      if (buffer.length > 0) return Effect.succeed(buffer.shift() as A | Lost);
      if (pulling !== undefined) return Effect.andThen(Deferred.await(pulling), next);
      const done = Deferred.makeUnsafe<void>();
      pulling = done;
      return Effect.forkIn(pull.pipe(Deferred.into(done)), scope).pipe(
        Effect.andThen(Deferred.await(done)),
        Effect.andThen(next),
      );
    });
    return { next };
  });

const skipLost = <A>({ next }: Subscription<A | Lost>): Subscription<A> => ({
  next: Effect.gen(function* () {
    while (true) {
      const value = yield* next;
      if (value !== lost) return value;
    }
  }),
});

const run = (
  result: Effect.Effect<unknown, unknown> | HandlerGenerator,
): Effect.Effect<unknown, unknown> => (Effect.isEffect(result) ? result : Effect.gen(() => result));

const failingTheScript = <A, E>(effect: Effect.Effect<A, E>): Effect.Effect<A, E> =>
  Effect.onExit(effect, (exit) =>
    Exit.isFailure(exit) && !Cause.hasInterruptsOnly(exit.cause)
      ? Deferred.failCause(failed, exit.cause)
      : Effect.void,
  );

const listen = <A>(
  next: Effect.Effect<A>,
  handler: Handler<A>,
): Effect.Effect<void, never, Scope.Scope> =>
  Effect.forkScoped(
    failingTheScript(
      Effect.forever(Effect.flatMap(next, (value) => Effect.suspend(() => run(handler(value))))),
    ),
  ).pipe(Effect.asVoid);

export type ForOptions<E> = {
  /** Starts after `trigger` finishes. Waits forever when omitted. */
  readonly timeout?: Duration.Input;
  /** Runs once the subscription is open. Returning false skips the wait. */
  readonly trigger?: Effect.Effect<boolean, E>;
};

const waitFor = <A, E>(
  open: Effect.Effect<Subscription<A | Lost>, never, Scope.Scope>,
  { timeout, trigger }: ForOptions<E>,
): Effect.Effect<A | null, E> =>
  Effect.scoped(
    Effect.gen(function* () {
      const { next } = yield* open;
      if (trigger !== undefined && !(yield* trigger)) return null;
      const first = Effect.map(next, (value) => (value === lost ? null : value));
      return timeout === undefined
        ? yield* first
        : Option.getOrNull(yield* Effect.timeoutOption(first, timeout));
    }),
  );

type RawPacket = { readonly from: "client" | "server"; readonly packet: string };

const readPacket =
  (cmds: readonly string[] | undefined): Read<RawPacket, Packet> =>
  ({ from, packet }) => {
    const read = from === "client" ? readClient(packet) : readServer(packet);
    return read !== undefined && (cmds === undefined || cmds.includes(read.cmd)) ? read : undefined;
  };

const readClient = (packet: string): ClientPacket | undefined => {
  const start = packet.indexOf("%xt%");
  if (start < 0) return undefined;
  const parts = packet.slice(start + 1, packet.lastIndexOf("%")).split("%");
  const cmd = parts[2];
  return cmd === undefined ? undefined : { from: "client", cmd, args: parts.slice(4) };
};

const readServer = (packet: string): ServerPacket | undefined => {
  let parsed: unknown;
  try {
    parsed = JSON.parse(packet);
  } catch {
    return undefined;
  }
  if (typeof parsed !== "object" || parsed === null) return undefined;
  const { type, dataObj } = parsed as { readonly type?: unknown; readonly dataObj?: unknown };
  if (
    type === "json" &&
    typeof dataObj === "object" &&
    dataObj !== null &&
    !Array.isArray(dataObj)
  ) {
    const data = dataObj as Readonly<Record<string, unknown>>;
    return typeof data["cmd"] === "string"
      ? { from: "server", format: "json", cmd: data["cmd"], data }
      : undefined;
  }
  if (type === "str" && Array.isArray(dataObj) && typeof dataObj[0] === "string") {
    return { from: "server", format: "str", cmd: dataObj[0], args: dataObj.slice(2).map(String) };
  }
  return undefined;
};

const openPackets = <F extends PacketFilter>(filter: F) => {
  const cmds = typeof filter.cmd === "string" ? [filter.cmd] : filter.cmd;
  return openFeed(
    (id) => packets("subscribe", { id, from: filter.from, cmds }),
    readPacket(cmds) as Read<RawPacket, PacketFor<F>>,
  );
};

export const Packets = {
  subscribe: <const F extends PacketFilter>(
    filter: F = {} as F,
  ): Effect.Effect<Subscription<PacketFor<F>>, never, Scope.Scope> =>
    Effect.map(openPackets(filter), skipLost),
  on: <const F extends PacketFilter>(
    filter: F,
    handler: Handler<PacketFor<F>>,
  ): Effect.Effect<void, never, Scope.Scope> =>
    Effect.flatMap(openPackets(filter), (feed) => listen(skipLost(feed).next, handler)),
  /** A full `%xt%zm%cmd%room%...%` packet. False when not logged in. */
  sendToServer: (packet: `%xt%zm%${string}%`) => packets<boolean>("sendToServer", { packet }),
  /** The game handles `data` as a json packet from the server. False when not in game. */
  sendToClient: (data: { readonly cmd: string; readonly [key: string]: unknown }) =>
    packets<boolean>("sendToClient", { data }),
};

export type GameEvent =
  | { readonly type: "monsterDeath"; readonly monMapId: number }
  | {
      readonly type: "itemDrop";
      readonly itemId: number;
      readonly name: string;
      /** The amount in this drop. */
      readonly quantity: number;
    }
  /** A player went AFK or came back. `self` is you. */
  | {
      readonly type: "afk";
      readonly username: string;
      readonly self: boolean;
      readonly afk: boolean;
    }
  /** `""` is the map's default zone. */
  | { readonly type: "zone"; readonly zone: string }
  | { readonly type: "mapJoined"; readonly map: string }
  /** A player jumped to a cell. Walking within a cell doesn't count. */
  | {
      readonly type: "cellChanged";
      readonly username: string;
      readonly self: boolean;
      readonly cell: string;
      readonly pad: string;
    }
  /** Another player entered the room. */
  | { readonly type: "playerJoined"; readonly username: string; readonly self: boolean }
  | { readonly type: "playerLeft"; readonly username: string; readonly self: boolean }
  /** The connection to the server dropped. */
  | { readonly type: "logout" }
  /** `"dead"` is a death. `"idle"` is a respawn or leaving combat. */
  | {
      readonly type: "playerState";
      readonly username: string;
      readonly self: boolean;
      readonly state: State;
    }
  | { readonly type: "monsterRespawn"; readonly monMapId: number }
  /** `targetId` is a monster's `monMapId` or a player's `userId`. */
  | {
      readonly type: "auraAdded";
      readonly name: string;
      readonly targetType: "monster" | "player";
      readonly targetId: number;
      readonly self: boolean;
      /** Seconds. */
      readonly duration: number | null;
    }
  | {
      readonly type: "auraRemoved";
      readonly name: string;
      readonly targetType: "monster" | "player";
      readonly targetId: number;
      readonly self: boolean;
    }
  /** A line a monster says, or a message the server shows you. */
  | { readonly type: "message"; readonly text: string; readonly monMapId: number | null }
  /** An item went straight into your inventory, such as a quest reward or a temporary item. */
  | {
      readonly type: "itemAdded";
      readonly itemId: number;
      readonly name: string;
      readonly quantity: number;
      readonly temp: boolean;
    }
  /** The server accepted a turn-in, whether from a script or the game's quest window. */
  | { readonly type: "questCompleted"; readonly questId: number; readonly name: string };
export type EventType = GameEvent["type"];
export type EventOf<T extends EventType> = Extract<GameEvent, { readonly type: T }>;
/** `type` picks the event. Other fields must match exactly. */
export type EventFilter<T extends EventType> = { readonly type: T } & Partial<
  Omit<EventOf<T>, "type">
>;

const openEvents = <T extends EventType>(filter: EventFilter<T>) =>
  openFeed(
    (id) => events("subscribe", { id, type: filter.type }),
    (event: GameEvent) =>
      Object.entries(filter).every(
        ([key, value]) => (event as Record<string, unknown>)[key] === value,
      )
        ? (event as EventOf<T>)
        : undefined,
  );

export const Events = {
  subscribe: <const T extends EventType>(
    filter: EventFilter<T>,
  ): Effect.Effect<Subscription<EventOf<T>>, never, Scope.Scope> =>
    Effect.map(openEvents(filter), skipLost),
  on: <const T extends EventType>(
    filter: EventFilter<T>,
    handler: Handler<EventOf<T>>,
  ): Effect.Effect<void, never, Scope.Scope> =>
    Effect.flatMap(openEvents(filter), (feed) => listen(skipLost(feed).next, handler)),
};

export type UntilOptions = {
  /** Defaults to 100 ms. */
  readonly interval?: Duration.Input;
  /** Waits forever when omitted. */
  readonly timeout?: Duration.Input;
};

const poll = <E>(condition: Effect.Effect<boolean, E>, interval: Duration.Input) =>
  Effect.gen(function* () {
    while (!(yield* condition)) yield* Effect.sleep(interval);
    return true;
  });

export const Wait = {
  /** Checks right away, then every `interval`. False if `timeout` passes first. */
  until: <E = never>(
    condition: Effect.Effect<boolean, E>,
    { interval = "100 millis", timeout }: UntilOptions = {},
  ): Effect.Effect<boolean, E> =>
    timeout === undefined
      ? poll(condition, interval)
      : poll(condition, interval).pipe(Effect.timeoutOption(timeout), Effect.map(Option.isSome)),
  /** Null if `trigger` returns false, the timeout passes, or the connection drops. */
  forPacket: <const F extends PacketFilter, E = never>(
    filter: F,
    options: ForOptions<E> = {},
  ): Effect.Effect<PacketFor<F> | null, E> => waitFor(openPackets(filter), options),
  /**
   * Null if `trigger` returns false, the timeout passes, or the connection drops.
   * A `logout` wait returns the event when the connection drops.
   */
  forEvent: <const T extends EventType, E = never>(
    filter: EventFilter<T>,
    options: ForOptions<E> = {},
  ): Effect.Effect<EventOf<T> | null, E> => waitFor(openEvents(filter), options),
};

export type Json =
  | null
  | boolean
  | number
  | string
  | readonly Json[]
  | { readonly [key: string]: Json };

export type ArmyInfo = {
  readonly name: string;
  /** Starts at 1 and follows the order of the config's `players`. */
  readonly number: number;
  readonly size: number;
  readonly room: number;
  readonly players: readonly string[];
};

export type ArmyStepOptions = {
  /** If the other players haven't all arrived by then, the army ends. Without it, the step waits forever. */
  readonly timeout?: Duration.Input | undefined;
};

export type ArmyUntilOptions = ArmyStepOptions & {
  /** Rounds of `work` start at least this far apart. Defaults to 500 ms. */
  readonly interval?: Duration.Input;
};

export type ArmyMapOptions = CellOptions & ArmyStepOptions;

export type Body<A> = () =>
  | Effect.Effect<A, unknown>
  | Generator<Effect.Effect<unknown, unknown>, A, never>;

export type ArmyUntil = {
  readonly done: Body<boolean>;
  readonly work: Body<unknown>;
};

export type ArmyTurns = {
  readonly players: readonly number[];
  readonly turn: Body<boolean>;
};

export type Rotation = { readonly stop: Effect.Effect<void> };

type ArmyStart = {
  (name: string): Effect.Effect<ArmyInfo>;
  (name: string, options: { readonly timeout: Duration.Input }): Effect.Effect<ArmyInfo | null>;
};

// The game can leave the consumable slot broken after a class change. Equipping safeClass and
// safePot first, then safePot again after class, is a workaround meant to restore it.
const GEAR = [
  "safeClass",
  "safePot",
  "class",
  "safePot",
  "weapon",
  "cape",
  "helm",
  "armor",
  "pet",
] as const;

type Gear = { readonly [Field in (typeof GEAR)[number] | "scroll"]: string | null } & {
  readonly pots: readonly string[] | null;
};

let joined: ArmyInfo | undefined;

const perform = <A>(body: Body<A>): Effect.Effect<A> =>
  Effect.orDie(Effect.suspend(() => run(body()) as Effect.Effect<A, unknown>));

const start = ((name: string, { timeout }: { readonly timeout?: Duration.Input } = {}) =>
  army<ArmyInfo | null>("start", { name, timeoutMs: millis(timeout) }).pipe(
    Effect.map((info) => {
      if (info !== null) joined = info;
      return info;
    }),
  )) as ArmyStart;

const sync = (label: string, { timeout }: ArmyStepOptions = {}): Effect.Effect<void> =>
  Effect.asVoid(army<null>("sync", { label, timeoutMs: millis(timeout) }));

const gather = <A extends Json>(
  label: string,
  value: A,
  { timeout }: ArmyStepOptions = {},
): Effect.Effect<readonly A[]> =>
  army<readonly A[]>("gather", { label, value, timeoutMs: millis(timeout) });

const until = (
  label: string,
  { done, work }: ArmyUntil,
  { interval = "500 millis", timeout }: ArmyUntilOptions = {},
): Effect.Effect<void> =>
  Effect.gen(function* () {
    let last = yield* perform(done);
    if (yield* army<boolean>("until", { label, done: last })) return;
    const round = Effect.gen(function* () {
      yield* Effect.all([perform(work), Effect.sleep(interval)], { concurrency: 2 });
      const now = yield* perform(done);
      if (now === last) return;
      last = now;
      yield* army<boolean>("report", { done: now });
    });
    yield* Effect.raceFirst(
      army<null>("settle", { timeoutMs: millis(timeout) }),
      Effect.forever(round),
    );
  });

const joinMap = (map: string, { timeout, ...cell }: ArmyMapOptions = {}): Effect.Effect<boolean> =>
  Effect.gen(function* () {
    const label = `joinMap ${map}`;
    yield* sync(label, { timeout });
    const room = joined?.room;
    const arrived =
      room !== undefined &&
      (yield* Player.joinMap(`${map}-${room}`, cell)) &&
      (yield* World.getRoomNumber()) === room;
    return (yield* gather(label, arrived, { timeout })).every(Boolean);
  });

const drink = (pot: string): Effect.Effect<boolean> =>
  Effect.gen(function* () {
    const item = yield* Inventory.getItem(pot);
    if (item === null) return false;
    if (["elixir", "tonic"].includes(item.link.trim().toLowerCase())) {
      return yield* Inventory.use(pot);
    }
    if (!(yield* Inventory.equip(pot))) return false;
    if (!(yield* Combat.useSkill(5, { force: true, waitUntilReady: true }))) return false;
    return yield* Wait.until(
      Effect.map(Inventory.getQuantity(pot), (left) => left < item.quantity),
      { timeout: "5 seconds" },
    );
  });

const equipSet = (name: string, { timeout }: ArmyStepOptions = {}): Effect.Effect<boolean> =>
  Effect.gen(function* () {
    const gear = yield* army<Gear>("gear", { name });
    let equipped = true;
    for (const field of GEAR) {
      const item = gear[field];
      if (item !== null) equipped = (yield* Inventory.equip(item)) && equipped;
    }
    for (const pot of gear.pots ?? []) equipped = (yield* drink(pot)) && equipped;
    if (gear.scroll !== null) equipped = (yield* Inventory.equip(gear.scroll)) && equipped;
    return (yield* gather(`equipSet ${name}`, equipped, { timeout })).every(Boolean);
  });

const takeTurns = (label: string, { players, turn }: ArmyTurns): Effect.Effect<Rotation> =>
  Effect.gen(function* () {
    if (!(yield* army<boolean>("rotate", { label, players }))) return { stop: Effect.void };
    const taking = Effect.gen(function* () {
      let acted: boolean | null = null;
      while (true) {
        yield* army<null>("turn", { label, acted });
        acted = yield* perform(turn);
      }
    });
    const fiber = yield* Effect.forkDetach(
      failingTheScript(Effect.ensuring(taking, army<null>("leave", { label }))),
    );
    return { stop: Fiber.interrupt(fiber) };
  });

export const Army = {
  /** Reads `~/Documents/Mira/armies/<name>.yaml` and waits until every player in it has started. */
  start,
  sync,
  /** Values come back in player order. */
  gather,
  /**
   * Every player runs `work` in rounds until `done` is true for all of them at the same time.
   * Players that are done keep working.
   */
  until,
  /** Joins `<map>-<room>`. Every player gets false if any player didn't get into the room. */
  joinMap,
  /**
   * Equips this player's gear for the set. Every player gets false if any player couldn't equip
   * all of theirs.
   */
  equipSet,
  /**
   * Runs `turn` on one player at a time, in `players` order, until `stop` or the end of the
   * script. `turn` returns whether it acted. Players not listed take no turns.
   */
  takeTurns,
};
