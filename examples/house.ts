import { House, Player } from "mira";

const FIND = "";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in first.");
    return;
  }

  console.log(
    `house: ${yield* House.getUsedSlots()} of ${yield* House.getSlots()} used, ${yield* House.getAvailableSlots()} free`,
  );
  for (const item of yield* House.getItems()) {
    console.log(
      `${item.name} x${item.quantity} [${item.category}, ${item.pool}${item.equipped ? ", equipped" : ""}]`,
    );
  }

  if (FIND) {
    const item = yield* House.getItem(FIND);
    console.log(
      `${FIND}: ${item ? `${item.name} (${item.itemId})` : "not found"}, quantity ${yield* House.getQuantity(FIND)}, contains ${yield* House.contains(FIND)}`,
    );
  }
}
