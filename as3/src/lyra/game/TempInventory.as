package lyra.game {
  import lyra.Main;

  [BridgeNamespace("tempInventory")]
  public class TempInventory {
    private static function row(item:Object):Object {
      return {
        itemId: item.ItemID,
        name: item.sName,
        quantity: item.iQty,
        maxStack: item.iStk
      };
    }

    [BridgeExport]
    public static function getItems():Array {
      var rows:Array = [];
      for each (var item:Object in Main.Game.world.myAvatar.tempitems) {
        rows.push(row(item));
      }

      return rows;
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var item:Object = ItemLookup.find(Main.Game.world.myAvatar.tempitems, selector);
      return item ? row(item) : null;
    }
  }
}
