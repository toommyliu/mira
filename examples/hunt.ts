import { Combat, Player, Script, type Inputs, type Meta } from "mira";

export const meta = {
  name: "Hunt",
  description: "Kills a monster until you reach the kill count, waiting out respawns.",
} satisfies Meta;

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
      console.log(`couldn't reach a ${monster} in the map`);
      return;
    }
    console.log(`hunting ${prey.name} (#${prey.monMapId}) in ${prey.cell}`);
    if (!(yield* Combat.kill({ monMapId: prey.monMapId }))) return;
    kills += 1;
    console.log(`killed ${kills}/${goal}`);
  }
}
