package mira.ui {
  import flash.display.Stage;
  import flash.events.Event;
  import flash.text.TextField;
  import flash.utils.Dictionary;

  import mira.Main;

  public class Percentages {
    private static var bars:Dictionary = new Dictionary(true);

    public static function setEnabled(enabled:Boolean):void {
      var stage:Stage = Main.getInstance().getStage();
      if (enabled) {
        stage.addEventListener(Event.ENTER_FRAME, onFrame);
        return;
      }

      stage.removeEventListener(Event.ENTER_FRAME, onFrame);
      restore();
    }

    private static function onFrame(event:Event):void {
      var game:* = Main.Game;
      var avatar:* = Main.resolvePath(game, ["world", "myAvatar"], true);
      if (avatar == null || game.ui == null) {
        return;
      }

      label(game.ui.mcPortrait, avatar.dataLeaf);
      label(game.ui.mcPortraitTarget, avatar.target == null ? null : avatar.target.dataLeaf);
    }

    private static function label(portrait:*, data:*):void {
      if (portrait == null || data == null) {
        return;
      }

      var hp:Object = bar(portrait.HP, "strIntHP", null);
      var mp:Object = bar(portrait.MP, "strIntMP", hp.field);
      show(hp, data.intHP, data.intHPMax);
      show(mp, data.intMP, data.intMPMax);
    }

    private static function bar(clip:*, name:String, widthOf:TextField):Object {
      var found:Object = bars[clip];
      if (found != null) {
        return found;
      }

      var field:TextField = TextField(clip.getChildByName(name));
      var decoy:TextField = new TextField();
      decoy.text = field.text;
      clip[name] = decoy;
      found = {name: name, field: field, decoy: decoy, x: field.x, width: field.width, base: null, percent: -1};
      if (widthOf != null) {
        field.x = widthOf.x;
        field.width = widthOf.width;
      }
      bars[clip] = found;
      return found;
    }

    private static function show(bar:Object, value:int, max:int):void {
      var base:String = bar.decoy.text;
      var percent:int = value >= 0 && max > 0 ? Math.floor(100 * Math.min(value, max) / max) : -1;
      if (base == bar.base && percent == bar.percent) {
        return;
      }

      bar.base = base;
      bar.percent = percent;
      bar.field.text = percent < 0 ? base : base + " (" + percent + "%)";
    }

    private static function restore():void {
      for (var clip:Object in bars) {
        var bar:Object = bars[clip];
        clip[bar.name] = bar.field;
        bar.field.text = bar.decoy.text;
        bar.field.x = bar.x;
        bar.field.width = bar.width;
      }
      bars = new Dictionary(true);
    }
  }
}
