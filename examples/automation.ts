import { Auth, Player } from "mira";
import * as AutoRelogin from "mira/autorelogin";
import * as AutoZone from "mira/autozone";

export default function* () {
  const server = yield* Auth.getServer();
  const relogin = yield* AutoRelogin.update({ enabled: true, server, delaySeconds: 10 });
  console.log(`auto relogin: ${relogin.server ?? "last server"} after ${relogin.delaySeconds}s`);

  yield* Player.joinMap("queeniona");
  const zone = yield* AutoZone.update({ enabled: true, map: "queeniona" });
  console.log(`auto zone: ${zone.map}`);
}
