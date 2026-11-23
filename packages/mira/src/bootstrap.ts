import * as Cause from "effect/Cause";
import * as Deferred from "effect/Deferred";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Fiber from "effect/Fiber";
import * as Logger from "effect/Logger";
import type * as LogLevel from "effect/LogLevel";
import * as References from "effect/References";

import { call } from "./call.ts";
import { drive, type Yielded } from "./drive.ts";
import { exited, exiting, failed, stopping, type ExitRequest } from "./run.ts";

type Settle = {
  (outcome: "completed" | "stopped"): void;
  (outcome: "exited", request: ExitRequest): void;
  (outcome: "failed", message: string, stack?: string): void;
};

type ConsoleLevel = "debug" | "info" | "warn" | "error";

declare const console: Record<ConsoleLevel, (...values: unknown[]) => void>;

const GeneratorFunction = Object.getPrototypeOf(function* () {}).constructor;

const consoleLevel = (level: LogLevel.LogLevel): ConsoleLevel => {
  switch (level) {
    case "Fatal":
    case "Error":
      return "error";
    case "Warn":
      return "warn";
    case "Info":
      return "info";
    default:
      return "debug";
  }
};

const logger = Logger.make<unknown, void>(({ logLevel, message, cause, fiber }) => {
  const parts = Array.isArray(message) ? [...message] : [message];
  if (cause.reasons.length > 0) parts.push(Cause.pretty(cause));
  const annotations = fiber.getRef(References.CurrentLogAnnotations);
  if (Object.keys(annotations).length > 0) parts.push(annotations);
  console[consoleLevel(logLevel)](...parts);
});

const program = (exported: unknown): Effect.Effect<unknown, unknown> | undefined => {
  if (Effect.isEffect(exported)) return exported as Effect.Effect<unknown, unknown>;
  if (exported instanceof GeneratorFunction) {
    const body = exported as () => Generator<Yielded, unknown, unknown>;
    return Effect.suspend(() => drive(body()));
  }
  return undefined;
};

export function start(exported: unknown, settle: Settle): () => void {
  const root = program(exported);
  if (root === undefined) {
    settle("failed", "The default export must be a generator function or an Effect.");
    return () => {};
  }
  const ended = Effect.scoped(
    Effect.raceFirst(root, Effect.raceFirst(Deferred.await(failed), Deferred.await(exited))),
  ).pipe(
    Effect.provideService(References.CurrentLoggers, new Set([logger])),
    Effect.provideService(References.MinimumLogLevel, "Trace"),
  );
  const fiber = Effect.runFork(
    Effect.uninterruptibleMask((restore) =>
      Effect.flatMap(Effect.exit(restore(ended)), (result) => {
        const request = exiting.request;
        if (request === undefined) return result;
        if (!request.logout) return Effect.void;
        const logout = call<void>({ domain: "auth", op: "logout" });
        return request.closeTab ? Effect.andThen(logout, Effect.sleep("1 second")) : logout;
      }),
    ),
  );
  fiber.addObserver((exit) => {
    stopping.abort();
    const request = exiting.request;
    if (Exit.isSuccess(exit)) {
      return request === undefined ? settle("completed") : settle("exited", request);
    }
    if (Cause.hasInterruptsOnly(exit.cause)) {
      return request === undefined ? settle("stopped") : settle("exited", request);
    }
    const error = Cause.squash(exit.cause);
    if (error instanceof Error) return settle("failed", error.message, error.stack);
    settle("failed", String(error), Cause.pretty(exit.cause));
  });
  return () => {
    stopping.abort();
    Effect.runFork(Fiber.interrupt(fiber));
  };
}
