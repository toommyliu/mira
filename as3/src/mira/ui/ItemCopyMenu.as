package mira.ui {
  import flash.events.MouseEvent;
  import flash.system.ApplicationDomain;

  import mira.Main;
  import mira.game.DropList;
  import mira.game.Inventory;
  import mira.game.Shops;

  public class ItemCopyMenu {
    private static var rowClass:Class;
    private static var listRowClass:Class;
    private static var dropRowClass:Class;
    private static var dropFrameClass:Class;

    public static function init(domain:ApplicationDomain):void {
      rowClass = domain.getDefinition("LPFElementInvAQRow") as Class;
      listRowClass = domain.getDefinition("LPFElementListItemItem") as Class;
      dropRowClass = domain.getDefinition("liteAssets.draw.dEntry") as Class;
      dropFrameClass = domain.getDefinition("DFrame2MC") as Class;
    }

    public static function listenerFor(target:Object):Object {
      return target is rowClass || target is listRowClass || target is dropRowClass || target is dropFrameClass ? target : null;
    }

    public static function optionsAt(row:Object, event:MouseEvent):Array {
      if (row is dropRowClass || row is dropFrameClass) {
        var drop:Object = row is dropRowClass ? DropList.customEntry(row.itemObj) : DropList.frameEntry(row);
        return drop ? options(drop.item, DropList.row(drop)) : [];
      }

      var item:Object = row is rowClass ? row.item : row.fData;
      return item ? options(item) : [];
    }

    private static function options(item:Object, copied:Object = null):Array {
      var found:Array = [
        CopyMenu.option("Copy Name", String(item.sName)),
        CopyMenu.option("Copy Item ID", String(item.ItemID))
      ];

      var shop:Object = Main.Game.world.shopinfo;
      var sold:Boolean = shop && shop.items is Array && shop.items.indexOf(item) != -1;
      if (sold) {
        found.push(
          CopyMenu.option("Copy Shop Item ID", String(item.ShopItemID)),
          CopyMenu.option("Copy Shop ID", String(shop.ShopID))
        );
      }

      var row:Object = copied || (sold ? Shops.getItem({shopItemId: item.ShopItemID}) : Inventory.row(item));
      found.push(CopyMenu.option("Copy Item", JSON.stringify(row, null, 2), "item " + item.sName));
      return found;
    }
  }
}
