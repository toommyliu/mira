import { Combat, Player, type Profile } from "mira";

const BOSS: Profile = {
  onCooldown: "skip",
  triggers: [{ includes: "prepares", skill: 3 }],
  priority: [
    { skill: 5, when: [{ type: "hp", of: "me", atMost: "30%" }] },
    { skill: 2, when: [{ type: "hp", of: "anyAlly", atMost: "60%" }] },
  ],
  steps: [
    { skill: 1 },
    {
      skill: 4,
      onCooldown: "wait",
      pauseAfter: "500 millis",
      when: [{ type: "aura", on: "target", name: "Burn", atMost: 0 }],
    },
  ],
};

export default function* () {
  if (!(yield* Player.isReady())) {
    console.log("Log in and stand in a cell with monsters first.");
    return;
  }

  console.log(`with Generic: ${yield* Combat.kill("*", { profile: "Generic" })}`);
  console.log(`with an inline profile: ${yield* Combat.kill("*", { profile: BOSS })}`);
}
