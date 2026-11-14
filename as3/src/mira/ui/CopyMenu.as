package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.DisplayObjectContainer;
  import flash.display.MovieClip;
  import flash.display.Stage;
  import flash.events.Event;
  import flash.events.MouseEvent;
  import flash.geom.ColorTransform;
  import flash.net.URLRequest;
  import flash.net.navigateToURL;
  import flash.system.ApplicationDomain;
  import flash.system.System;
  import flash.utils.Dictionary;

  import mira.Main;

  public class CopyMenu {
    private static const ROW_COLOR:uint = 0x999999;
    private static const HOVER_COLOR:uint = 0xFFFFFF;
    private static const HIGHLIGHT_COLOR:uint = 0xCCFF33;
    private static const PICKED_COLOR:uint = 0xFFCC33;
    private static const ROW_HEIGHT:int = 14;
    private static const GAME_ROWS:int = 13;
    private static const GAME_WIDTH:int = 960;
    private static const GAME_HEIGHT:int = 500;

    private static const MENUS:Array = [QuestCopyMenu, ItemCopyMenu, MonsterCopyMenu, ShopCopyMenu];

    private static var options:Array = [];
    private static var attached:Dictionary = new Dictionary(true);

    public static function init(domain:ApplicationDomain):void {
      for each (var menu:Object in MENUS) {
        menu.init(domain);
      }
    }

    public static function setEnabled(enabled:Boolean):void {
      var stage:Stage = Main.getInstance().getStage();
      if (enabled) {
        stage.addEventListener(Event.ADDED, onAdded);
        attachWithin(stage);
        return;
      }

      stage.removeEventListener(Event.ADDED, onAdded);
      for (var target:Object in attached) {
        target.removeEventListener(MouseEvent.RIGHT_MOUSE_DOWN, onRightMouseDown);
      }
      attached = new Dictionary(true);
    }

    // Ruffle skips its own context menu only for right-clicks something listens for, so copy menus listen on the windows they cover instead of the stage.
    private static function onAdded(event:Event):void {
      attach(event.target);
    }

    private static function attachWithin(container:DisplayObjectContainer):void {
      for (var index:int = 0; index < container.numChildren; index++) {
        var child:DisplayObject = container.getChildAt(index);
        attach(child);
        if (child is DisplayObjectContainer) {
          attachWithin(child as DisplayObjectContainer);
        }
      }
    }

    private static function attach(target:Object):void {
      for each (var menu:Object in MENUS) {
        var listener:Object = menu.listenerFor(target);
        if (listener) {
          listener.addEventListener(MouseEvent.RIGHT_MOUSE_DOWN, onRightMouseDown);
          attached[listener] = menu;
          return;
        }
      }
    }

    private static function onRightMouseDown(event:MouseEvent):void {
      var found:Array = attached[event.currentTarget].optionsAt(event.currentTarget, event);
      if (found.length > 0) {
        open(found);
      }
    }

    public static function option(label:String, value:String, note:String = null):Object {
      return {label: label, value: value, note: note || value};
    }

    public static function wiki(name:String):Object {
      return {label: "Open Wiki", url: "https://aqwwiki.wikidot.com/" + encodeURIComponent(name)};
    }

    public static function open(found:Array):void {
      var rowClass:Class = Main.getInstance().getGameDomain().getDefinition("cProto") as Class;
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
      if (option.url != null) {
        navigateToURL(new URLRequest(option.url), "_blank");
      } else if (option.value != null) {
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
