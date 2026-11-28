---
title: Loop taunting
order: 9
---

Loop taunting keeps a boss taunted. Players take turns casting Scroll of Enrage on it.

## Start a rotation

Equip Scroll of Enrage on every player in the rotation. Each player must call [`Army.loopTaunt`](/reference/army/#loopTaunt) with the same plan.

```ts
import { Army, Player, Script } from "mira";

export default function* () {
  yield* Army.start("example");
  const rotation = yield* Army.loopTaunt([
    [
      {
        target: "Boss",
        players: [1, 2],
        on: "focus",
        skipWhen: function* () {
          return !(yield* Player.isReady());
        },
      },
    ],
  ]);
  yield* Script.sleep("1 minute");
  yield* rotation.stop();
}
```

## Choose targets and triggers

The outer list orders groups by priority. Only the first group with a living target taunts. Players taunt the targets in that group at the same time. Each target must be a different monster, and a player can have one target per group.

This plan keeps two adds taunted while either is alive. When both are dead, all four players take turns on the boss whenever it says "prepares".

```ts
import { Army, Combat } from "mira";

export default function* () {
  yield* Army.start("example");
  yield* Army.loopTaunt([
    [
      { target: { monMapId: 2 }, players: [1, 2], on: "focus" },
      { target: { monMapId: 3 }, players: [3, 4], on: "focus" },
    ],
    [{ target: "Boss", players: [1, 2, 3, 4], on: { includes: "prepares" } }],
  ]);
  while (true) {
    if (!(yield* Combat.kill("*"))) return;
  }
}
```

Use `on: "focus"` to cast when Focus wears off, or `on: { includes: "message text" }` to cast on a matching combat or server message. A `skipWhen` generator returns `true` to pass the turn. A dead or cooling-down player also passes. The rotation stops when the script ends or the player leaves the map.
