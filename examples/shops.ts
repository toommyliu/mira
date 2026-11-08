import { Inventory, Player, Shops } from "lyra";

const MAP = "arcangrove";
const SHOP_ID = 211;

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand somewhere out of combat first.");
    return;
  }

  if (!(yield* Player.joinMap(MAP))) {
    console.log(`Couldn't join ${MAP}.`);
    return;
  }

  if (!(yield* Shops.load(SHOP_ID))) {
    console.log(`Shop ${SHOP_ID} didn't open.`);
    return;
  }

  const shop = yield* Shops.getCurrent();
  console.log(`${shop?.name} (${shop?.id}), merge: ${shop?.merge}, limited: ${shop?.limited}`);
  let pick: { readonly shopItemId: number } | null = null;
  for (const item of yield* Shops.getItems()) {
    const needs = item.requirements.map((need) => `${need.name} x${need.quantity}`).join(", ");
    const buyable = yield* Shops.canBuy({ shopItemId: item.shopItemId });
    console.log(
      `[${item.shopItemId}] ${item.name} x${item.quantity}: ${item.cost} ${item.coins ? "AC" : "gold"}${needs ? `, needs ${needs}` : ""}, canBuy ${buyable}`,
    );
    if (buyable && !item.coins && pick === null) {
      pick = { shopItemId: item.shopItemId };
    }
  }

  if (pick === null) {
    console.log("Nothing here is buyable with gold.");
    return;
  }

  const item = yield* Shops.getItem(pick);
  if (item === null) {
    return;
  }
  console.log(`${item.name}: max ${yield* Shops.getMaxBuyQuantity(pick)}`);
  console.log(`before: ${yield* Inventory.getQuantity(item.name)}`);
  console.log(`buy: ${yield* Shops.buy(pick)}`);
  console.log(`after buying: ${yield* Inventory.getQuantity(item.name)}`);
  console.log(`sell one: ${yield* Shops.sell(item.name, { quantity: 1 })}`);
  console.log(`after selling: ${yield* Inventory.getQuantity(item.name)}`);
  console.log(`close: ${yield* Shops.close()}, isOpen: ${yield* Shops.isOpen()}`);
}
