import { Player } from "mira";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and enter a map first.");
    return;
  }

  console.log(
    `map ${yield* Player.getMap()}, cell ${yield* Player.getCell()}, pad ${yield* Player.getPad()}`,
  );
  console.log(`position ${JSON.stringify(yield* Player.getPosition())}`);
  console.log(
    `hp ${yield* Player.getHp()}/${yield* Player.getMaxHp()}, mp ${yield* Player.getMp()}/${yield* Player.getMaxMp()}`,
  );
  console.log(`state ${yield* Player.getState()}, alive ${yield* Player.isAlive()}`);
  console.log(
    `level ${yield* Player.getLevel()}, gold ${yield* Player.getGold()}, class ${yield* Player.getClassName()}`,
  );
  console.log(`user ${yield* Player.getUserId()}, character ${yield* Player.getCharId()}`);
  console.log(`member ${yield* Player.isMember()}, afk ${yield* Player.isAfk()}`);
  for (const faction of yield* Player.getFactions()) {
    console.log(`faction ${faction.name}: rank ${faction.rank}, ${faction.rep} rep`);
  }
  for (const kind of ["gold", "xp", "rep", "classPoints"] as const) {
    console.log(`${kind} boost ${yield* Player.boosts.isActive(kind)}`);
  }

  console.log(
    `joined battleon: ${yield* Player.joinMap("battleon-12345", { cell: "Enter", pad: "Spawn" })}`,
  );
  console.log(`jumped to Enter: ${yield* Player.jumpToCell("Enter", { pad: "Spawn" })}`);
  console.log(`walked: ${yield* Player.walkTo(480, 330)}`);
  console.log(`rested until full: ${yield* Player.rest({ untilFull: true })}`);

  const outfits = yield* Player.outfits.getNames();
  console.log(`outfits: ${outfits.join(", ") || "none"}`);
  const outfit = outfits[0];
  if (outfit === undefined) return;
  console.log(`equipped ${outfit}: ${yield* Player.outfits.equip(outfit, { keepColors: true })}`);
  console.log(`wore ${outfit}: ${yield* Player.outfits.wear(outfit, { keepColors: true })}`);
}
