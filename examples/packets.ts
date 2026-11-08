import { Effect } from "effect";
import { Packets, Player, Wait } from "lyra";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and enter a map first.");
    return;
  }

  yield* Packets.on({ from: "client" }, (packet) =>
    Effect.sync(() => console.log(`[cb] sent ${packet.cmd} ${packet.args.join(", ")}`)),
  );

  const arrival = yield* Wait.forPacket(
    { from: "server", cmd: "moveToArea" },
    { trigger: Player.joinMap("yulgar"), timeout: "10 seconds" },
  );
  console.log(`moveToArea arrived: ${arrival !== null}`);

  console.log(`sent afk: ${yield* Packets.sendToServer("%xt%zm%afk%1%false%")}`);
  yield* Effect.sleep("5 seconds");
  console.log(`sent afk: ${yield* Packets.sendToServer("%xt%zm%afk%1%true%")}`);
}
