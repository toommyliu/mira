package lyra.game {
  import lyra.Main;

  [BridgeNamespace("bank")]
  public class Bank {
    private static function categories():Object {
      return Class(Main.getInstance().getGameDomain().getDefinition("InvCat"));
    }

    private static function find(selector:Object):Object {
      return ItemLookup.find(Main.Game.world.bankinfo.BankArray, selector);
    }

    private static function isFull():Boolean {
      var avatar:Object = Main.Game.world.myAvatar;
      return avatar.iBankCount >= avatar.objData.iBankSlots;
    }

    private static function canStore(item:Object):Boolean {
      return !categories().isClass(item) && item.bEquip != 1;
    }

    [BridgeExport]
    public static function getItems():Array {
      var rows:Array = [];
      for each (var item:Object in Main.Game.world.bankinfo.BankArray) {
        rows.push(Inventory.row(item));
      }

      return rows;
    }

    [BridgeExport]
    public static function getItem(selector:Object):Object {
      var item:Object = find(selector);
      return item ? Inventory.row(item) : null;
    }

    [BridgeExport]
    public static function isLoaded():Boolean {
      return Main.Game.world.myAvatar.bank != null;
    }

    [BridgeExport]
    public static function loadItems():void {
      if (!isLoaded()) {
        Main.Game.getBank();
      }
    }

    [BridgeExport]
    public static function getSlots():int {
      return Main.Game.world.myAvatar.objData.iBankSlots;
    }

    [BridgeExport]
    public static function getUsedSlots():int {
      return Main.Game.world.myAvatar.iBankCount;
    }

    // Mirrors the refusals in the client's World.sendBankFromInvRequest, without its message boxes.
    [BridgeExport]
    public static function deposit(selector:Object):Boolean {
      var item:Object = Inventory.find(selector);
      if (!item || !canStore(item) || (item.bCoins == 0 && isFull())) {
        return false;
      }

      Main.Game.world.sendBankFromInvRequest(item);
      return true;
    }

    // The client checks inventory space only while its bank window is open, so this checks it always.
    [BridgeExport]
    public static function withdraw(selector:Object):Boolean {
      var item:Object = find(selector);
      var avatar:Object = Main.Game.world.myAvatar;
      if (!item) {
        return false;
      }

      var full:Boolean = categories().isHouse(item)
        ? avatar.houseitems.length >= avatar.objData.iHouseSlots
        : categories().isFullFor(avatar.items, item, avatar.objData.iBagSlots);
      if (full) {
        return false;
      }

      Main.Game.world.sendBankToInvRequest(item);
      return true;
    }

    // Mirrors the refusals in the client's World.sendBankSwapInvRequest, without its message boxes.
    [BridgeExport]
    public static function swap(inventorySelector:Object, bankSelector:Object):Boolean {
      var invItem:Object = Inventory.find(inventorySelector);
      var bankItem:Object = find(bankSelector);
      if (!invItem || !bankItem || !canStore(invItem)) {
        return false;
      }

      var cats:Object = categories();
      var avatar:Object = Main.Game.world.myAvatar;
      var bagSlots:int = avatar.objData.iBagSlots;
      if (invItem.bCoins == 0 && bankItem.bCoins == 1 && isFull()) {
        return false;
      }

      if (cats.isMisc(bankItem) && !cats.isMisc(invItem) && cats.isFullFor(avatar.items, bankItem, bagSlots)) {
        return false;
      }

      if (cats.isBag(bankItem) && !cats.isBag(invItem) && cats.countBag(avatar.items) >= bagSlots) {
        return false;
      }

      Main.Game.world.sendBankSwapInvRequest(bankItem, invItem);
      return true;
    }

    [BridgeExport]
    public static function open():Boolean {
      var world:Object = Main.Game.world;
      if (world.uiLock) {
        return false;
      }

      if (!isOpen()) {
        world.toggleBank();
      }

      return true;
    }

    [BridgeExport]
    public static function isOpen():Boolean {
      var popup:Object = Main.Game.ui.mcPopup;
      return popup.currentLabel == "Bank" && popup.getChildByName("mcBank") != null;
    }
  }
}
