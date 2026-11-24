import { Player, Wait } from "mira";

export default function* () {
  console.log("Log in within 30 seconds.");
  const ready = yield* Wait.until(Player.isReady, { timeout: "30 seconds" });
  console.log(`ready: ${ready}`);
  if (!ready) return;

  const calm = yield* Wait.until(
    function* () {
      return (yield* Player.getState()) !== "combat";
    },
    { interval: "500 millis", timeout: "10 seconds" },
  );
  console.log(`out of combat: ${calm}`);
}
