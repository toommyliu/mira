import { Player, type Meta } from "mira";
import { eldersBlood } from "@mira/dailies";

export const meta = {
  name: "Elders' Blood",
  description: "Completes the Elders' Blood daily unless maxed.",
} satisfies Meta;

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and join a map first.");
    return;
  }
  const result = yield* eldersBlood();
  switch (result.status) {
    case "completed": {
      const limit = result.maxStack === null ? "" : `/${result.maxStack}`;
      console.log(`Got Elders' Blood, now ${result.quantity}${limit}.`);
      break;
    }
    case "maxed":
      console.log(`You already have ${result.quantity}/${result.maxStack} Elders' Blood.`);
      break;
    case "unavailable":
      console.log("Elders' Blood isn't available again yet.");
      break;
    default: {
      const _exhaustive: never = result;
    }
  }
}
