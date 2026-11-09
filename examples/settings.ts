import { Settings } from "mira";

export default function* () {
  const before = yield* Settings.get();
  console.log(`walk speed ${before.walkSpeed}, ads ${before.ads ? "shown" : "hidden"}`);

  const after = yield* Settings.update({ ads: false, hidePlayers: true, walkSpeed: 12 });
  console.log(`walk speed ${after.walkSpeed}, ads ${after.ads ? "shown" : "hidden"}`);
}
