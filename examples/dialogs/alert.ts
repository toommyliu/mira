import { Effect, Fiber } from "effect";
import { Script } from "mira";

export default function* () {
  yield* Script.alert("Press OK, Enter or Escape to continue.");
  console.log("1. closed the first alert");

  yield* Script.alert("This one has its own title.\nAnd a second line.", { title: "Heads up" });
  console.log("2. closed the titled alert");

  const ticking = yield* Effect.forkChild(
    Effect.forever(
      Effect.sleep("1 second").pipe(Effect.andThen(Effect.sync(() => console.log("tick")))),
    ),
  );
  yield* Script.alert("Ticks keep logging while this is open. Wait a few seconds, then close it.");
  yield* Fiber.interrupt(ticking);
  console.log("3. stopped ticking");

  const closed = yield* Script.alert("This closes by itself in 5 seconds.").pipe(
    Effect.timeoutOption("5 seconds"),
  );
  console.log(`4. ${closed._tag === "None" ? "timed out" : "closed by you"}`);

  yield* Script.alert("Press Stop script. The script should end as stopped, not finished.");
  console.log("5. you closed it instead of stopping");
}
