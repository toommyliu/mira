import * as Effect from "effect/Effect";

import { send } from "./native.ts";

const failure = (message: string, stack: string | undefined): Error =>
  Object.assign(new Error(message), { stack });

export const call = <A>(request: object): Effect.Effect<A> => {
  const { stack } = new Error();
  return Effect.callback<A>((resume) => {
    const cancel = send(request, (settled) => {
      resume(
        "ok" in settled
          ? Effect.succeed(settled.ok as A)
          : Effect.die(failure(settled.error, stack)),
      );
    });
    return Effect.sync(cancel);
  });
};
