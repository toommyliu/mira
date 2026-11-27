---
title: Army scripts
order: 8
---

An army is a group of accounts that run the same script together. Each account does its part and waits for the others at the points you choose.

## Create an army config

Create `~/Mira/armies/example.yaml` with the accounts in player order and a private room number. Quote the room number.

```yaml
players:
  - FirstPlayer
  - SecondPlayer
room: "1234"
items:
  tank: Legion Revenant
sets:
  battle:
    default:
      class: tank
    player2:
      class: Lord of Order
```

## Define gear sets

Gear sets use two keys in the army config:

- `items` maps an alias, such as `tank`, to a gear name. Any gear field can use the alias.
- `sets` names each set. A set's `default` gear applies to every player. `player1`, `player2`, and later keys override only the fields they set.

A gear entry accepts `class`, `weapon`, `cape`, `helm`, `armor`, `pet`, `pots`, `scroll`, `safeClass`, and `safePot`. `pots` takes a list.

## Start the army

Start the same script on every account. You can use a setup in **Manager**.

```ts
import { Army } from "mira";

export default function* () {
  const army = yield* Army.start("example");
  console.log(`player ${army.number} of ${army.size}`);
  if (!(yield* Army.joinMap("battleontown"))) return;
  yield* Army.equipSet("battle");
}
```

## Sync and gather

[`Army.sync`](/reference/army/#sync) waits until every player reaches the same label. [`Army.gather`](/reference/army/#gather) also waits, then returns one value from each player in player order. Every player gets the same list, so every player makes the same decision.

```ts
import { Army, Inventory } from "mira";

export default function* () {
  yield* Army.start("example");
  const potions = yield* Army.gather("potions", yield* Inventory.getQuantity("Healer Elixir"));
  if (potions.some((count) => count < 5)) {
    console.log("a player is low on potions");
    return;
  }
  yield* Army.sync("ready");
}
```

Without a timeout, a step waits forever. Pass `{ timeout: "30 seconds" }` to end the army when a player doesn't arrive in time.

## Coordinate work in rounds

[`Army.until`](/reference/army/#until) runs `work` on every player in rounds until `done` returns `true` for all of them at the same time. A player that is done keeps working. Rounds start at least 500 ms apart. Set `interval` to change the gap.

```ts
import { Army, Combat } from "mira";

export default function* () {
  yield* Army.start("example");
  let kills = 0;
  yield* Army.until("three kills each", {
    done: function* () {
      return kills >= 3;
    },
    work: function* () {
      if (yield* Combat.kill("*")) kills += 1;
    },
  });
}
```

## Take turns

[`Army.takeTurns`](/reference/army/#takeTurns) runs `turn` on one player at a time, in the order of `players`. Every listed player must call it with the same label, and turns start once they all have. `turn` returns whether the player acted. If no player acts in a full lap, the next turn waits 1 second. Turns run in the background until you call `stop` or the script ends.

```ts
import { Army, Combat } from "mira";

export default function* () {
  yield* Army.start("example");
  const rotation = yield* Army.takeTurns("skill 3", {
    players: [1, 2],
    turn: function* () {
      return yield* Combat.useSkill(3, { target: "Boss" });
    },
  });
  yield* Combat.kill("Boss");
  yield* rotation.stop();
}
```
