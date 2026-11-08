import { Effect } from "effect";
import { Combat, Events, Packets, Player, Wait } from "lyra";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand in a cell with monsters first.");
    return;
  }

  yield* Events.on({ type: "afk" }, ({ username, afk }) =>
    Effect.sync(() => console.log(`${username} afk ${afk}`)),
  );
  yield* Events.on({ type: "afk", self: true, afk: true }, () =>
    Packets.sendToServer("%xt%zm%afk%1%false%"),
  );
  yield* Events.on({ type: "itemDrop" }, (drop) =>
    Effect.sync(() => console.log(`dropped ${drop.quantity} ${drop.name}`)),
  );
  yield* Events.on({ type: "zone" }, ({ zone }) =>
    Effect.sync(() => console.log(`zone ${zone === "" ? "default" : zone}`)),
  );

  const death = yield* Wait.forEvent(
    { type: "monsterDeath" },
    { trigger: Combat.attack("*"), timeout: "60 seconds" },
  );
  console.log(death === null ? "nothing died in time" : `monster ${death.monMapId} died`);
  yield* Effect.sleep("10 seconds");
}
