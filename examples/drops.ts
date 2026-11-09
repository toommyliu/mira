import { Drops, Inventory, Player } from "mira";

const ACCEPT = "";
const REJECT = "";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand somewhere out of combat first.");
    return;
  }

  for (const item of yield* Drops.getItems()) {
    console.log(
      `waiting: ${item.name} x${item.quantity} [${item.category}${item.coins ? ", AC" : ""}]`,
    );
  }

  if (ACCEPT) {
    console.log(`accept ${ACCEPT}: ${yield* Drops.accept(ACCEPT)}`);
    console.log(
      `still waiting: ${yield* Drops.contains(ACCEPT)}, in bag: ${yield* Inventory.contains(ACCEPT)}`,
    );
  }
  if (REJECT) {
    console.log(`reject ${REJECT}: ${yield* Drops.reject(REJECT)}`);
    console.log(`still waiting: ${yield* Drops.contains(REJECT)}`);
  }
}
