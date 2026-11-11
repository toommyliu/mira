package mira.game {
  public class GearBoosts {
    private static const EFFECTS:Object = {
      dmgall: {kind: "damage", against: "all"},
      undead: {kind: "damage", against: "undead"},
      human: {kind: "damage", against: "human"},
      chaos: {kind: "damage", against: "chaos"},
      dragonkin: {kind: "damage", against: "dragonkin"},
      orc: {kind: "damage", against: "orc"},
      drakath: {kind: "damage", against: "drakath"},
      elemental: {kind: "damage", against: "elemental"},
      cp: {kind: "classPoints"},
      gold: {kind: "gold"},
      rep: {kind: "rep"},
      exp: {kind: "xp"}
    };

    public static function parse(meta:*):Array {
      var boosts:Array = [];
      for each (var entry:String in String(meta || "").split(",")) {
        var parts:Array = entry.split(":");
        var key:String = String(parts[0]).replace(/^\s+|\s+$/g, "").toLowerCase();
        var multiplier:Number = Number(parts[1]);
        if (!EFFECTS.hasOwnProperty(key) || !(multiplier > 1)) {
          continue;
        }

        var effect:Object = EFFECTS[key];
        var boost:Object = {kind: effect.kind, percent: Math.round((multiplier - 1) * 100)};
        if (effect.against) {
          boost.against = effect.against;
        }
        boosts.push(boost);
      }

      return boosts;
    }
  }
}
