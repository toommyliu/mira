package mira.ui {
  import flash.display.Bitmap;
  import flash.events.Event;
  import flash.text.TextField;
  import flash.utils.Dictionary;
  import flash.utils.getTimer;

  import mira.Main;

  public class Cooldowns {
    private static const BAR:Array = ["ui", "mcInterface", "actBar"];
    private static const STATS:Array = ["myAvatar", "dataLeaf", "sta"];

    private static var timers:Dictionary = new Dictionary();
    private static var frame:int;
    private static var slotsOf:* = null;
    private static var slotsChildren:int = -1;
    private static var slots:Array = [];

    public static function setEnabled(enabled:Boolean):void {
      var stage:* = Main.getInstance().getStage();
      if (enabled) {
        stage.addEventListener(Event.EXIT_FRAME, onFrame);
        return;
      }

      stage.removeEventListener(Event.EXIT_FRAME, onFrame);
      AuraIcons.clear();
      for (var dimmed:* in timers) {
        drop(dimmed);
      }
    }

    private static function onFrame(event:Event):void {
      frame++;
      var game:* = Main.Game;
      var now:int = getTimer();
      var bar:* = Main.resolvePath(game, BAR, true);
      if (bar != null && game.world != null) {
        skills(game, bar, now);
      }
      AuraIcons.step(game, now);
      for (var dimmed:* in timers) {
        if (timers[dimmed].stamp != frame) {
          drop(dimmed);
        }
      }
    }

    private static function skills(game:*, bar:*, now:int):void {
      if (bar !== slotsOf || bar.numChildren != slotsChildren || detached(bar)) {
        slotsOf = bar;
        slotsChildren = bar.numChildren;
        slots = [];
        var slot:*;
        for (var i:int = 1; (slot = bar.getChildByName("i" + i)) != null; i++) {
          slots.push(slot);
        }
      }

      var haste:Number = NaN;
      for each (var icon:* in slots) {
        var dimmed:Bitmap = icon.icon2 as Bitmap;
        if (dimmed == null || dimmed.parent == null) {
          continue;
        }

        if (isNaN(haste)) {
          var stats:* = Main.resolvePath(game.world, STATS, true);
          var tha:Number = stats == null ? NaN : Number(stats.$tha);
          haste = isNaN(tha) ? 0 : Math.min(Math.max(tha, -1), 0.5);
        }
        var action:* = icon.actObj;
        var timer:SkillTimer = timers[dimmed];
        if (timer == null) {
          var empty:Boolean = action != null && action.ref == "i1" && action.sArg1 == "";
          timer = timers[dimmed] = new SkillTimer(icon, dimmed, empty, labelOf(bar, action));
        }
        timer.stamp = frame;

        var cooldown:Number = Math.round(icon.cd * (1 - haste));
        var remaining:Number = cooldown - (now - icon.tsg);
        timer.update(remaining / cooldown, remaining);
      }
    }

    private static function labelOf(bar:*, action:*):TextField {
      if (action == null || action.ref == null) {
        return null;
      }
      var ref:String = String(action.ref);
      var name:String = ref == "aa" ? "txtCD0" : ref == "i1" ? "txtCD5" : "txtCD" + ref.slice(1);
      return bar.getChildByName(name) as TextField;
    }

    private static function detached(bar:*):Boolean {
      for each (var slot:* in slots) {
        if (slot.parent !== bar) {
          return true;
        }
      }
      return false;
    }

    private static function drop(dimmed:*):void {
      timers[dimmed].remove();
      delete timers[dimmed];
    }
  }
}
