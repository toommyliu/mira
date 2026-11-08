import type * as Effect from "effect/Effect";

import { call } from "./call.ts";

export type Settings = {
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

const autoRelogin = <A>(op: string, args: object = {}): Effect.Effect<A> =>
  call<A>({ domain: "autoRelogin", op, ...args });

export const get = () => autoRelogin<Settings>("get");
/** Changes only the given fields and returns the settings as stored, with out-of-range values clamped. */
export const update = (patch: Partial<Settings>) => autoRelogin<Settings>("update", patch);
