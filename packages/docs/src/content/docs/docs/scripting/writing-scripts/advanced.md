---
title: Advanced
order: 4
---

Mira runs each script in its own [QuickJS](https://quickjs-ng.github.io/quickjs/) engine, not in Node or a browser. That affects what a run keeps, which APIs a script can use, and when Mira stops a script.

## Use Effect

The script runtime is built on [Effect](https://effect.website/docs). Mira bundles `effect` 4.1.2, so scripts can import it directly. Top-level modules such as `effect/Schedule` work, but nested ones such as `effect/http/HttpClient` don't.

## Every run starts fresh

Mira creates a new engine for each run and discards it when the run ends. Globals, module state, timers, and listeners don't carry over, so you don't need to clear them. Packet and event subscriptions also close with the run.

## Some changes outlast the run

These changes stay after the run ends:

- Drop, quest, and boost registrations and their configs, from [`Drops.register`](/reference/drops/#register), [`Quests.register`](/reference/quests/#register), and `Player.boosts.register`. They last until the session closes.
- Game settings from [`Settings.update`](/reference/settings/#update).

Undo the ones the next script shouldn't inherit in a `finally` block.

## Clean up on stop

When you stop a script, Mira aborts [`Script.signal`](/reference/script/#signal), then runs `finally` blocks and Effect finalizers. Cleanup can still make calls and wait. Mira ends the run if cleanup takes more than 5 seconds.

If the script is in the middle of running code when you stop it, Mira ends it right away and skips cleanup.

```ts
import { Drops } from "mira";

export default function* () {
  yield* Drops.register(["Dragon Scale"]);
  try {
    // farm until stopped
  } finally {
    yield* Drops.unregisterAll();
  }
}
```

## Available globals

Mira adds these globals:

- `console.log`, `console.debug`, `console.info`, `console.warn`, and `console.error`, which write to the **Log** page.
- `setTimeout`, `setInterval`, `clearTimeout`, and `clearInterval`.
- `performance.now()`, which returns the milliseconds since the run started.
- `AbortController` and `AbortSignal`.
- `TextEncoder` and `TextDecoder`, for UTF-8 only.

QuickJS adds `atob`, `btoa`, and `queueMicrotask` to standard JavaScript.

## Missing APIs

Scripts don't have these:

- `fetch`, `XMLHttpRequest`, and `WebSocket`. Use [`mira/http`](/reference/http/) for HTTP requests.
- `Intl`. `toLocaleString` ignores the locale, so `(1234567.5).toLocaleString()` returns `"1234567.5"`.
- `structuredClone`, `URL`, `URLSearchParams`, and `crypto`. Use `Math.random` for random numbers.
- Node's `require`, `process`, `Buffer`, and built-in modules such as `fs`.
- `window`, `document`, `localStorage`, and the rest of the DOM.

Scripts can't import from npm. They can import [`mira`](/reference/), [`mira/http`](/reference/http/), [`mira/schema`](/reference/schema/), `effect` and its top-level modules, [installed packages](/docs/scripting/packages/), and files in their own folder.

## TypeScript syntax

Mira runs scripts without type-checking. Use `import type` for type-only imports.

Use syntax Mira can erase, such as type annotations and interfaces. Declaring an `enum` or `const enum` can cause load errors or incorrect values. Use an `as const` object for named values.

```ts
const Mode = { Hunt: "hunt", Rest: "rest" } as const;
type Mode = (typeof Mode)[keyof typeof Mode];
```

## Limits

Mira stops a script that goes 5 seconds without waiting on a `yield*`. A loop that only computes never waits, so add a `yield* Script.sleep("1 millis")` inside it.

Mira stops a script that uses more than 64 MiB of memory. The call stack is 1 MiB, so deep recursion fails sooner than in Node.
