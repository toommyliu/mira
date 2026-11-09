import { Bank, Inventory, Player } from "mira";

const DEPOSIT = "crag & bamboozle";
const SWAP_FOR = "beetle general pet";
const reverse = false;

function* where(name: string) {
  const banked = yield* Bank.contains(name);
  const bagged = yield* Inventory.contains(name);
  return `${name}: ${banked ? "bank" : bagged ? "bag" : "missing"}`;
}

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand somewhere out of combat first.");
    return;
  }

  if (!(yield* Bank.load())) {
    console.log("The bank didn't load.");
    return;
  }

  console.log(
    `bank: ${yield* Bank.getUsedSlots()} of ${yield* Bank.getSlots()} used, ${yield* Bank.getAvailableSlots()} free`,
  );
  for (const item of yield* Bank.getItems()) {
    console.log(`${item.name} x${item.quantity}/${item.maxStack} [${item.pool}]`);
  }

  console.log(`open: ${yield* Bank.open()}, isOpen: ${yield* Bank.isOpen()}`);

  const [fromBag, fromBank] = reverse ? [DEPOSIT, SWAP_FOR] : [SWAP_FOR, DEPOSIT];
  console.log(`before: ${yield* where(DEPOSIT)}, ${yield* where(SWAP_FOR)}`);
  console.log(`swap ${fromBag} for ${fromBank}: ${yield* Bank.swap(fromBag, fromBank)}`);
  console.log(`after: ${yield* where(DEPOSIT)}, ${yield* where(SWAP_FOR)}`);
}
