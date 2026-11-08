import { Player, Quests } from "lyra";

const QUEST = 6552;
const SILENT = false;
const BURST = [8152, 8153, 8154];

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand somewhere out of combat first.");
    return;
  }

  for (const id of BURST) {
    const started = Date.now();
    const loaded = yield* Quests.load(id, { silent: SILENT });
    console.log(`load ${id}: ${loaded} after ${Date.now() - started} ms`);
  }
  console.log(
    `accept ${BURST.join(", ")}: ${yield* Quests.acceptBatch(BURST, { silent: SILENT })}`,
  );

  if (!(yield* Quests.load(QUEST, { silent: SILENT }))) {
    console.log(`Quest ${QUEST} didn't load.`);
    return;
  }

  const quest = yield* Quests.get(QUEST);
  if (!quest) return;
  console.log(`${quest.name} (${quest.id}) cadence ${quest.cadence}, once ${quest.once}`);
  for (const need of quest.requirements) {
    console.log(`  needs ${need.name} x${need.quantity}`);
  }
  for (const reward of quest.rewards) {
    console.log(`  ${reward.kind} ${reward.name} x${reward.quantity} (${reward.dropChance}%)`);
  }

  console.log(
    `available ${yield* Quests.isAvailable(QUEST)}, in progress ${yield* Quests.isInProgress(QUEST)}`,
  );
  console.log(`accept: ${yield* Quests.accept(QUEST, { silent: SILENT })}`);
  console.log(
    `accepted: ${(yield* Quests.getAccepted()).map((accepted) => accepted.id).join(", ")}`,
  );
  console.log(
    `can complete ${yield* Quests.canComplete(QUEST)}, max turn-ins ${yield* Quests.getMaxTurnIns(QUEST)}`,
  );

  const choice = quest.rewards.find((reward) => reward.kind === "choice");
  if (choice) {
    console.log(`complete without a pick: ${yield* Quests.complete(QUEST)}`);
    console.log(
      `complete picking ${choice.name}: ${yield* Quests.complete(QUEST, { rewardItemId: choice.itemId })}`,
    );
  } else {
    console.log(`complete: ${yield* Quests.complete(QUEST)}`);
  }
  console.log(`in progress after: ${yield* Quests.isInProgress(QUEST)}`);
}
