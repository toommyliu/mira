import * as Effect from "effect/Effect";

import * as S from "../src/schema.ts";

const Farm = S.object({
  map: S.string().min(1),
  kills: S.number().int().default(10),
  mode: S.literal("solo", "party"),
  note: S.string().optional(),
  boss: S.object({ name: S.string(), hp: S.number() }).nullable(),
  drops: S.array(S.string()).min(1),
  ids: S.record(S.number()),
  target: S.union(S.string(), S.number()),
}).refine((farm) => farm.kills > 0);

type Farm = S.Infer<typeof Farm>;

export const typed = (farm: Farm) => {
  const map: string = farm.map;
  const kills: number = farm.kills;
  const mode: "solo" | "party" = farm.mode;
  const note: string | undefined = farm.note;
  const hp: number | undefined = farm.boss?.hp;
  const drop: string | undefined = farm.drops[0];
  const id: number | undefined = farm.ids["Bone"];
  const target: string | number = farm.target;
  // @ts-expect-error a literal outside the list
  const raid: "raid" = farm.mode;
  // @ts-expect-error parsed values are read-only
  farm.map = "";
  return [map, kills, mode, note, hp, drop, id, target, raid];
};

export const fields: Farm = {
  map: "battleunderb",
  kills: 1,
  mode: "solo",
  boss: null,
  drops: ["Bone"],
  ids: {},
  target: 1,
};

const { boss: _, ...withoutBoss } = fields;
// @ts-expect-error a field without a default or optional() is required
export const missing: Farm = withoutBoss;

export const parsed: Effect.Effect<Farm, S.ParseError> = Farm.parse({});

export const narrowed = (value: unknown): string => (Farm.is(value) ? value.map : "");

// @ts-expect-error a default must match the schema
S.number().default("ten");

// @ts-expect-error string checks don't apply to numbers
S.number().regex(/x/);

// @ts-expect-error object fields must be schemas
S.object({ map: "battleunderb" });

// @ts-expect-error optional() makes an object field, which can't also be nullable
S.string().optional().nullable();

// @ts-expect-error record values can't be optional
S.record(S.number().optional());

// @ts-expect-error array elements can't have defaults
S.array(S.number().default(0));

// @ts-expect-error union members can't be optional
S.union(S.string().optional(), S.number());

export const nullableField = S.object({ boss: S.string().nullable().optional() });
