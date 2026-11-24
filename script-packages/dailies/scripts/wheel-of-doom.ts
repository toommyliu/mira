import { Player, Script, type Inputs, type Meta } from "mira";
import { wheelOfDoom, type Spin } from "@mira/dailies";

export const meta = {
  name: "Wheel of Doom",
  description: "Spins the daily and weekly Wheel of Doom.",
} satisfies Meta;

export const inputs = {
  bankRewards: {
    type: "boolean",
    label: "Bank rewards",
    description: "Deposit what each spin wins.",
    default: false,
  },
} as const satisfies Inputs;

const describe = (spin: Spin): string => {
  switch (spin.status) {
    case "spun": {
      const won = spin.won.join(", ") || "nothing";
      return spin.banked ? `won and banked ${won}` : `won ${won}`;
    }
    case "already-spun":
      return "already spun";
    case "members-only":
      return "members only";
    case "needs-gear":
      return `needs 3 Gear of Doom, you have ${spin.gears}`;
    default: {
      const _exhaustive: never = spin;
      return _exhaustive;
    }
  }
};

export default function* () {
  const { bankRewards } = Script.inputs<typeof inputs>();
  if (!(yield* Player.isReady())) {
    console.log("Log in and join a map first.");
    return;
  }
  const { daily, weekly } = yield* wheelOfDoom({ bankRewards });
  console.log(`Daily spin: ${describe(daily)}.`);
  console.log(`Weekly spin: ${describe(weekly)}.`);
}
