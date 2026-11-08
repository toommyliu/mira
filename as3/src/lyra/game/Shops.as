package lyra.game {
  import lyra.Main;

  [BridgeNamespace("shops")]
  public class Shops {
    private static const SHOP_POPUP_LABELS:Object = {
      "Shop": true,
      "MergeShop": true,
      "HouseShop": true
    };

    private static function getShopInfo():Object {
      var game:Object = Main.Game;
      return game.world.shopinfo;
    }

    private static function getShopItems():Array {
      var info:Object = getShopInfo();
      if (!info || !(info.items is Array)) {
        return [];
      }

      return info.items;
    }

    private static function getStringSelector(selector:Object, key:String):String {
      var value:* = selector[key];
      if (!(value is String)) {
        return null;
      }

      return String(value);
    }

    private static function getNumberSelector(selector:Object, key:String):Number {
      var value:* = selector[key];
      if (!(value is Number) && !(value is int) && !(value is uint)) {
        return NaN;
      }

      return Number(value);
    }

    private static function getItemByShopItemId(shopItemId:Number):Object {
      if (isNaN(shopItemId) || shopItemId <= 0) {
        return null;
      }

      for each (var item:Object in getShopItems()) {
        if (item && "ShopItemID" in item && Number(item.ShopItemID) === shopItemId) {
          return item;
        }
      }

      return null;
    }

    private static function getShopItem(selector:Object):Object {
      if ("name" in selector) {
        return ItemLookup.find(getShopItems(), selector);
      }

      if ("itemId" in selector) {
        return ItemLookup.find(getShopItems(), selector);
      }

      if ("shopItemId" in selector) {
        return getItemByShopItemId(getNumberSelector(selector, "shopItemId"));
      }

      return null;
    }

    private static function getInventoryItem(selector:Object):Object {
      if ("name" in selector) {
        return Inventory.find(selector);
      }

      if ("itemId" in selector) {
        return Inventory.find(selector);
      }

      return null;
    }

    private static function sendBuy(item:Object, quantity:int):void {
      var game:Object = Main.Game;
      if (!game.world.coolDown("buyItem")) {
        return;
      }

      game.world.shopBuyItem = item;
      if (quantity === 1) {
        game.sfc.sendXtMessage("zm", "buyItem", [
          item.ItemID,
          game.world.shopinfo.ShopID,
          item.ShopItemID
        ], "str", game.world.curRoom);
        return;
      }

      game.sfc.sendXtMessage("zm", "buyItem", [
        item.ItemID,
        game.world.shopinfo.ShopID,
        item.ShopItemID,
        quantity
      ], "str", game.world.curRoom);
    }

    private static function sendSell(item:Object, quantity:int):void {
      var game:Object = Main.Game;
      if (quantity === -1) {
        game.world.sendSellItemRequest(item);
        return;
      }

      game.world.sendSellItemRequestWithQuantity({
        accept: 1,
        iSel: item,
        iQty: quantity
      });
    }

    [BridgeTsParamType("selector: FlashTypes.ShopItemSelector")]
    [BridgeExport]
    public static function getItem(selector:Object):Object {
      if (!selector) {
        return null;
      }

      return getShopItem(selector);
    }

    [BridgeTsParamType("selector: FlashTypes.ShopItemSelector")]
    [BridgeExport]
    public static function getMaxBuyQuantity(selector:Object):int {
      if (!selector || !getShopInfo()) {
        return 0;
      }

      var item:Object = getShopItem(selector);
      if (!item) {
        return 0;
      }

      var game:Object = Main.Game;
      return game.world.maximumShopBuys(item);
    }

    private static function isShopPopupOpen():Boolean {
      var game:Object = Main.Game;
      var popup:* = game.ui.mcPopup;

      return popup != null &&
        popup.visible &&
        SHOP_POPUP_LABELS[popup.currentLabel] === true;
    }

    private static function isEnhancement(item:Object):Boolean {
      return item && "sType" in item && String(item.sType).toLowerCase() == "enhancement";
    }

    [BridgeExport]
    public static function isOpen(shopId:int = 0):Boolean {
      var info:Object = getShopInfo();

      if (!isShopPopupOpen() || info == null || !("ShopID" in info)) {
        return false;
      }

      return shopId <= 0 || int(info.ShopID) == shopId;
    }

    [BridgeExport]
    public static function close(shopId:int = 0):Boolean {
      if (!isOpen(shopId)) {
        return false;
      }

      var game:Object = Main.Game;
      game.ui.mcPopup.onClose();
      return true;
    }

    private static function canBuyShopItem(item:Object, quantity:int = 1):Boolean {
      var game:Object = Main.Game;
      var categories:Class = Class(Main.getInstance().getGameDomain().getDefinition("InvCat"));
      if (item.bStaff == 1 && game.world.myAvatar.objData.intAccessLevel < 40) {
        return false;
      }
      else if (game.world.shopinfo.sField != "" && game.world.getAchievement(game.world.shopinfo.sField, game.world.shopinfo.iIndex) != 1) {
        return false;
      }
      else if (item.bUpg == 1 && !game.world.myAvatar.isUpgraded()) {
        return false;
      }
      else if (int(item.iLvl) > int(game.world.myAvatar.objData.intLevel)) {
        return false;
      }
      else if (item.FactionID > 1 && game.world.myAvatar.getRep(item.FactionID) < item.iReqRep) {
        return false;
      }
      else if (!validateArmor(item)) {
        return false;
      }
      else if (item.iQSindex >= 0 && game.world.getQuestValue(item.iQSindex) < int(item.iQSvalue)) {
        return false;
      }
      else if (
          (game.world.myAvatar.isItemInInventory(item.ItemID) || game.world.myAvatar.isItemInBank(item.ItemID)) &&
          game.world.myAvatar.isItemStackMaxed(item.ItemID)
        ) {
        return false;
      }
      else if (item.bCoins == 0 && item.iCost > game.world.myAvatar.objData.intGold) {
        return false;
      }
      else if (item.bCoins == 1 && item.iCost > game.world.myAvatar.objData.intCoins) {
        return false;
      }
      else if (
          !game.isHouseItem(item) && categories["isFullFor"](
            game.world.myAvatar.items,
            item,
            game.world.myAvatar.objData.iBagSlots,
            categories["freedBy"](game.world.myAvatar.items, item, quantity)
          ) ||
          game.isHouseItem(item) && game.world.myAvatar.houseitems.length >= game.world.myAvatar.objData.iHouseSlots
        ) {
        return false;
      }

      return true;
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

    private static function canBuyQuantity(item:Object, quantity:int):Boolean {
      if (!item || quantity <= 0) {
        return false;
      }

      if (!canBuyShopItem(item, quantity)) {
        return false;
      }

      var game:Object = Main.Game;
      return game.world.maximumShopBuys(item) >= quantity;
    }

    [BridgeTsParamType("selector: FlashTypes.ShopItemSelector")]
    [BridgeExport]
    public static function enhance(selector:Object, itemId:int):Boolean {
      if (!selector || itemId <= 0 || !getShopInfo()) {
        return false;
      }

      var enhancement:Object = getShopItem(selector);
      if (!isEnhancement(enhancement) || !canBuyShopItem(enhancement)) {
        return false;
      }

      var game:Object = Main.Game;
      if (!game.world.coolDown("buyItem")) {
        return false;
      }

      game.world.enhItem = [itemId];
      game.world.confirmSendEnhItemRequestShop({
        accept: true,
        enh: enhancement,
        item: [itemId]
      });
      return true;
    }

    [BridgeTsParamType("selector: FlashTypes.ShopItemSelector")]
    [BridgeExport]
    public static function buy(selector:Object, quantity:int = 1):void {
      if (!selector || quantity <= 0) {
        return;
      }

      var item:Object = getShopItem(selector);
      if (!canBuyQuantity(item, quantity)) {
        return;
      }

      sendBuy(item, quantity);
    }

    [BridgeTsParamType("selector: FlashTypes.InventoryItemSelector")]
    [BridgeExport]
    public static function sell(selector:Object, quantity:int = -1):Boolean {
      if (!selector || (quantity !== -1 && quantity <= 0)) {
        return false;
      }

      var item:Object = getInventoryItem(selector);
      if (!item) {
        return false;
      }

      sendSell(item, quantity);
      return true;
    }

    [BridgeExport]
    public static function load(shopId:int):void {
      var game:Object = Main.Game;
      game.world.sendLoadShopRequest(shopId);
    }

    [BridgeExport]
    public static function loadHairShop(shopId:int):void {
      var game:Object = Main.Game;
      game.world.sendLoadHairShopRequest(shopId);
    }

    [BridgeExport]
    public static function loadArmorCustomize():void {
      var game:Object = Main.Game;
      game.openArmorCustomize();
    }

    [BridgeExport]
    public static function isMergeShop():Boolean {
      var info:Object = getShopInfo();
      if (!info) {
        return false;
      }

      var game:Object = Main.Game;
      return game.isMergeShop(info);
    }

    [BridgeTsParamType("selector: FlashTypes.ShopItemSelector")]
    [BridgeExport]
    public static function canBuyItem(selector:Object, quantity:int = 1):Boolean {
      if (!selector || !getShopInfo()) {
        return false;
      }

      var item:Object = getShopItem(selector);
      if (!item) {
        return false;
      }

      if (isEnhancement(item)) {
        return quantity === 1 && canBuyShopItem(item);
      }

      return canBuyQuantity(item, quantity);
    }
  }
}
