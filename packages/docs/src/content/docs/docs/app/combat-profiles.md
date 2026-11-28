---
title: Combat profiles
order: 3
---

A combat profile tells Mira which skills to use and when, without writing code. **Auto attack** and `Combat.kill` can use the same saved profile, so both follow the same skill rules.

## Create a profile

1. Open **Auto attack** on the game bar and select **Edit profiles…**.
2. In **Combat profiles**, open the profile menu in the title bar and select **New profile**.
3. Name the profile, configure its skills, and select **Save**.

The same menu switches between profiles. **Duplicate** and **Delete…** at the bottom of the window act on the one shown. Mira edits one profile at a time, so it asks you to save your changes before it shows another. To drop them instead, cancel and select **Revert**, or **Discard** for a profile you haven't saved.

## Use a profile in a script

Pass a saved profile name to [`Combat.kill`](/reference/combat/#kill). Saving changes also updates a running kill. You can define a profile inline instead.

```ts
import { Combat, type Profile } from "mira";

const boss: Profile = {
  onCooldown: "skip",
  triggers: [{ includes: "prepares", skill: 3 }],
  priority: [{ skill: 5, when: [{ type: "hp", of: "me", atMost: "30%" }] }],
  steps: [{ skill: 1 }, { skill: 4, onCooldown: "wait", pauseAfter: "500 millis" }],
};

export default function* () {
  yield* Combat.kill("*", { profile: "Generic" });
  yield* Combat.kill("*", { profile: boss });
}
```

## Choose skills

A [`Profile`](/reference/combat/#Profile) responds to matching combat and server message triggers first. A trigger casts once when its skill is ready, unless three seconds pass after the latest match. Triggers ignore pauses.

On each attempt, Mira checks priority skills from top to bottom. Priority skills never wait for cooldowns. Mira then rotates through steps in order. Set `onCooldown` to `wait` or `skip` for the profile, or override it on a step. `pauseAfter` holds all skills after a cast.

## Add conditions

Every condition in a `when` list must match. Each condition uses either `atMost` or `atLeast`.

| Type   | What it checks                                                               |
| ------ | ---------------------------------------------------------------------------- |
| `hp`   | Health of `me` or `anyAlly`.                                                 |
| `mp`   | Your mana.                                                                   |
| `aura` | An aura by `name` on `me` or `target`. Set `measure` to `stacks` or `value`. |

Health and mana bounds accept points or a percentage such as `"30%"`. Aura bounds use numbers. The default aura measure is `stacks`. A missing aura counts as zero.
