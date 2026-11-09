import { Army, Combat, Player } from "lyra";

export default function* () {
  const army = yield* Army.start("example");
  console.log(`player ${army.number} of ${army.size}, room ${army.room}`);

  if (!(yield* Army.joinMap("battleontown"))) {
    console.log("the army couldn't get into the same room");
    return;
  }

  const classes = yield* Army.gather("classes", yield* Player.getClassName());
  console.log(`classes: ${classes.join(", ")}`);

  let kills = 0;
  yield* Army.until("three kills each", {
    done: function* () {
      return kills >= 3;
    },
    work: function* () {
      if (yield* Combat.kill("*")) kills += 1;
    },
  });

  yield* Army.sync("finished");
  console.log(`finished with ${kills} kills`);
}
