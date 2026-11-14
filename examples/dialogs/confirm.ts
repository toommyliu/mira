import { Effect } from "effect";
import { Script } from "mira";

export default function* () {
  const plain = yield* Script.confirm("Press Confirm or Enter.");
  console.log(`1. confirm: ${plain}`);

  const cancelled = yield* Script.confirm("Press Cancel or Escape.");
  console.log(`2. cancel: ${cancelled}`);

  const labelled = yield* Script.confirm("Your bank is full. Deposit AC items?", {
    title: "Bank",
    confirm: "Deposit",
    cancel: "Skip",
  });
  console.log(`3. deposit: ${labelled}`);

  const answers = yield* Effect.all(
    [Script.confirm("First of two. Answer this one."), Script.confirm("Second of two.")],
    { concurrency: "unbounded" },
  );
  console.log(`4. queued: ${answers.join(", ")}`);

  const late = yield* Script.confirm("Don't answer. This closes in 5 seconds.").pipe(
    Effect.timeoutOption("5 seconds"),
  );
  console.log(`5. ${late._tag === "None" ? "timed out" : `answered ${late.value}`}`);
}
