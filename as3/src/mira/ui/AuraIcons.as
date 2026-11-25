package mira.ui {
  import flash.events.MouseEvent;
  import flash.utils.Dictionary;

  import mira.Main;

  public class AuraIcons {
    private static const TICK_MS:int = 100;

    private static var icons:Dictionary = new Dictionary();
    private static var hovered:AuraIcon;
    private static var shownTip:String;
    private static var frame:int;
    private static var lastTick:int;
    private static var lastChildren:int = -1;

    public static function step(game:*, now:int):void {
      var children:int = game == null ? 0 : childrenOf(game.pAurasUI) + childrenOf(game.tAurasUI);
      if (children == lastChildren && now - lastTick < TICK_MS) {
        return;
      }

      lastTick = now;
      frame++;
      if (game != null && game.world != null) {
        cover(game.pAurasUI, game.world, now);
        cover(game.tAurasUI, game.world, now);
      }
      for (var cell:* in icons) {
        if (icons[cell].stamp != frame) {
          drop(cell);
        }
      }
      lastChildren = game == null ? 0 : childrenOf(game.pAurasUI) + childrenOf(game.tAurasUI);
    }

    public static function clear():void {
      for (var cell:* in icons) {
        drop(cell);
      }
      lastChildren = -1;
    }

    private static function childrenOf(hud:*):int {
      return hud == null || hud.parent == null ? 0 : hud.numChildren;
    }

    private static function cover(hud:*, world:*, now:int):void {
      if (hud == null || hud.parent == null) {
        return;
      }

      for (var i:int = 0; i < hud.numChildren; i++) {
        var cell:* = hud.getChildAt(i);
        if (cell is AuraIcon || !("h" in cell) || cell.h == null) {
          continue;
        }

        var icon:AuraIcon = icons[cell];
        if (icon == null) {
          icon = icons[cell] = new AuraIcon();
          icon.addEventListener(MouseEvent.ROLL_OVER, onOver);
          icon.addEventListener(MouseEvent.ROLL_OUT, onOut);
          hud.addChild(icon);
        }
        cell.visible = false;
        icon.x = cell.x + 1;
        icon.y = cell.y;
        icon.stamp = frame;
        icon.show(world, String(cell.nam), cell.h, now);
        if (icon == hovered) {
          showTip(icon);
        }
      }
    }

    private static function drop(cell:*):void {
      var icon:AuraIcon = icons[cell];
      icon.removeEventListener(MouseEvent.ROLL_OVER, onOver);
      icon.removeEventListener(MouseEvent.ROLL_OUT, onOut);
      if (icon == hovered) {
        closeTip();
      }
      if (icon.parent != null) {
        icon.parent.removeChild(icon);
      }
      cell.visible = true;
      delete icons[cell];
    }

    private static function onOver(event:MouseEvent):void {
      var game:* = Main.Game;
      var off:* = Main.resolvePath(game, ["litePreference", "data", "dOptions", "disAuraTips"], true);
      if (off == true || game.ui == null || game.ui.ToolTip == null) {
        return;
      }

      hovered = event.currentTarget as AuraIcon;
      shownTip = null;
      showTip(hovered);
    }

    private static function showTip(icon:AuraIcon):void {
      var game:* = Main.Game;
      var tip:String = "<b>" + html(icon.nam) + (icon.count > 1 ? " ×" + icon.count : "") + "</b>";
      var description:* = game.world == null || game.world.auraDescs == null ? null : game.world.auraDescs[icon.nam];
      if (description != null && description != "") {
        tip += "\n" + html(String(description));
      }
      if (tip != shownTip) {
        shownTip = tip;
        game.ui.ToolTip.openWith({str: tip});
      }
    }

    private static function onOut(event:MouseEvent):void {
      if (event.currentTarget == hovered) {
        closeTip();
      }
    }

    private static function closeTip():void {
      hovered = null;
      var tooltip:* = Main.resolvePath(Main.Game, ["ui", "ToolTip"], true);
      if (tooltip != null) {
        tooltip.close();
      }
    }

    private static function html(text:String):String {
      return text.split("&").join("&amp;").split("<").join("&lt;").split(">").join("&gt;");
    }
  }
}
