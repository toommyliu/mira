import * as Effect from "effect/Effect";

import { send, type Channel } from "./native.ts";

const failure = (message: string, stack: string | undefined): Error =>
  Object.assign(new Error(message), { stack });

const over =
  (channel: Channel) =>
  <A>(request: object): Effect.Effect<A> => {
    const { stack } = new Error();
    return Effect.callback<A>((resume) => {
      try {
        const cancel = send(channel, request, (settled) => {
          resume(
            "ok" in settled
              ? Effect.succeed(settled.ok as A)
              : Effect.die(failure(settled.error, stack)),
          );
        });
        return Effect.sync(cancel);
      } catch (error) {
        resume(Effect.die(failure(error instanceof Error ? error.message : String(error), stack)));
        return;
      }
    });
  };

export const call = over("call");

export const ask = over("ask");
