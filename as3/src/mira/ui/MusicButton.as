package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.DisplayObjectContainer;
  import flash.display.Stage;
  import flash.events.Event;

  import mira.Main;

  public class MusicButton {
    private static const NAME:String = "btnSong";

    private static var hidden:DisplayObject;

    public static function setEnabled(enabled:Boolean):void {
      var stage:Stage = Main.getInstance().getStage();
      if (enabled) {
        stage.addEventListener(Event.ADDED, onAdded);
        hideIn(map());
        return;
      }

      stage.removeEventListener(Event.ADDED, onAdded);
      if (hidden != null) {
        hidden.visible = true;
        hidden = null;
      }
    }

    private static function onAdded(event:Event):void {
      var target:DisplayObject = DisplayObject(event.target);
      if (target.name == NAME) {
        hide(target);
      } else if (target === map()) {
        hideIn(DisplayObjectContainer(target));
      }
    }

    private static function map():DisplayObjectContainer {
      return Main.resolvePath(Main.Game, ["world", "map"], true) as DisplayObjectContainer;
    }

    private static function hideIn(map:DisplayObjectContainer):void {
      var button:DisplayObject = map == null ? null : map.getChildByName(NAME);
      if (button != null) {
        hide(button);
      }
    }

    private static function hide(button:DisplayObject):void {
      button.visible = false;
      hidden = button;
    }
  }
}
