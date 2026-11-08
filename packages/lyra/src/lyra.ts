import * as Cause from "effect/Cause";
import * as Deferred from "effect/Deferred";
import type * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Option from "effect/Option";
import type * as Scope from "effect/Scope";

import { failed, stopping } from "./run.ts";
import { call } from "./call.ts";

declare const console: { readonly warn: (message: string) => void };

const auth = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "auth", op, ...args });

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

const shops = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "shops", op, ...args });

const packets = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "packets", op, ...args });

const events = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "events", op, ...args });

const feeds = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "feeds", op, ...args });

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
  hasBoost: (kind: Boost) => player<boolean>("hasBoost", { kind }),
  getAuras: () => player<readonly Aura[]>("auras"),
  getAura: (name: string) => player<Aura | null>("aura", { name }),
  hasAura: (name: string) => player<boolean>("hasAura", { name }),
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
  /** Aborts when the script is stopped, before cleanup runs, or when it ends. */
  signal: stopping.signal,
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

const listen = <A>(
  next: Effect.Effect<A>,
  handler: Handler<A>,
): Effect.Effect<void, never, Scope.Scope> =>
  Effect.forkScoped(
    Effect.forever(Effect.flatMap(next, (value) => Effect.suspend(() => run(handler(value))))).pipe(
      Effect.onExit((exit) =>
        Exit.isFailure(exit) && !Cause.hasInterruptsOnly(exit.cause)
          ? Deferred.failCause(failed, exit.cause)
          : Effect.void,
      ),
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
    };
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
