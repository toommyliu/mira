import type * as Effect from "effect/Effect";

export type Spin = { readonly status: "spun" } | { readonly status: "already-spun" };

declare const spin: Effect.Effect<Spin>;

export function* wheelOfDoom(): Generator<Effect.Effect<unknown, unknown>, Spin, unknown> {
  return yield* spin;
}
