import { Script } from "mira";

export default function* () {
  const monster = yield* Script.prompt("Which monster? Type a name and press Enter.", {
    placeholder: "Frogzard",
  });
  console.log(`1. monster: ${JSON.stringify(monster)}`);

  const kills = yield* Script.prompt("How many kills?", { title: "Hunt", default: "100" });
  const count = Number(kills);
  console.log(
    `2. kills: ${JSON.stringify(kills)} (${kills !== null && Number.isInteger(count) ? count : "not a whole number"})`,
  );

  const empty = yield* Script.prompt("Clear the box and press OK.", { default: "delete me" });
  console.log(`3. empty: ${JSON.stringify(empty)}`);

  const cancelled = yield* Script.prompt("Press Cancel or Escape.");
  console.log(`4. cancelled: ${JSON.stringify(cancelled)}`);
}
