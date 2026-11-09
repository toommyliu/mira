import * as Effect from "effect/Effect";

import { send } from "./native.ts";

export const call = <A>(request: object): Effect.Effect<A> =>
  Effect.callback<A>((resume) => {
    const cancel = send(request, (settled) => {
      resume(
        "ok" in settled ? Effect.succeed(settled.ok as A) : Effect.die(new Error(settled.error)),
      );
    });
    return Effect.sync(cancel);
  });
