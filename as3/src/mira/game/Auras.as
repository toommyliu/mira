package mira.game {
  import flash.utils.getTimer;

  public class Auras {
    private static function applied(leaf:Object, name:String):Object {
      for each (var aura:Object in leaf.auras) {
        if (aura != null && aura.nam == name) {
          return aura;
        }
      }

      return null;
    }

    public static function rows(leaf:Object):Array {
      var rows:Array = [];
      if (leaf == null) {
        return rows;
      }

      var now:Number = new Date().getTime();
      var clock:int = getTimer();
      for (var name:String in leaf.hudAuras) {
        var hud:Object = leaf.hudAuras[name];
        if (hud == null) {
          continue;
        }

        var aura:Object = applied(leaf, name);
        rows.push({
          seq: hud.seq,
          name: name,
          value: aura ? aura.val : null,
          stack: hud.n,
          category: aura ? aura.cat : null,
          icon: hud.icon,
          persistent: hud.persist == true,
          duration: hud.dur,
          expiresAt: hud.end > 0 ? now + (hud.end - clock) : null
        });
      }
      rows.sortOn("seq", Array.NUMERIC);

      return rows;
    }
  }
}
