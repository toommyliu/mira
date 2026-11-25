/** @module mira/http */
import type * as Effect from "effect/Effect";

declare const run: <A, E = never>(...args: readonly unknown[]) => Effect.Effect<A, E>;

export type Response = { readonly status: number };
export type RequestError = { readonly reason: "network" | "timeout" };

export const request = (url: string) => run<Response, RequestError>(url);
