package lyra.game {
  import lyra.Main;

  [BridgeNamespace("house")]
  public class House {
    [BridgeExport]
    public static function getItems():Array {
      var rows:Array = [];
      for each (var item:Object in Main.Game.world.myAvatar.houseitems) {
        rows.push(Inventory.row(item));
      }

      return rows;
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var item:Object = ItemLookup.find(Main.Game.world.myAvatar.houseitems, selector);
      return item ? Inventory.row(item) : null;
    }

    [BridgeExport]
    public static function getSlots():int {
      return Main.Game.world.myAvatar.objData.iHouseSlots;
    }

    [BridgeExport]
    public static function getUsedSlots():int {
      return Main.Game.world.myAvatar.houseitems.length;
    }
  }
}
