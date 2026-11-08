import { Player, Players } from "lyra";

const FIND = "";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in first.");
    return;
  }

  for (const player of yield* Players.getAll()) {
    const at = player.position ? ` at ${player.position.x}, ${player.position.y}` : "";
    console.log(
      `${player.username} (${player.userId}): level ${player.level}, ${player.hp}/${player.maxHp} hp, ${player.state}, cell ${player.cell}${at}`,
    );
  }

  if (FIND) {
    const player = yield* Players.get(FIND);
    console.log(`${FIND}: ${player ? `${player.name} in ${player.cell}` : "not here"}`);
  }
}
