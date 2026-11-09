package mira.game {
  import mira.Main;

  [BridgeNamespace("player")]
  public class Player {

    [BridgeExport]
    public static function joinMap(map:String, cell:String = "Enter", pad:String = "Spawn"):void {
      if (!map)
        return;

      var game:Object = Main.Game;
      game.world.gotoTown(map, cell, pad);
    }

    [BridgeExport]
    public static function getMap():String {
      var game:Object = Main.Game;
      return game.world.strMapName;
    }

    [BridgeExport]
    public static function jump(cell:String, pad:String = "Spawn"):void {
      if (!cell)
        return;

      var game:Object = Main.Game;
      game.world.moveToCell(cell, pad);
    }

    [BridgeExport]
    public static function getCell():String {
      var game:Object = Main.Game;
      return game.world.strFrame;
    }

    [BridgeExport]
    public static function getPad():String {
      var game:Object = Main.Game;
      return game.world.strPad;
    }

    [BridgeExport]
    public static function getFactions():Array {
      var game:Object = Main.Game;
      return game.world.myAvatar.factions;
    }

    [BridgeExport]
    public static function getState():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intState;
    }

    [BridgeExport]
    public static function getHp():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intHP;
    }

    [BridgeExport]
    public static function getMaxHp():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intHPMax;
    }

    [BridgeExport]
    public static function getMp():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intMP;
    }

    [BridgeExport]
    public static function getMaxMp():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intMPMax;
    }

    [BridgeExport]
    public static function getLevel():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.intLevel;
    }

    [BridgeExport]
    public static function getGold():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.objData.intGold;
    }

    [BridgeExport]
    public static function isMember():Boolean {
      var game:Object = Main.Game;
      return game.world.myAvatar.isUpgraded();
    }

    [BridgeExport]
    public static function isAfk():Boolean {
      var game:Object = Main.Game;
      return game.world.myAvatar.dataLeaf.afk;
    }

    [BridgeExport]
    public static function getPosition():Array {
      var game:Object = Main.Game;
      return [game.world.myAvatar.pMC.x, game.world.myAvatar.pMC.y];
    }

    [BridgeExport]
    public static function walkTo(x:int, y:int, walkSpeed:Number = NaN):Boolean {
      var game:Object = Main.Game;
      if (isNaN(walkSpeed) || walkSpeed <= 0)
        walkSpeed = game.world.WALKSPEED;

      game.world.myAvatar.pMC.walkTo(x, y, walkSpeed);
      game.world.moveRequest({mc: game.world.myAvatar.pMC, tx: x, ty: y, sp: walkSpeed});
      return true;
    }

    [BridgeExport]
    public static function rest():void {
      var game:Object = Main.Game;
      game.world.rest();
    }

    [BridgeExport]
    public static function hasActiveBoost(boostType:String):Boolean {
      if (!boostType)
        return false;

      var game:Object = Main.Game;
      if (boostType.indexOf("gold") > -1) {
        return game.world.myAvatar.objData.iBoostG > 0;
      }

      if (boostType.indexOf("xp") > -1) {
        return game.world.myAvatar.objData.iBoostXP > 0;
      }

      if (boostType.indexOf("rep") > -1) {
        return game.world.myAvatar.objData.iBoostRep > 0;
      }

      if (boostType.indexOf("class") > -1) {
        return game.world.myAvatar.objData.iBoostCP > 0;
      }

      return false;
    }

    [BridgeExport]
    public static function getAuras():Array {
      return Auras.rows(Main.Game.world.myAvatar.dataLeaf);
    }

    [BridgeExport]
    public static function getClassName():String {
      var game:Object = Main.Game;
      return game.world.myAvatar.objData.strClassName.toUpperCase();
    }

    [BridgeExport]
    public static function getUserId():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.uid;
    }

    [BridgeExport]
    public static function getCharId():int {
      var game:Object = Main.Game;
      return game.world.myAvatar.objData.CharID;
    }

    [BridgeExport]
    public static function getGender():String {
      var game:Object = Main.Game;
      return game.world.myAvatar.objData.strGender.toUpperCase();
    }

    [BridgeExport]
    public static function getData():Object {
      var game:Object = Main.Game;
      if (!game.world.myAvatar) {
        return null;
      }

      return game.world.myAvatar.objData;
    }

    [BridgeExport]
    public static function isLoaded():Boolean {
      var game:Object = Main.Game;
      var world:Object = game ? game.world : null;
      if (!world) {
        return false;
      }

      var avatar:Object = world.myAvatar;
      if (!avatar) {
        return false;
      }

      var items:Array = avatar.items as Array;
      var playerMovie:Object = avatar.pMC;
      return Boolean(items) && items.length > 0 && World.isLoaded() && Boolean(playerMovie) && playerMovie.artLoaded();
    }

    [BridgeExport]
    public static function goToPlayer(name:String):void {
      if (!name) {
        return;
      }

      var game:Object = Main.Game;
      game.world['goto'](name);
    }
  }
}
