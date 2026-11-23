/** @module mira */
import type * as Effect from "effect/Effect";

declare const run: <A, E = never>(...args: readonly unknown[]) => Effect.Effect<A, E>;

export type Query = string | { readonly id: number };
export type Aura = { readonly name: string };
export type Target = { readonly name: string; readonly auras: readonly Aura[] };
export type KillOptions = { readonly retries?: number };
export type KillError = { readonly reason: "fled" | "timeout" };

export const Combat = {
  getTarget: () => run<Target | null>(),
  kill: (query: Query, { retries }: KillOptions = {}) => run<boolean, KillError>(retries === 0),
};

export const World = {
  find: (query: Query) => run<boolean>(query),
  cells: {
    list: () => run<readonly string[]>(),
  },
};
