import { Combat, Drops, Player, Quests, Script } from "mira";

const ITEMS = [
  "Unidentified 13",
  "Unidentified 10",
  "Tainted Gem",
  "Dark Crystal Shard",
  "Diamond of Nulgath",
  "Voucher of Nulgath (non-mem)",
  "Gem of Nulgath",
  "Essence of Nulgath",
];
const QUESTS = [609, 2857];

export default function* () {
  yield* Drops.register(ITEMS);
  yield* Quests.register(QUESTS);
  yield* Quests.configure({ acceptRequirements: true });
  yield* Drops.configure({ rejectOthers: true });

  if (!(yield* Player.joinMap("evilmarsh-100000", { cell: "End", pad: "Left" }))) {
    console.log("Couldn't join evilmarsh. Log in first.");
    return;
  }
  while (!Script.signal.aborted) {
    yield* Combat.kill("Tainted Elemental");
  }
}
