package lyra.game {
  import lyra.Main;

  [BridgeNamespace("players")]
  public class Players {
    [BridgeIgnore]
    public static function row(leaf:Object):Object {
      return {
        hp: leaf.intHP,
        maxHp: leaf.intHPMax,
        state: leaf.intState,
        cell: leaf.strFrame,
        afk: leaf.afk,
        level: leaf.intLevel,
        mp: leaf.intMP,
        maxMp: leaf.intMPMax,
        pad: leaf.strPad,
        username: leaf.strUsername,
        name: leaf.uoName,
        userId: leaf.entID,
        x: leaf.tx,
        y: leaf.ty,
        auras: Auras.rows(leaf)
      };
    }

    [BridgeExport]
    public static function getAll():Array {
      var rows:Array = [];
      for each (var leaf:Object in Main.Game.world.uoTree) {
        if (leaf) {
          rows.push(row(leaf));
        }
      }

      return rows;
    }

    [BridgeExport]
    public static function get(username:String):Object {
      var leaf:Object = Main.Game.world.uoTree[username.toLowerCase()];
      return leaf ? row(leaf) : null;
    }
  }
}
