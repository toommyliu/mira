/** @module mira */
import type * as Effect from "effect/Effect";

declare const run: <A, E = never>(...args: readonly unknown[]) => Effect.Effect<A, E>;

export type Query = string | { readonly id: number };
export type Aura = { readonly name: string };
export type Target = { readonly name: string; readonly auras: readonly Aura[] };
export type KillOptions = { readonly retries?: number };
export type KillError = { readonly reason: "fled" | "timeout" };
/** @group World */
export type Signal =
  /** A door opened. */
  | {
      readonly type: "open";
      /** Which door. */
      readonly door: string;
    }
  | { readonly type: "close" };
/** @group World */
export type SignalType = Signal["type"];

export const Combat = {
  getTarget: () => run<Target | null>(),
  kill: (query: Query, { retries }: KillOptions = {}) => run<boolean, KillError>(retries === 0),
  waitFor: (type: SignalType) => run<Signal>(type),
};

export const World = {
  find: (query: Query) => run<boolean>(query),
  cells: {
    list: () => run<readonly string[]>(),
  },
  on: <const T extends SignalType>(type: T) => run<Extract<Signal, { readonly type: T }>>(type),
};
