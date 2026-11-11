package mira.game {
  import mira.Main;
  import flash.events.MouseEvent;
  import flash.utils.getQualifiedClassName;

  [BridgeNamespace("drops")]
  public class DropList {
    private static const DROP_FRAME:String = "DFrame2MC";
    private static const NAME_AND_COUNT:RegExp = /^(.*?)(?:\s+x(\d+))?$/;

    private static function itemNamed(name:String):Object {
      var wanted:String = name.toLowerCase();
      for each (var item:Object in Main.Game.world.invTree) {
        if (item && String(item.sName).toLowerCase() == wanted) {
          return item;
        }
      }

      return null;
    }

    // DFrame2MC keeps its item in an internal field this package cannot read, so the frame's text is read instead.
    [BridgeIgnore]
    public static function frameEntry(frame:*):Object {
      if (getQualifiedClassName(frame).indexOf(DROP_FRAME) == -1 || !frame.cnt || !frame.mouseChildren || !frame.cnt.ybtn.mouseEnabled) {
        return null;
      }

      var shown:Array = NAME_AND_COUNT.exec(String(frame.cnt.strName.text).replace(/^\s+|\s+$/g, ""));
      var item:Object = shown ? itemNamed(shown[1]) : null;
      return item ? {item: item, quantity: shown[2] ? int(shown[2]) : 1, frame: frame} : null;
    }

    [BridgeIgnore]
    public static function customEntry(item:Object):Object {
      return {item: item, quantity: int(Main.Game.cDropsUI.itemCount[item.dID]), frame: null};
    }

    private static function entries():Array {
      var game:Object = Main.Game;
      var found:Array = [];
      if (game.cDropsUI) {
        for each (var item:Object in game.cDropsUI.invTree) {
          found.push(customEntry(item));
        }
      }

      for (var index:int = 0; index < game.ui.dropStack.numChildren; index++) {
        var entry:Object = frameEntry(game.ui.dropStack.getChildAt(index));
        if (entry) {
          found.push(entry);
        }
      }

      return found;
    }

    private static function find(selector:Object):Object {
      var all:Array = entries();
      var items:Array = [];
      for each (var entry:Object in all) {
        items.push(entry.item);
      }

      var item:Object = ItemLookup.find(items, selector);
      return item ? all[items.indexOf(item)] : null;
    }

    [BridgeIgnore]
    public static function row(entry:Object):Object {
      var row:Object = Inventory.row(entry.item);
      row.quantity = entry.quantity;
      return row;
    }

    [BridgeExport]
    public static function getItems():Array {
      var rows:Array = [];
      for each (var entry:Object in entries()) {
        rows.push(row(entry));
      }

      return rows;
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var entry:Object = find(selector);
      return entry ? row(entry) : null;
    }

    private static function known(selector:Object):Object {
      var items:Array = [];
      for each (var item:Object in Main.Game.world.invTree) {
        if (item) {
          items.push(item);
        }
      }

      return ItemLookup.find(items, selector);
    }

    // Returns the item ID requested, or 0 when nothing was sent.
    [BridgeExport]
    public static function accept(selector:Object):int {
      var entry:Object = find(selector);
      var item:Object = entry ? entry.item : known(selector);
      if (!item) {
        return 0;
      }

      var game:Object = Main.Game;
      var avatar:Object = game.world.myAvatar;
      var full:Boolean = game.isHouseItem(item)
        ? avatar.houseitems.length >= avatar.objData.iHouseSlots
        : Inventory.categories().isFullFor(avatar.items, item, avatar.objData.iBagSlots);
      if (full) {
        return 0;
      }

      if (entry && entry.frame) {
        entry.frame.cnt.ybtn.dispatchEvent(new MouseEvent(MouseEvent.CLICK));
      }
      else if (entry) {
        if (!isCustomDropsUiOpen()) {
          game.cDropsUI.onShow();
        }

        game.cDropsUI.requestDrop(item, false);
      }
      else {
        game.sfc.sendXtMessage("zm", "getDrop", [item.ItemID], "str", game.world.curRoom);
      }

      return int(item.ItemID);
    }

    [BridgeExport]
    public static function reject(selector:Object):Boolean {
      var entry:Object = find(selector);
      if (!entry) {
        return false;
      }

      if (entry.frame) {
        entry.frame.cnt.nbtn.dispatchEvent(new MouseEvent(MouseEvent.CLICK));
      }
      else {
        Main.Game.cDropsUI.onBtNo(entry.item);
      }

      return true;
    }

    [BridgeExport]
    public static function toggleUi():void {
      var game:Object = Main.Game;
      if (isDraggable()) {
        game.cDropsUI.mcDraggable.menuBar.dispatchEvent(new MouseEvent(MouseEvent.CLICK));
      }
      else if (isUsingCustomDrops()) {
        game.cDropsUI.onShow();
      }
    }

    [BridgeExport]
    public static function isUsingCustomDrops():Boolean {
      var game:Object = Main.Game;
      return Boolean(game.cDropsUI) && game.litePreference.data.bCustomDrops;
    }

    [BridgeIgnore]
    public static function isDraggable():Boolean {
      var game:Object = Main.Game;
      return isUsingCustomDrops() && Boolean(game.cDropsUI.mcDraggable);
    }

    [BridgeIgnore]
    public static function isCustomDropsUiOpen():Boolean {
      var game:Object = Main.Game;
      if (game.cDropsUI)
        return game.cDropsUI.isMenuOpen();

      return false;
    }
  }
}
