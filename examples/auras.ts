import { type Aura, Combat, Player, Players, World } from "mira";

const FIND = "";

const describe = (aura: Aura) => {
  const left =
    aura.expiresAt === null ? "" : `, ${Math.max(0, aura.expiresAt - Date.now())} ms left`;
  return `${aura.name} x${aura.stack}${aura.value === null ? "" : ` = ${aura.value}`}${left}`;
};

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in first.");
    return;
  }

  for (const aura of yield* Player.getAuras()) console.log(`me: ${describe(aura)}`);

  const target = yield* Combat.getTarget();
  for (const aura of target?.auras ?? []) console.log(`target ${target?.name}: ${describe(aura)}`);

  for (const monster of yield* World.getMonsters()) {
    for (const aura of monster.auras)
      console.log(`${monster.name} ${monster.monMapId}: ${describe(aura)}`);
  }

  for (const player of yield* Players.getAll()) {
    for (const aura of player.auras) console.log(`${player.username}: ${describe(aura)}`);
  }

  if (FIND) {
    const aura = yield* Player.getAura(FIND);
    console.log(
      `${FIND}: ${aura ? describe(aura) : "not found"}, has ${yield* Player.hasAura(FIND)}`,
    );
  }
}
