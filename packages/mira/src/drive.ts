import * as Cause from "effect/Cause";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Option from "effect/Option";
import { SingleShotGen } from "effect/Utils";

export type Yielded = Effect.Effect<unknown, unknown>;

type Driven<A> = Generator<Yielded, A, unknown>;

// `yield* effect` hands off to SingleShotGen, which has no `throw`. Throwing into a generator paused
// there would raise "The iterator does not provide a 'throw' method" in place of the error.
Object.defineProperty(SingleShotGen.prototype, "throw", {
  value(error: unknown): never {
    throw error;
  },
});

const catchable = (cause: Cause.Cause<unknown>): Option.Option<unknown> =>
  Cause.hasInterrupts(cause) || Cause.hasDies(cause) ? Option.none() : Cause.findErrorOption(cause);

const run = <A>(
  generator: Driven<A>,
  start: (generator: Driven<A>) => IteratorResult<Yielded, A>,
): Effect.Effect<A, unknown> => {
  const settle = (step: IteratorResult<Yielded, A>): Effect.Effect<A, unknown> =>
    step.done ? Effect.succeed(step.value) : Effect.matchCauseEffect(step.value, handlers);
  const handlers = {
    onSuccess: (value: unknown): Effect.Effect<A, unknown> => {
      try {
        return settle(generator.next(value));
      } catch (error) {
        return Effect.fail(error);
      }
    },
    onFailure: (cause: Cause.Cause<unknown>): Effect.Effect<A, unknown> => {
      const error = catchable(cause);
      if (Option.isNone(error)) return Effect.failCause(cause);
      try {
        return settle(generator.throw(error.value));
      } catch (thrown) {
        return Effect.fail(thrown);
      }
    },
  };
  const started = Effect.suspend(() => {
    try {
      return settle(start(generator));
    } catch (error) {
      return Effect.fail(error);
    }
  });
  return Effect.onExit(started, (exit) =>
    Exit.isSuccess(exit)
      ? Effect.void
      : Effect.asVoid(run<unknown>(generator, (driven) => driven.return(undefined))),
  );
};

export const drive = <A>(generator: Driven<A>): Effect.Effect<A, unknown> =>
  run(generator, (driven) => driven.next());
