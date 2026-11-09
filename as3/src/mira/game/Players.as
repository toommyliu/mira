package mira.game {
  import mira.Main;

  [BridgeNamespace("players")]
  public class Players {
    [BridgeIgnore]
    public static function row(leaf:Object):Object {
      var avatar:Object = Main.Game.world.avatars[leaf.entID];
      var shown:Object = avatar && avatar.pMC && avatar.pMC.visible ? avatar.pMC : null;
      var reported:Boolean = leaf.tx != 0 || leaf.ty != 0;
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
        x: shown ? shown.x : (reported ? leaf.tx : null),
        y: shown ? shown.y : (reported ? leaf.ty : null),
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
