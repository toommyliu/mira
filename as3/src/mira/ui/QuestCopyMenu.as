package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.MovieClip;
  import flash.events.MouseEvent;
  import flash.geom.Point;
  import flash.system.ApplicationDomain;
  import flash.text.TextField;

  import mira.Main;
  import mira.game.Quests;

  public class QuestCopyMenu {
    private static var frameClass:Class;

    public static function init(domain:ApplicationDomain):void {
      frameClass = domain.getDefinition("QFrameMC") as Class;
    }

    public static function listenerFor(target:Object):Object {
      return target is frameClass ? target : null;
    }

    public static function optionsAt(frame:MovieClip, event:MouseEvent):Array {
      var quest:Object = frame.qData;
      for (var node:DisplayObject = event.target as DisplayObject; node && node != frame; node = node.parent) {
        if (node.hasOwnProperty("ItemID")) {
          return rewardOptions(reward(quest, node["ItemID"]), quest);
        }

        if (node.hasOwnProperty("QuestID")) {
          return questOptions(Main.Game.world.questTree[node["QuestID"]]);
        }
      }

      var core:MovieClip = frame.cnt.core;
      if (!quest || !core) {
        return [];
      }

      if (event.target == core.strReq) {
        return itemOptions(requirement(quest, core.strReq, event), quest);
      }

      if (event.target == frame.cnt.strTitle) {
        return questOptions(quest);
      }

      return [];
    }

    private static function reward(quest:Object, itemId:int):Object {
      for each (var row:Object in quest ? Quests.row(quest).rewards : null) {
        if (int(row.itemId) == itemId) {
          return row;
        }
      }

      return null;
    }

    // QFrameMC.showQuestDetail puts one turn-in per line, so the line breaks before the clicked line give its index.
    private static function requirement(quest:Object, text:TextField, event:MouseEvent):Object {
      var local:Point = text.globalToLocal(new Point(event.stageX, event.stageY));
      var line:int = text.getLineIndexAtPoint(local.x, local.y);
      if (line < 0) {
        return null;
      }

      var index:int = text.text.substring(0, text.getLineOffset(line)).split(/\r\n?|\n/).length - 1;
      return Quests.row(quest).requirements[index];
    }

    private static function itemOptions(item:Object, quest:Object):Array {
      if (!item) {
        return [];
      }

      return [
        CopyMenu.option("Copy Name", String(item.name)),
        CopyMenu.option("Copy Item ID", String(item.itemId)),
        CopyMenu.option("Copy Quest ID", String(quest.QuestID))
      ];
    }

    private static function rewardOptions(item:Object, quest:Object):Array {
      if (!item) {
        return [];
      }

      return itemOptions(item, quest).concat([CopyMenu.option("Copy Reward", JSON.stringify(item, null, 2), "reward " + item.name)]);
    }

    private static function questOptions(quest:Object):Array {
      if (!quest) {
        return [];
      }

      return [
        CopyMenu.option("Copy Name", String(quest.sName)),
        CopyMenu.option("Copy Quest ID", String(quest.QuestID)),
        CopyMenu.option("Copy Quest", JSON.stringify(Quests.row(quest), null, 2), "quest " + quest.sName)
      ];
    }
  }
}
