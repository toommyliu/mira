import * as Deferred from "effect/Deferred";

import { AbortController } from "./abort.ts";

export const failed = Deferred.makeUnsafe<never, unknown>();

export const stopping = new AbortController();
