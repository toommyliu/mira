package mira.game
{
import mira.Externalizer;
import mira.Main;
import skua.api.Auras;
import skua.api.Combat;
import skua.api.Inventory;
import skua.api.Monster;
import skua.api.Player;
import skua.api.Server;
import skua.api.Shop;
import skua.api.Skills;
import skua.api.World;
[BridgeNamespace("skua")]
public class Skua
  {
    [BridgeIgnore]
    public static function get game():*
    {
      return Main.Game;
    }

    [BridgeIgnore]
    public static function get external():Externalizer
    {
      return Main.getInstance().getExternal();
    }

    [BridgeIgnore]
    public static function gameObjectS(path:String):String
    {
      return Main.getGameObjectS(path);
    }

    [BridgeExport]
    public static function connectToServer(server:String):String
    {
      return skua.api.Server.connectToServer(server);
    }

    [BridgeExport]
    public static function clickServer(serverName:String):String
    {
      return skua.api.Server.clickServer(serverName);
    }

    [BridgeExport]
    public static function isLoggedIn():String
    {
      return skua.api.Player.isLoggedIn();
    }

    [BridgeExport]
    public static function isKicked():String
    {
      return skua.api.Player.isKicked();
    }

    [BridgeExport]
    public static function walkTo(xPos:int, yPos:int, walkSpeed:int):void
    {
      skua.api.Player.walkTo(xPos, yPos, walkSpeed);
    }

    [BridgeExport]
    public static function untargetSelf():void
    {
      skua.api.Player.untargetSelf();
    }

    [BridgeExport]
    public static function attackPlayer(name:String):String
    {
      return skua.api.Player.attackPlayer(name);
    }

    [BridgeExport]
    public static function getAvatar(id:int):String
    {
      return skua.api.Player.getAvatar(id);
    }

    [BridgeExport]
    public static function getLoadouts():String
    {
      return skua.api.Player.getLoadouts();
    }

    [BridgeExport]
    public static function gender():String
    {
      return skua.api.Player.Gender();
    }

    [BridgeExport]
    public static function rejectExcept(whitelist:String):String
    {
      return skua.api.Player.rejectExcept(whitelist);
    }

    [BridgeExport]
    public static function jumpCorrectRoom(cell:String, pad:String, autoCorrect:Boolean = true, clientOnly:Boolean = false):void
    {
      skua.api.World.jumpCorrectRoom(cell, pad, autoCorrect, clientOnly);
    }

    [BridgeExport]
    public static function disableDeathAd(enable:Boolean):void
    {
      skua.api.World.disableDeathAd(enable);
    }

    [BridgeExport]
    public static function skipCutscenes():void
    {
      skua.api.World.skipCutscenes();
    }

    [BridgeExport]
    public static function hidePlayers(enabled:Boolean):void
    {
      skua.api.World.hidePlayers(enabled);
    }

    [BridgeExport]
    public static function disableFX(enabled:Boolean):void
    {
      skua.api.World.disableFX(enabled);
    }

    [BridgeExport]
    public static function killLag(enable:Boolean):void
    {
      skua.api.World.killLag(enable);
    }

    [BridgeExport]
    public static function availableMonsters():String
    {
      return skua.api.Monster.availableMonstersInCell();
    }

    [BridgeExport]
    public static function getMonsters():String
    {
      return skua.api.Monster.getMonsters();
    }

    [BridgeExport]
    public static function getTargetMonster():String
    {
      return skua.api.Monster.getTargetMonster();
    }

    [BridgeExport]
    public static function attackMonsterName(name:String):String
    {
      return skua.api.Monster.attackMonsterByName(name);
    }

    [BridgeExport]
    public static function attackMonsterID(id:int):String
    {
      return skua.api.Monster.attackMonsterByID(id);
    }

    [BridgeExport]
    public static function canUseSkill(index:int, ignoreTarget:Boolean = false):String
    {
      return skua.api.Skills.canUseSkill(index, ignoreTarget);
    }

    [BridgeExport]
    public static function skillCooldownRemaining(index:int):String
    {
      return skua.api.Skills.skillCooldownRemaining(index);
    }

    [BridgeExport]
    public static function globalCooldownRemaining():String
    {
      return skua.api.Skills.globalCooldownRemaining();
    }

    [BridgeExport]
    public static function useSkill(index:int):String
    {
      return skua.api.Skills.useSkill(index);
    }

    [BridgeExport]
    public static function infiniteRange():void
    {
      skua.api.Combat.infiniteRange();
    }

    [BridgeExport]
    public static function magnetize():void
    {
      skua.api.Combat.magnetize();
    }

    [BridgeExport]
    public static function hasInventoryCategories():String
    {
      return skua.api.Inventory.hasInventoryCategories();
    }

    [BridgeExport]
    public static function inventoryBagUsedSlots():int
    {
      return skua.api.Inventory.inventoryBagUsedSlots();
    }

    [BridgeExport]
    public static function inventoryMiscSlots():int
    {
      return skua.api.Inventory.inventoryMiscSlots();
    }

    [BridgeExport]
    public static function inventoryMiscUsedSlots():int
    {
      return skua.api.Inventory.inventoryMiscUsedSlots();
    }

    [BridgeExport]
    public static function isFavoriteItem(itemID:int):String
    {
      return skua.api.Inventory.isFavoriteItem(itemID);
    }

    [BridgeExport]
    public static function inventoryPool(itemJson:String):String
    {
      return skua.api.Inventory.inventoryPool(itemJson);
    }

    [BridgeExport]
    public static function inventoryHasSpaceFor(itemJson:String, quantity:int = 1, purchase:Boolean = false, outgoingJson:String = null):String
    {
      return skua.api.Inventory.inventoryHasSpaceFor(itemJson, quantity, purchase, outgoingJson);
    }

    [BridgeExport]
    public static function buyItemByName(name:String, quantity:int = -1):void
    {
      skua.api.Shop.buyItemByName(name, quantity);
    }

    [BridgeExport]
    public static function buyItemByID(id:int, shopItemID:int, quantity:int = -1):void
    {
      skua.api.Shop.buyItemByID(id, shopItemID, quantity);
    }

    [BridgeExport]
    public static function getShopItem(name:String):*
    {
      return skua.api.Shop.getShopItem(name);
    }

    [BridgeExport]
    public static function getShopItemByID(itemID:int, shopItemID:int):*
    {
      return skua.api.Shop.getShopItemByID(itemID, shopItemID);
    }

    [BridgeExport]
    public static function getUnmetPurchaseRequirements(itemID:int, shopItemID:int, quantity:int = -1):String
    {
      return skua.api.Shop.getUnmetPurchaseRequirements(itemID, shopItemID, quantity);
    }

    [BridgeExport]
    public static function getSubjectAuras(subject:String):Array
    {
      return skua.api.Auras.getSubjectAuras(subject);
    }

    [BridgeExport]
    public static function GetAuraSnapshots(subject:String):String
    {
      return skua.api.Auras.GetAuraSnapshots(subject);
    }

    [BridgeExport]
    public static function GetAuraRuleStacks(subject:String, auraName:String):String
    {
      return skua.api.Auras.GetAuraRuleStacks(subject, auraName);
    }

    [BridgeExport]
    public static function HasAnyActiveAura(subject:String, auraNames:String):String
    {
      return skua.api.Auras.HasAnyActiveAura(subject, auraNames);
    }

    [BridgeExport]
    public static function GetPlayerAura(playerName:String):String
    {
      return skua.api.Auras.GetPlayerAura(playerName);
    }

    [BridgeExport]
    public static function GetMonsterAuraByName(monsterName:String):String
    {
      return skua.api.Auras.GetMonsterAuraByName(monsterName);
    }

    [BridgeExport]
    public static function GetMonsterAuraByID(monID:int):String
    {
      return skua.api.Auras.GetMonsterAuraByID(monID);
    }

    [BridgeExport]
    public static function rebuildAuraArray(auras:Object):Array
    {
      return skua.api.Auras.rebuildAuraArray(auras);
    }

    [BridgeExport]
    public static function GetAurasValue(subject:String, auraName:String):Number
    {
      return skua.api.Auras.GetAurasValue(subject, auraName);
    }

    [BridgeExport]
    public static function rebuilduoTree(playerName:String):Object
    {
      return skua.api.Auras.rebuilduoTree(playerName);
    }

    [BridgeExport]
    public static function rebuildmonTree(monID:int):Object
    {
      return skua.api.Auras.rebuildmonTree(monID);
    }
  }
}
