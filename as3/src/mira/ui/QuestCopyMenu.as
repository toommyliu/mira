package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.MovieClip;
  import flash.display.Stage;
  import flash.events.Event;
  import flash.events.MouseEvent;
  import flash.geom.ColorTransform;
  import flash.geom.Point;
  import flash.system.ApplicationDomain;
  import flash.system.System;
  import flash.text.TextField;

  import mira.Main;
  import mira.game.Quests;

  public class QuestCopyMenu {
    private static const ROW_COLOR:uint = 0x999999;
    private static const HOVER_COLOR:uint = 0xFFFFFF;
    private static const HIGHLIGHT_COLOR:uint = 0xCCFF33;
    private static const PICKED_COLOR:uint = 0xFFCC33;
    private static const ROW_HEIGHT:int = 14;
    private static const GAME_ROWS:int = 13;
    private static const GAME_WIDTH:int = 960;
    private static const GAME_HEIGHT:int = 500;

    private static var frameClass:Class;
    private static var rowClass:Class;
    private static var options:Array = [];

    public static function init(stage:Stage, domain:ApplicationDomain):void {
      frameClass = domain.getDefinition("QFrameMC") as Class;
      rowClass = domain.getDefinition("cProto") as Class;
      stage.addEventListener(Event.ADDED, onAdded);
    }

    // Ruffle skips its own context menu only for right-clicks something listens for, so only quest windows listen.
    private static function onAdded(event:Event):void {
      if (event.target is frameClass) {
        event.target.addEventListener(MouseEvent.RIGHT_MOUSE_DOWN, onRightMouseDown);
      }
    }

    private static function onRightMouseDown(event:MouseEvent):void {
      var found:Array = optionsAt(event.currentTarget as MovieClip, event);
      if (found.length > 0) {
        open(found);
      }
    }

    private static function optionsAt(frame:MovieClip, event:MouseEvent):Array {
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
        option("Copy Name", String(item.name)),
        option("Copy Item ID", String(item.itemId)),
        option("Copy Quest ID", String(quest.QuestID))
      ];
    }

    private static function rewardOptions(item:Object, quest:Object):Array {
      if (!item) {
        return [];
      }

      return itemOptions(item, quest).concat([option("Copy Reward", JSON.stringify(item, null, 2), "reward " + item.name)]);
    }

    private static function questOptions(quest:Object):Array {
      if (!quest) {
        return [];
      }

      return [
        option("Copy Name", String(quest.sName)),
        option("Copy Quest ID", String(quest.QuestID)),
        option("Copy Quest", JSON.stringify(Quests.row(quest), null, 2), "quest " + quest.sName)
      ];
    }

    private static function option(label:String, value:String, note:String = null):Object {
      return {label: label, value: value, note: note || value};
    }

    private static function open(found:Array):void {
      var menu:MovieClip = Main.Game.ui.cMenu;
      var list:MovieClip = menu.cnt;
      for (var index:int = 0; index < GAME_ROWS; index++) {
        var stale:DisplayObject = list.getChildByName("i" + index);
        if (stale) {
          list.removeChild(stale);
        }
      }

      options = found.concat([option("Close", null)]);
      list.mHi.visible = false;
      tint(list.mHi, HIGHLIGHT_COLOR);

      var widest:Number = 0;
      var row:MovieClip;
      for (index = 0; index < options.length; index++) {
        row = list.addChild(new rowClass()) as MovieClip;
        row.name = "i" + index;
        row.y = list.iproto.y + index * ROW_HEIGHT;
        row.buttonMode = true;
        row.ti.text = options[index].label;
        row.ti.width = row.ti.textWidth + 5;
        row.hit.width = row.ti.x + row.ti.textWidth + 2;
        widest = Math.max(widest, row.ti.textWidth);
        tint(row, ROW_COLOR);
        row.addEventListener(MouseEvent.MOUSE_OVER, onRowOver);
        row.addEventListener(MouseEvent.CLICK, onRowClick);
      }

      list.bg.height = row.y + 26;
      list.bg.width = widest + 20;

      var ui:MovieClip = menu.parent as MovieClip;
      ui.setChildIndex(menu, ui.numChildren - 1);
      menu.x = ui.mouseX - 5;
      menu.y = ui.mouseY - 5;
      if (menu.x + list.bg.width > GAME_WIDTH) {
        menu.x = ui.mouseX - list.bg.width;
      }

      if (menu.y + list.bg.height > GAME_HEIGHT) {
        menu.y = GAME_HEIGHT - list.bg.height;
      }

      menu.gotoAndPlay("in");
    }

    private static function onRowOver(event:MouseEvent):void {
      var hovered:MovieClip = event.currentTarget as MovieClip;
      var list:MovieClip = hovered.parent as MovieClip;
      for (var index:int = 0; index < options.length; index++) {
        var row:DisplayObject = list.getChildByName("i" + index);
        tint(row, row == hovered ? HOVER_COLOR : ROW_COLOR);
      }

      list.mHi.visible = true;
      list.mHi.y = hovered.y + 3;
    }

    private static function onRowClick(event:MouseEvent):void {
      var row:MovieClip = event.currentTarget as MovieClip;
      var option:Object = options[int(row.name.substr(1))];
      tint(row.parent["mHi"], PICKED_COLOR);
      Main.Game.ui.cMenu.fClose();
      if (option.value != null) {
        System.setClipboard(option.value);
        Main.Game.addUpdate("Copied: " + option.note);
      }
    }

    private static function tint(target:DisplayObject, color:uint):void {
      var transform:ColorTransform = target.transform.colorTransform;
      transform.color = color;
      target.transform.colorTransform = transform;
    }
  }
}
