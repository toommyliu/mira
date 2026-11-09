import { Combat, Player, Script } from "mira";

const MONSTER = "Frogzard";
const KILLS = 5;

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log(`Log in and join a map with ${MONSTER} first.`);
    return;
  }

  let kills = 0;
  while (kills < KILLS) {
    const prey = yield* Combat.hunt(MONSTER, { preferMostMatches: true });
    if (prey === null) {
      console.log(`no live ${MONSTER} in the map, waiting for a respawn`);
      yield* Script.sleep("2 seconds");
      continue;
    }
    console.log(`hunting ${prey.name} (#${prey.monMapId}) in ${prey.cell}`);
    if (yield* Combat.kill({ monMapId: prey.monMapId })) {
      kills += 1;
      console.log(`killed ${kills}/${KILLS}`);
    }
  }
}
