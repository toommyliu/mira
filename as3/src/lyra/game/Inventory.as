package lyra.game {
  import lyra.Main;

  [BridgeNamespace("inventory")]
  public class Inventory {
    private static const UNEQUIPPABLE_SLOTS:Array = ["Weapon", "ar"];
    private static const UNENHANCED_SLOTS:Array = ["mi", "co", "pe", "am"];

    private static function categories():Object {
      return Class(Main.getInstance().getGameDomain().getDefinition("InvCat"));
    }

    private static function row(item:Object):Object {
      return {
        itemId: item.ItemID,
        charItemId: item.CharItemID,
        name: item.sName,
        description: item.sDesc,
        quantity: item.iQty,
        maxStack: item.iStk,
        category: item.sType,
        slot: item.sES,
        link: item.sLink,
        meta: item.sMeta,
        file: item.sFile,
        equipped: item.bEquip == 1,
        wearable: item.hasOwnProperty("bWear"),
        worn: item.bWear == 1,
        member: item.bUpg == 1,
        coins: item.bCoins == 1,
        pool: categories().poolOf(item),
        classRank: item.sType == "Class" ? Main.Game.getRankFromPoints(item.iQty) : null,
        enhId: item.EnhID,
        enhLevel: item.EnhLvl,
        enhPatternId: item.EnhPatternID,
        enhProcId: item.ProcID,
        enhDps: item.EnhDPS,
        enhRange: item.EnhRng,
        enhRarity: item.EnhRty
      };
    }

    [BridgeIgnore]
    public static function find(selector:Object):Object {
      return ItemLookup.find(Main.Game.world.myAvatar.items, selector);
    }

    [BridgeExport]
    public static function getItems():Array {
      var rows:Array = [];
      for each (var item:Object in Main.Game.world.myAvatar.items) {
        rows.push(row(item));
      }

      return rows;
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var item:Object = find(selector);
      return item ? row(item) : null;
    }

    [BridgeExport]
    public static function getSlots():int {
      return Main.Game.world.myAvatar.objData.iBagSlots;
    }

    [BridgeExport]
    public static function getUsedSlots(pool:String):int {
      var items:Array = Main.Game.world.myAvatar.items;
      return pool == "misc" ? categories().countMisc(items) : categories().countBag(items);
    }

    private static function isUsable(item:Object):Boolean {
      return item.sType == "Item";
    }

    private static function canChangeEquipment():Boolean {
      var world:Object = Main.Game.world;
      var leaf:Object = world.getUoLeafById(world.myAvatar.uid);
      return leaf != null && leaf.intState == 1 && !world.bPvP;
    }

    // Mirrors the refusals in the client's Game.toggleItemEquip, without its message boxes.
    [BridgeExport]
    public static function equip(selector:Object):Boolean {
      var item:Object = find(selector);
      var world:Object = Main.Game.world;
      if (!item || (item.bEquip == 1 && !isUsable(item)) || !canChangeEquipment()) {
        return false;
      }

      if ((item.bUpg == 1 && !world.myAvatar.isUpgraded()) || int(item.EnhLvl) > int(world.myAvatar.objData.intLevel)) {
        return false;
      }

      if (isUsable(item)) {
        world.equipUseableItem(item);
        return true;
      }

      if (UNENHANCED_SLOTS.indexOf(item.sES) == -1 && item.EnhID <= 0) {
        return false;
      }

      return world.sendEquipItemRequest(item);
    }

    [BridgeExport]
    public static function unequip(selector:Object):Boolean {
      var item:Object = find(selector);
      var world:Object = Main.Game.world;
      if (!item || item.bEquip != 1 || !canChangeEquipment() || UNEQUIPPABLE_SLOTS.indexOf(item.sES) != -1) {
        return false;
      }

      if (isUsable(item)) {
        world.unequipUseableItem(item);
        return true;
      }

      if (!world.coolDown("unequipItem")) {
        return false;
      }

      Main.Game.sfc.sendXtMessage("zm", "unequipItem", [item.ItemID], "str", world.curRoom);
      return true;
    }

    [BridgeExport("use")]
    public static function useItem(selector:Object):Boolean {
      var item:Object = find(selector);
      if (!item) {
        return false;
      }

      var link:String = String(item.sLink).toLowerCase();
      if (String(item.sType).toLowerCase() != "serveruse" && link != "elixir" && link != "tonic") {
        return false;
      }

      Main.Game.world.sendUseItemRequest(item);
      return true;
    }

    [BridgeExport]
    public static function wear(selector:Object):Boolean {
      var item:Object = find(selector);
      var world:Object = Main.Game.world;
      if (!item || !item.hasOwnProperty("bWear") || item.bWear == 1) {
        return false;
      }

      if ((item.bUpg == 1 && !world.myAvatar.isUpgraded()) || !world.coolDown("wearItem")) {
        return false;
      }

      Main.Game.sfc.sendXtMessage("zm", "wearItem", [item.ItemID], "str", world.curRoom);
      return true;
    }
  }
}
