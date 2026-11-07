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

export const Script = {
  sleep: (duration: Duration.Input) => Effect.sleep(duration),
};
