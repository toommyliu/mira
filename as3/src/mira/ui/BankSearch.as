package mira.ui {
  import flash.events.Event;
  import flash.system.ApplicationDomain;

  import mira.Main;
  import mira.game.GearBoosts;

  public class BankSearch {
    private static const FRAME:String = "LPFFrameBankAQ";
    private static const TAGGED:String = "_miraTagged";
    private static const SEPARATOR:String = "\x01";
    private static const FIRST_FORGE_PROC:int = 7;

    private static const BOOST_LABELS:Object = {
      classPoints: "cp",
      gold: "gold",
      rep: "rep",
      xp: "xp"
    };

    private static const PROCS:Object = {
      2: "Spiral Carve",
      3: "Awe Blast",
      4: "Health Vamp",
      5: "Mana Vamp",
      6: "Powerword DIE",
      7: "Lacerate",
      8: "Smite",
      9: "Valiance",
      10: "Arcana's Concerto",
      11: "Acheron",
      12: "Elysium",
      13: "Praxis",
      14: "Dauntless",
      15: "Ravenous"
    };

    private static var frameClass:Object;

    public static function setEnabled(enabled:Boolean):void {
      var main:Main = Main.getInstance();
      if (!enabled) {
        main.getStage().removeEventListener(Event.ENTER_FRAME, onFrame);
        restore();
        return;
      }

      var domain:ApplicationDomain = main.getGameDomain();
      frameClass = domain.hasDefinition(FRAME) ? domain.getDefinition(FRAME) : null;
      if (frameClass == null || !frameClass.hasOwnProperty("active")) {
        main.emitDebug("Bank search is off because this client's bank has no " + FRAME + ".active.");
        return;
      }

      main.getStage().addEventListener(Event.ENTER_FRAME, onFrame);
    }

    private static function onFrame(event:Event):void {
      try {
        var frame:Object = frameClass.active;
        if (frame != null && tagAll()) {
          frame.notify({eventType: "refreshInventory"});
        }
      } catch (error:Error) {
        Main.getInstance().emitDebug("Bank search turned itself off: " + error.message);
        setEnabled(false);
      }
    }

    private static function lists():Array {
      var world:Object = Main.Game.world;
      return [world.bankinfo.BankArray, world.myAvatar.items, world.myAvatar.houseitems];
    }

    private static function tagAll():Boolean {
      var patterns:Object = patternsById();
      var changed:Boolean = false;
      for each (var list:Array in lists()) {
        for each (var item:Object in list) {
          var enhancement:String = item == null ? null : item.EnhPatternID + ":" + item.ProcID;
          if (item != null && item._aqk is String && item[TAGGED] !== enhancement) {
            changed = tag(item, patterns, enhancement) || changed;
          }
        }
      }

      return changed;
    }

    private static function tag(item:Object, patterns:Object, enhancement:String):Boolean {
      var name:String = String(item.sName).toLowerCase();
      var current:String = String(item._aqn);
      if (current !== name && current.indexOf(name + SEPARATOR) != 0) {
        throw new Error("the bank no longer searches item names the way Mira expects");
      }

      item[TAGGED] = enhancement;
      var tags:Array = enhancementTags(item, patterns).concat(boostTags(item.sMeta));
      var tagged:String = tags.length == 0 ? name : name + SEPARATOR + tags.join(SEPARATOR).toLowerCase();
      if (tagged == current) {
        return false;
      }

      item._aqn = tagged;
      return true;
    }

    private static function restore():void {
      try {
        for each (var list:Array in lists()) {
          for each (var item:Object in list) {
            if (item != null && item.hasOwnProperty(TAGGED)) {
              item._aqn = String(item.sName).toLowerCase();
              delete item[TAGGED];
            }
          }
        }

        var frame:Object = frameClass == null ? null : frameClass.active;
        if (frame != null) {
          frame.notify({eventType: "refreshInventory"});
        }
      } catch (error:Error) {
        Main.getInstance().emitDebug("Bank search couldn't restore item names: " + error.message);
      }
    }

    private static function patternsById():Object {
      var patterns:Object = {};
      for each (var pattern:Object in Main.Game.world.enhp) {
        patterns[pattern.ID] = pattern;
      }

      return patterns;
    }

    private static function enhancementTags(item:Object, patterns:Object):Array {
      var tags:Array = [];
      var pattern:Object = patterns[item.EnhPatternID];
      if (pattern != null) {
        tags.push(pattern.sName);
        if (pattern.DIS) {
          tags.push("Forge");
        }
      }

      var proc:int = int(item.ProcID);
      var info:Object = proc > 0 && Main.Game.procInfo ? Main.Game.procInfo[String(proc)] : null;
      if (PROCS[proc]) {
        tags.push(PROCS[proc]);
      }
      if (info && info.nam && info.nam != PROCS[proc]) {
        tags.push(info.nam);
      }
      if (proc >= FIRST_FORGE_PROC) {
        tags.push("Forge");
      }

      return tags;
    }

    private static function boostTags(meta:*):Array {
      var tags:Array = [];
      for each (var boost:Object in GearBoosts.parse(meta)) {
        var label:String = boost.kind == "damage" ? "dmg " + boost.against : BOOST_LABELS[boost.kind];
        tags.push(label + " +" + boost.percent + "%");
      }

      return tags;
    }
  }
}
