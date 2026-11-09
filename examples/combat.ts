import { Combat, Player } from "mira";

const MONSTER = "*";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand in a cell with monsters first.");
    return;
  }

  console.log(`killed ${MONSTER}: ${yield* Combat.kill(MONSTER)}`);

  for (const skill of [1, 2, 3, 4] as const) {
    console.log(
      `skill ${skill}: usable ${yield* Combat.canUseSkill(skill)}, cooldown ${yield* Combat.getSkillCooldown(skill)} ms`,
    );
  }

  console.log(`attacking ${MONSTER}: ${yield* Combat.attack(MONSTER)}`);
  console.log(`target ${JSON.stringify(yield* Combat.getTarget())}`);
  console.log(`skill 1 once ready: ${yield* Combat.useSkill(1, { waitUntilReady: true })}`);
  yield* Combat.cancelTarget();
  console.log(`target after cancelling: ${JSON.stringify(yield* Combat.getTarget())}`);;
}
