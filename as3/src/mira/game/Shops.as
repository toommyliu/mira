package mira.game {
  import mira.Main;

  [BridgeNamespace("shops")]
  public class Shops {
    private static const SHOP_POPUP_LABELS:Object = {
      "Shop": true,
      "MergeShop": true,
      "HouseShop": true
    };

    private static function info():Object {
      return Main.Game.world.shopinfo;
    }

    private static function items():Array {
      var shop:Object = info();
      return shop && shop.items is Array ? shop.items : [];
    }

    private static function find(selector:Object):Object {
      if (!selector || !("shopItemId" in selector)) {
        return ItemLookup.find(items(), selector);
      }

      for each (var item:Object in items()) {
        if (Number(item.ShopItemID) === Number(selector.shopItemId)) {
          return item;
        }
      }

      return null;
    }

    private static function requirements(item:Object):Array {
      var rows:Array = [];
      for each (var need:Object in item.turnin) {
        rows.push({itemId: need.ItemID, name: need.sName, quantity: need.iQty});
      }

      return rows;
    }

    private static function row(item:Object):Object {
      return {
        shopItemId: item.ShopItemID,
        itemId: item.ItemID,
        name: item.sName,
        description: item.sDesc,
        category: item.sType,
        slot: item.sES,
        patternId: item.PatternID,
        procId: item.ItemProcID,
        quantity: item.iQty,
        maxStack: item.iStk,
        cost: item.iCost,
        coins: item.bCoins == 1,
        member: item.bUpg == 1,
        level: item.iLvl,
        faction: item.FactionID > 1 ? {id: item.FactionID, name: item.sFaction, rep: item.iReqRep} : null,
        requirements: requirements(item)
      };
    }

    [BridgeExport]
    public static function getShop():Object {
      var shop:Object = info();
      if (!shop) {
        return null;
      }

      var rows:Array = [];
      for each (var item:Object in items()) {
        rows.push(row(item));
      }

      return {
        id: shop.ShopID,
        name: shop.sName,
        merge: Main.Game.isMergeShop(shop),
        limited: Boolean(shop.bLimited),
        house: shop.bHouse == 1,
        items: rows
      };
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var item:Object = find(selector);
      return item ? row(item) : null;
    }

    [BridgeExport]
    public static function getMaxBuyQuantity(selector:Object):int {
      var item:Object = find(selector);
      return item ? Main.Game.world.maximumShopBuys(item) : 0;
    }

    [BridgeExport]
    public static function isOpen(shopId:int = 0):Boolean {
      var popup:Object = Main.Game.ui.mcPopup;
      var shop:Object = info();
      if (!popup || !popup.visible || SHOP_POPUP_LABELS[popup.currentLabel] !== true || !shop) {
        return false;
      }

      return shopId <= 0 || int(shop.ShopID) == shopId;
    }

    [BridgeExport]
    public static function close(shopId:int = 0):Boolean {
      if (!isOpen(shopId)) {
        return false;
      }

      Main.Game.ui.mcPopup.onClose();
      return true;
    }

    [BridgeExport]
    public static function load(shopId:int):void {
      Main.Game.world.sendLoadShopRequest(shopId);
    }

    [BridgeExport]
    public static function loadHairShop(shopId:int):void {
      Main.Game.world.sendLoadHairShopRequest(shopId);
    }

    [BridgeExport]
    public static function openArmorCustomizer():void {
      Main.Game.openArmorCustomize();
    }

    private static function isEnhancement(item:Object):Boolean {
      return String(item.sType).toLowerCase() == "enhancement";
    }

    // Mirrors the refusals in the client's World.sendBuyItemRequest and sendBuyItemRequestWithQuantity, without their message boxes.
    private static function canBuyShopItem(item:Object, quantity:int, needsSpace:Boolean = true):Boolean {
      var game:Object = Main.Game;
      var world:Object = game.world;
      var avatar:Object = world.myAvatar;
      var shop:Object = info();
      var categories:Object = Inventory.categories();
      if (item.bStaff == 1 && avatar.objData.intAccessLevel < 40) {
        return false;
      }
      else if (shop.sField != "" && world.getAchievement(shop.sField, shop.iIndex) != 1) {
        return false;
      }
      else if (item.bUpg == 1 && !avatar.isUpgraded()) {
        return false;
      }
      else if (int(item.iLvl) > int(avatar.objData.intLevel)) {
        return false;
      }
      else if (item.FactionID > 1 && avatar.getRep(item.FactionID) < item.iReqRep) {
        return false;
      }
      else if (!validateArmor(item)) {
        return false;
      }
      else if (item.iQSindex >= 0 && world.getQuestValue(item.iQSindex) < int(item.iQSvalue)) {
        return false;
      }
      else if (categories.isToken(item) && categories.tokenHave(item, world) < categories.tokenNeed(item, quantity)) {
        return false;
      }
      else if (
          (avatar.isItemInInventory(item.ItemID) || avatar.isItemInBank(item.ItemID)) &&
          avatar.isItemStackMaxed(item.ItemID)
        ) {
        return false;
      }
      else if (item.bCoins == 0 && item.iCost > avatar.objData.intGold) {
        return false;
      }
      else if (item.bCoins == 1 && item.iCost > avatar.objData.intCoins) {
        return false;
      }
      else if (!needsSpace) {
        return true;
      }
      else if (game.isHouseItem(item)) {
        return avatar.houseitems.length < avatar.objData.iHouseSlots;
      }

      return !categories.isFullFor(
        avatar.items,
        item,
        avatar.objData.iBagSlots,
        categories.freedBy(avatar.items, item, quantity)
      );
    }

    private static function validateArmor(item:Object):Boolean {
      var game:Object = Main.Game;

      var index:uint = 0;
      var classIndex:uint = 0;
      var classIds:Array = [];
      var valid:Boolean = true;
      var requiresAll:Boolean = false;
      var requiresAny:Boolean = false;
      var itemId:int = int(item.ItemID);
      switch (itemId) {
        case 319:
        case 2083:
          requiresAll = true;
          classIds = [16, 15654, 407, 20, 15651, 409];
          break;
        case 409:
          requiresAny = true;
          classIds = [20, 15651];
          break;
        case 408:
          requiresAny = true;
          classIds = [17, 15653];
          break;
        case 410:
          requiresAny = true;
          classIds = [18, 15652];
          break;
        case 407:
          requiresAny = true;
          classIds = [16, 15654];
      }

      if (requiresAll) {
        index = 0;
        while (index < classIds.length) {
          if (game.world.myAvatar.getCPByID(classIds[index]) < 302500) {
            valid = false;
          }
          else {
            valid = true;
            if (index < 2) {
              index = 2;
            }
            if (index < 5 && index > 2) {
              break;
            }
          }
          index++;
        }
        return valid;
      }

      if (requiresAny) {
        classIndex = 0;
        while (classIndex < classIds.length) {
          if (game.world.myAvatar.getCPByID(classIds[classIndex]) >= item.iReqCP) {
            return true;
          }
          classIndex++;
        }
        return false;
      }

      return !(Number(item.iClass) > 0 && game.world.myAvatar.getCPByID(item.iClass) < item.iReqCP);
    }

    [BridgeExport]
    public static function canBuyItem(selector:Object, quantity:int = 1):Boolean {
      var item:Object = find(selector);
      return item != null &&
        quantity > 0 &&
        canBuyShopItem(item, quantity) &&
        Main.Game.world.maximumShopBuys(item) >= quantity;
    }

    [BridgeExport]
    public static function buy(selector:Object, quantity:int = 1):Boolean {
      var item:Object = find(selector);
      var world:Object = Main.Game.world;
      if (!canBuyItem(selector, quantity) || !world.coolDown("buyItem")) {
        return false;
      }

      var args:Array = [item.ItemID, info().ShopID, item.ShopItemID];
      if (quantity != 1) {
        args.push(quantity);
      }

      world.shopBuyItem = item;
      Main.Game.sfc.sendXtMessage("zm", "buyItem", args, "str", world.curRoom);
      return true;
    }

    // Applying an enhancement adds nothing to the bag, so bag space never blocks it.
    [BridgeExport]
    public static function canEnhance(selector:Object):Boolean {
      var enhancement:Object = find(selector);
      return enhancement != null && isEnhancement(enhancement) && canBuyShopItem(enhancement, 1, false);
    }

    // Mirrors the refusals in the client's World.sendEnhItemRequestShop, without its message box and confirmation.
    [BridgeExport]
    public static function enhance(selector:Object, itemId:int):Boolean {
      var enhancement:Object = find(selector);
      var world:Object = Main.Game.world;
      if (!enhancement || !isEnhancement(enhancement) || !Inventory.find({itemId: itemId})) {
        return false;
      }

      if (world.myAvatar.objData.intGold < enhancement.iCost || !world.coolDown("buyItem")) {
        return false;
      }

      world.enhItem = [itemId];
      world.confirmSendEnhItemRequestShop({accept: true, enh: enhancement, item: [itemId]});
      return true;
    }

    private static function canSell(item:Object, quantity:int):Boolean {
      var favorites:Object = Main.getInstance().getGameDomain().getDefinition("liteAssets.draw.FavStore");
      var placed:Boolean = item.sType == "Floor Item" || item.sType == "Wall Item";
      if (quantity <= 0 || quantity > int(item.iQty) || favorites.has(item.ItemID)) {
        return false;
      }

      if ((item.bEquip == 1 && !placed) || int(item.bEquip) >= int(item.iQty)) {
        return false;
      }

      var bought:Boolean = item.iHrs != null || item.tPurchase != null;
      if (item.bCoins == 1 && !bought && item.iCost != 0) {
        return false;
      }

      return !(item.hasOwnProperty("sMeta") && String(item.sMeta).indexOf("NoSell") > -1);
    }

    [BridgeExport]
    public static function sell(selector:Object, quantity:int = -1):Boolean {
      var item:Object = Inventory.find(selector);
      var world:Object = Main.Game.world;
      if (!item) {
        return false;
      }

      var amount:int = quantity == -1 ? int(item.iQty) : quantity;
      if (!canSell(item, amount) || !world.coolDown("sellItem")) {
        return false;
      }

      Main.Game.sfc.sendXtMessage("zm", "sellItem", [item.ItemID, amount, item.CharItemID], "str", world.curRoom);
      return true;
    }
  }
}
