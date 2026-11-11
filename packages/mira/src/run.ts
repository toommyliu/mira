import * as Deferred from "effect/Deferred";

import { AbortController } from "./abort.ts";

export type ExitRequest = { readonly logout: boolean; readonly closeTab: boolean };

export const failed = Deferred.makeUnsafe<never, unknown>();

export const exiting: { request?: ExitRequest } = {};

export const exited = Deferred.makeUnsafe<void>();

export const stopping = new AbortController();
