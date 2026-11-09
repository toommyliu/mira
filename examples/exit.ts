import { Combat, Player } from "mira";

export default function* () {
  yield* Player.joinMap("xantown");
  console.log(`left combat: ${yield* Combat.exit()}`);
}
