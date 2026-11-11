package mira.game
{
import mira.Main;
import flash.display.DisplayObject;

[BridgeNamespace("world")]
public class World
  {
    private static var padNames:RegExp = /(Spawn|Center|Left|Right|Up|Down|Top|Bottom)/;

    private static function getStringSelector(selector:Object, key:String):String
    {
      var value:* = selector[key];
      if (!(value is String))
      {
        return null;
      }

      return String(value);
    }

    private static function getNumberSelector(selector:Object, key:String):Number
    {
      var value:* = selector[key];
      if (!(value is Number) && !(value is int) && !(value is uint))
      {
        return NaN;
      }

      return Number(value);
    }

    private static function sameText(left:*, right:*):Boolean
    {
      return left != null && right != null && String(left).toLowerCase() == String(right).toLowerCase();
    }

    [BridgeExport]
    public static function isLoaded():Boolean
    {
      var game:Object = Main.Game;
      var world:Object = game ? game.world : null;
      if (!world || world.mapLoadInProgress || !world.map)
      {
        return false;
      }

      try
      {
        if (!world.map.currentScene || !world.map.currentScene.labels)
        {
          return false;
        }

        return game.numChildren > 0 && game.getChildAt((game.numChildren - 1)) != game.mcConnDetail;
      }
      catch (e:Error)
      {
        return false;
      }

      return false;
    }

    // Players in the cell exist before their retrieveUserDatas reply sets objData, and the
    // game's testAction reads objData from every player in the cell.
    private static function hasCellUserData(world:Object):Boolean
    {
      var frame:String = world.myAvatar.dataLeaf.strFrame;
      for each (var avatar:Object in world.avatars)
      {
        if (avatar.dataLeaf != null && avatar.dataLeaf.strFrame == frame && avatar.objData == null)
        {
          return false;
        }
      }

      return true;
    }

    /** Reports whether AQW has finished initializing the requested cell. */
    [BridgeExport]
    public static function isCellReady(cell:String, pad:String = null):Boolean
    {
      var game:Object = Main.Game;
      var world:Object = game ? game.world : null;
      if (
        !world ||
        !cell ||
        world.mapLoadInProgress ||
        !world.mapEntered ||
        !world.map ||
        !world.myAvatar ||
        !world.myAvatar.pMC ||
        !world.CHARS
      )
      {
        return false;
      }

      try
      {
        return (
          sameText(world.strFrame, cell) &&
          (pad == null || sameText(world.strPad, pad)) &&
          Boolean(world.map.stage) &&
          !world.map.isPlaying &&
          Boolean(world.bitWalk) &&
          Boolean(world.myAvatar.pMC.stage) &&
          world.CHARS.contains(world.myAvatar.pMC) &&
          hasCellUserData(world)
        );
      }
      catch (e:Error)
      {
        return false;
      }

      return false;
    }


    [BridgeExport]
    [BridgeRustType("gameAction: GameAction")]
    public static function isActionAvailable(gameAction:String):Boolean
    {
      var game:Object = Main.Game;
      var _loc_2:* = undefined;
      var _loc_3:* = undefined;
      var _loc_4:* = undefined;
      var _loc_5:* = undefined;
      _loc_2 = game.world.lock[gameAction];
      _loc_3 = new Date();
      _loc_4 = _loc_3.getTime();
      _loc_5 = _loc_4 - _loc_2.ts;
      return _loc_5 < _loc_2.cd ? false : true;
    }

    [BridgeExport]
    public static function isMonsterAvailable(monsterMapId:Number):Boolean
    {
      var game:Object = Main.Game;
      var monster:Object = game.world.getMonster(monsterMapId);
      if (!monster)
      {
        return false;
      }

      return Boolean(monster.pMC) && monster.dataLeaf.intState > 0;
    }

    [BridgeExport]
    [BridgeTsReturnType("number[]")]
    public static function getAvailableMonsterMapIds():Array
    {
      var game:Object = Main.Game;
      var ids:Array = [];

      for each (var mon:Object in game.world.getMonstersByCell(game.world.strFrame))
      {
        if (!mon || !mon.dataLeaf)
        {
          continue;
        }

        var monsterMapId:Number = Number(mon.dataLeaf.MonMapID);
        if (!isNaN(monsterMapId) && isMonsterAvailable(monsterMapId))
        {
          ids.push(monsterMapId);
        }
      }

      return ids;
    }

    [BridgeIgnore]
    public static function monsterRow(mon:Object):Object
    {
      return {
        monMapId: Number(mon.dataLeaf.MonMapID),
        monsterId: Number(mon.objData.MonID),
        name: String(mon.objData.strMonName || ""),
        race: String(mon.objData.sRace || ""),
        cell: String(mon.dataLeaf.strFrame || ""),
        hp: Number(mon.dataLeaf.intHP),
        maxHp: Number(mon.dataLeaf.intHPMax),
        level: Number(mon.dataLeaf.iLvl || mon.objData.iLvl),
        state: int(mon.dataLeaf.intState),
        available: Boolean(mon.pMC) && mon.dataLeaf.intState > 0,
        aggressive: mon.objData.bRed == 1,
        auras: Auras.rows(mon.dataLeaf)
      };
    }

    [BridgeExport]
    public static function getMonsters():Array
    {
      var game:Object = Main.Game;
      var monsters:Array = [];
      if (!game || !game.world || !game.world.monsters)
      {
        return monsters;
      }

      for each (var mon:Object in game.world.monsters)
      {
        if (!mon || !mon.objData || !mon.dataLeaf)
        {
          continue;
        }

        monsters.push(monsterRow(mon));
      }

      return monsters;
    }

    [BridgeExport]
    public static function requestMonsterDrops(monsterMapId:Number):Boolean
    {
      var game:Object = Main.Game;
      if (!game || !game.world || isNaN(monsterMapId) || monsterMapId <= 0)
      {
        return false;
      }

      for each (var mon:Object in game.world.getMonstersByCell(game.world.strFrame))
      {
        if (mon && mon.dataLeaf && Number(mon.dataLeaf.MonMapID) == monsterMapId)
        {
          game.sfc.sendXtMessage("zm", "getMonsterDrops", [monsterMapId], "str", game.world.curRoom);
          return true;
        }
      }

      return false;
    }

    private static function getMonsterByName(name:String):Object
    {
      if (!name)
      {
        return null;
      }

      name = name.replace(/^\s+|\s+$/g, "").toLowerCase();
      if (name == "")
      {
        return null;
      }

      var game:Object = Main.Game;
      for each (var mon:Object in game.world.getMonstersByCell(game.world.strFrame))
      {
        if (mon.pMC)
        {
          var monsterName:String = mon.pMC.pname.ti.text.replace(/^\s+|\s+$/g, "").toLowerCase();
          if ((monsterName == name || name == "*") && mon.dataLeaf.intState > 0)
          {
            return mon;
          }
        }
      }

      return null;
    }


    private static function getMonsterByMonsterMapId(monsterMapId:Number):Object
    {
      if (isNaN(monsterMapId) || monsterMapId <= 0)
      {
        return null;
      }

      var game:Object = Main.Game;
      for each (var mon:Object in game.world.getMonstersByCell(game.world.strFrame))
      {
        if (mon.pMC)
        {
          if (mon != null && mon.dataLeaf != null && mon.dataLeaf.MonMapID == monsterMapId)
          {
            return mon;
          }
        }
      }

      return null;
    }


    [BridgeTsParamType("selector: FlashTypes.MonsterSelector")]
    [BridgeExport]
    public static function getMonster(selector:Object):Object
    {
      if (!selector)
      {
        return null;
      }

      if ("name" in selector)
      {
        return getMonsterByName(getStringSelector(selector, "name"));
      }

      if ("monsterMapId" in selector)
      {
        return getMonsterByMonsterMapId(getNumberSelector(selector, "monsterMapId"));
      }

      return null;
    }


    [BridgeExport]
    public static function getCells():Array
    {
      var game:Object = Main.Game;
      var cells:Array = [];
      for each (var cell:Object in game.world.map.currentScene.labels)
      {
        cells.push(cell.name);
      }
      return cells;
    }


    [BridgeExport]
    public static function getCellPads():Array
    {
      var game:Object = Main.Game;
      var cellPads:Array = new Array();
      var cellPadsCnt:int = game.world.map.numChildren;
      for (var i:int = 0; i < cellPadsCnt; ++i)
      {
        var child:DisplayObject = game.world.map.getChildAt(i);
        if (padNames.test(child.name))
        {
          cellPads.push(child.name);
        }
      }

      return cellPads;
    }

    [BridgeExport]
    public static function reload():void
    {
      var game:Object = Main.Game;
      game.world.reloadCurrentMap();
    }


    [BridgeExport]
    public static function loadSwf(swf:String):void
    {
      var game:Object = Main.Game;
      game.world.loadMap(swf);
    }


    [BridgeExport]
    public static function getMapItem(itemId:int):void
    {
      if (!itemId)
      {
        return;
      }

      var game:Object = Main.Game;
      game.world.getMapItem(itemId);
    }


    [BridgeExport]
    public static function setSpawnPoint(cell:String = null, pad:String = null):void
    {
      var game:Object = Main.Game;
      if (!cell)
      {
        cell = game.world.strFrame;
      }

      if (!pad)
      {
        pad = game.world.strPad;
      }

      game.world.setSpawnPoint(cell, pad);
    }
  }
}
