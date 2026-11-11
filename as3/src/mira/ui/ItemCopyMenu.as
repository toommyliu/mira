package mira.ui {
  import flash.events.MouseEvent;
  import flash.system.ApplicationDomain;

  import mira.Main;
  import mira.game.Inventory;
  import mira.game.Shops;

  public class ItemCopyMenu {
    private static var rowClass:Class;
    private static var listRowClass:Class;

    public static function init(domain:ApplicationDomain):void {
      rowClass = domain.getDefinition("LPFElementInvAQRow") as Class;
      listRowClass = domain.getDefinition("LPFElementListItemItem") as Class;
    }

    public static function covers(target:Object):Boolean {
      return target is rowClass || target is listRowClass;
    }

    public static function optionsAt(row:Object, event:MouseEvent):Array {
      var item:Object = row is rowClass ? row.item : row.fData;
      return item ? options(item) : [];
    }

    private static function options(item:Object):Array {
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

      var row:Object = sold ? Shops.getItem({shopItemId: item.ShopItemID}) : Inventory.row(item);
      found.push(CopyMenu.option("Copy Item", JSON.stringify(row, null, 2), "item " + item.sName));
      return found;
    }
  }
}
