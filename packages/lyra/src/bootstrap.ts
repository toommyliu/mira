import * as Cause from "effect/Cause";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Fiber from "effect/Fiber";

import { failed, stopping } from "./run.ts";

type Settle = (
  outcome: "completed" | "failed" | "stopped",
  message?: string,
  stack?: string,
) => void;

const GeneratorFunction = Object.getPrototypeOf(function* () {}).constructor;

const program = (exported: unknown): Effect.Effect<unknown, unknown> | undefined => {
  if (Effect.isEffect(exported)) return exported as Effect.Effect<unknown, unknown>;
  if (exported instanceof GeneratorFunction) {
    return Effect.gen(exported as () => Generator<Effect.Effect<unknown, unknown>, unknown>);
  }
  return undefined;
};

export function start(exported: unknown, settle: Settle): () => void {
  const root = program(exported);
  if (root === undefined) {
    settle("failed", "The default export must be a generator function or an Effect.");
    return () => {};
  }
  const fiber = Effect.runFork(Effect.scoped(Effect.raceFirst(root, Deferred.await(failed))));
  fiber.addObserver((exit) => {
    stopping.abort();
    if (Exit.isSuccess(exit)) return settle("completed");
    if (Cause.hasInterruptsOnly(exit.cause)) return settle("stopped");
    const error = Cause.squash(exit.cause);
    if (error instanceof Error) return settle("failed", error.message, error.stack);
    settle("failed", String(error), Cause.pretty(exit.cause));
  });
  return () => {
    stopping.abort();
    Effect.runFork(Fiber.interrupt(fiber));
  };
}
