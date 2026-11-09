import { Auth, Player } from "mira";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in first.");
    return;
  }

  const current = yield* Auth.getServer();
  const member = yield* Player.isMember();
  console.log(`${yield* Auth.getUsername()} on ${current}`);

  yield* Auth.logout();
  if (!(yield* Auth.login())) {
    console.log("Couldn't log back in.");
    return;
  }

  const servers = (yield* Auth.getServers())
    .filter(
      (server) =>
        server.online &&
        server.name !== current &&
        server.players < server.maxPlayers &&
        (member || !server.membersOnly),
    )
    .sort((a, b) => a.players - b.players);
  for (const server of servers) {
    const result = yield* Auth.connect(server.name);
    if (result.ok) {
      console.log(`now on ${yield* Auth.getServer()}`);
      return;
    }
    console.log(`${server.name}: ${result.reason}, ${result.message}`);
  }
  console.log("No other server let us in.");
}
