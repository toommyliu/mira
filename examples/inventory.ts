import { Inventory, Player, TempInventory } from "lyra";

const HELM = "unidentified 9";
const CONSUMABLE = "potent soul potion";
const TONIC = "potent soul potion";

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand somewhere out of combat first.");
    return;
  }

  const slots = yield* Inventory.getSlots();
  for (const pool of ["bag", "misc"] as const) {
    console.log(
      `${pool}: ${yield* Inventory.getUsedSlots(pool)} of ${slots} used, ${yield* Inventory.getAvailableSlots(pool)} free`,
    );
  }

  for (const item of yield* Inventory.getItems()) {
    const flags = [
      item.equipped && "equipped",
      item.worn && "worn",
      item.classRank !== null && `rank ${item.classRank}`,
      item.enhancement && `enhancement ${item.enhancement.id} lvl ${item.enhancement.level}`,
    ]
      .filter(Boolean)
      .join(", ");
    console.log(`${item.name} x${item.quantity}/${item.maxStack} [${item.pool}] ${flags}`);
  }
  for (const item of yield* TempInventory.getItems()) {
    console.log(`temp: ${item.name} x${item.quantity}/${item.maxStack}`);
  }

  if (HELM) {
    console.log(`unequip ${HELM}: ${yield* Inventory.unequip(HELM)}`);
    console.log(`equip ${HELM}: ${yield* Inventory.equip(HELM)}`);
  }
  if (CONSUMABLE) {
    console.log(`equip ${CONSUMABLE}: ${yield* Inventory.equip(CONSUMABLE)}`);
  }
  if (TONIC) {
    console.log(`use ${TONIC}: ${yield* Inventory.use(TONIC)}`);
  }
}
