import { Combat, Player, Script, type Inputs } from "mira";

export const inputs = {
  monster: {
    type: "string",
    label: "Monster",
    description: "Name as it appears in game.",
    required: true,
    default: "Frogzard",
  },
  kills: { type: "number", label: "Kills", integer: true, min: 1, default: 5 },
} as const satisfies Inputs;

export default function* () {
  const { monster, kills: goal } = Script.inputs<typeof inputs>();
  if (!(yield* Player.isReady())) {
    console.log(`Log in and join a map with ${monster} first.`);
    return;
  }

  let kills = 0;
  while (kills < goal) {
    const prey = yield* Combat.hunt(monster, { preferMostMatches: true });
    if (prey === null) {
      console.log(`no live ${monster} in the map, waiting for a respawn`);
      yield* Script.sleep("2 seconds");
      continue;
    }
    console.log(`hunting ${prey.name} (#${prey.monMapId}) in ${prey.cell}`);
    if (yield* Combat.kill({ monMapId: prey.monMapId })) {
      kills += 1;
      console.log(`killed ${kills}/${goal}`);
    }
  }
}
