import * as Deferred from "effect/Deferred";

export const failed = Deferred.makeUnsafe<never, unknown>();
