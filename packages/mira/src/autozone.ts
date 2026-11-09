import type * as Effect from "effect/Effect";

import { call } from "./call.ts";

export type SupportedMap =
  | "ledgermayne"
  | "moreskulls"
  | "ultradage"
  | "darkcarnax"
  | "astralshrine"
  | "queeniona"
  | "magnumopus";

export type Settings = {
  readonly enabled: boolean;
  /** Auto zone moves the player only while they are in this map. */
  readonly map: SupportedMap | null;
};

const autoZone = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "autoZone", op, ...args });

export const get = () => autoZone<Settings>("get");
/** Changes only the given fields and returns the settings as stored. */
export const update = (patch: Partial<Settings>) => autoZone<Settings>("update", patch);
