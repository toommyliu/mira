import { Player, World } from "lyra";

const MAP_ITEM_ID = 0;
const MAP_SWF = "";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and enter a map first.");
    return;
  }

  console.log(
    `map ${yield* Player.getMap()}, room ${yield* World.getRoomNumber()}, area id ${yield* World.getAreaId()}`,
  );
  console.log(`cells: ${(yield* World.getCells()).join(", ")}`);
  console.log(`pads here: ${(yield* World.getCellPads()).join(", ")}`);

  const monsters = yield* World.getMonsters();
  console.log(`${monsters.length} monsters in the map`);
  for (const monster of monsters) {
    console.log(
      `  ${monster.name} (#${monster.monMapId}) in ${monster.cell}: ${monster.hp}/${monster.maxHp} HP, ${monster.state}`,
    );
  }

  const cell = yield* Player.getCell();
  const here = yield* World.getMonsters(cell === null ? {} : { cell });
  console.log(`${here.length} monsters in ${cell}`);
  const target = yield* World.getMonster("*");
  if (target !== null) {
    console.log(
      `first monster here: ${target.name}, available ${yield* World.isMonsterAvailable({ monMapId: target.monMapId })}`,
    );
    const drops = yield* World.getMonsterDrops({ monMapId: target.monMapId });
    for (const drop of drops ?? []) {
      const rate = drop.rate === null ? "" : ` at ${drop.rate}%`;
      console.log(`  drops ${drop.name} (#${drop.itemId}) x${drop.quantity}${rate}`);
    }
  }

  console.log(`spawn point set: ${yield* World.setSpawnPoint()}`);
  if (MAP_ITEM_ID !== 0) {
    console.log(`picked up map item: ${yield* World.getMapItem(MAP_ITEM_ID)}`);
  }
  if (MAP_SWF !== "") {
    console.log(`loaded ${MAP_SWF}: ${yield* World.loadSwf(MAP_SWF)}`);
  }
  console.log(`reloaded: ${yield* World.reload()}`);
}
