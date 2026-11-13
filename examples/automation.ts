import { Auth, Player, Settings } from "mira";
import * as AutoRelogin from "mira/autorelogin";

export default function* () {
  const server = yield* Auth.getServer();
  const relogin = yield* AutoRelogin.update({ enabled: true, server, delaySeconds: 10 });
  console.log(`auto relogin: ${relogin.server ?? "last server"} after ${relogin.delaySeconds}s`);

  yield* Player.joinMap("queeniona");
  const settings = yield* Settings.update({ autoZone: true });
  console.log(`auto zone: ${settings.autoZone ? "on" : "off"}`);
}
