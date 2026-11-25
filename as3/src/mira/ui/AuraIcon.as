package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.MovieClip;
  import flash.display.Shape;
  import flash.display.Sprite;
  import flash.filters.GlowFilter;
  import flash.geom.Rectangle;
  import flash.text.TextField;
  import flash.text.TextFieldAutoSize;
  import flash.text.TextFormat;

  public class AuraIcon extends Sprite {
    private static const SIZE:int = 24;
    private static const ART:int = 20;
    private static const BAR:int = 20;
    private static const OUTLINE:Array = [new GlowFilter(0x000000, 1, 2, 2, 8, 1)];

    public var stamp:int;
    public var nam:String;
    public var count:int;

    private var art:DisplayObject;
    private var iconKey:String;
    private var bar:Shape = new Shape();
    private var stacks:TextField = new TextField();
    private var barWidth:int = -1;
    private var expired:Boolean;

    public function AuraIcon() {
      mouseChildren = false;
      graphics.beginFill(0x111317, 0.9);
      graphics.lineStyle(1, 0xFFFFFF, 0.14);
      graphics.drawRoundRect(0, 0, SIZE, SIZE, 6, 6);
      graphics.endFill();

      bar.graphics.beginFill(0xFFFFFF, 0.85);
      bar.graphics.drawRect(0, 0, BAR, 2);
      bar.graphics.endFill();
      bar.x = (SIZE - BAR) / 2;
      bar.y = SIZE - 4;
      bar.visible = false;
      addChild(bar);

      stacks.defaultTextFormat = new TextFormat("Arial", 10, 0xFFFFFF, true);
      stacks.autoSize = TextFieldAutoSize.RIGHT;
      stacks.selectable = false;
      stacks.mouseEnabled = false;
      stacks.filters = OUTLINE;
      stacks.visible = false;
      addChild(stacks);
    }

    public function show(world:*, name:String, timer:Object, now:int):void {
      nam = name;
      var key:String = timer.icon == null ? "" : String(timer.icon);
      if (key != iconKey) {
        iconKey = key;
        setArt(world, key);
      }

      var n:int = int(timer.n);
      if (n != count) {
        count = n;
        stacks.visible = n > 1;
        stacks.text = String(n);
        stacks.x = SIZE - stacks.width;
        stacks.y = SIZE - stacks.height + 1;
      }

      var timed:Boolean = timer.end > 0 && timer.dur > 0;
      var left:Number = timed ? Math.max(0, Math.min(1, (timer.end - now) / (timer.dur * 1000))) : 0;
      var width:int = Math.round(BAR * left);
      if (width != barWidth) {
        barWidth = width;
        bar.visible = width > 0;
        if (width > 0) {
          bar.scaleX = width / BAR;
        }
      }

      var done:Boolean = timed && left == 0;
      if (done != expired && art != null) {
        expired = done;
        art.alpha = done ? 0.4 : 1;
      }
    }

    private function setArt(world:*, key:String):void {
      if (art != null) {
        if (art is MovieClip) {
          MovieClip(art).stop();
        }
        removeChild(art);
        art = null;
      }

      var link:String = key.split(",").pop();
      var source:Class = null;
      try {
        if (link != "" && link != "undefined") {
          source = world.getClass(link) as Class;
        }
        source ||= world.getClass("isp2") as Class;
        art = source == null ? null : new source() as DisplayObject;
      }
      catch (error:Error) {
        art = null;
      }
      if (art == null) {
        return;
      }

      if (art is MovieClip) {
        MovieClip(art).stop();
        MovieClip(art).mouseEnabled = MovieClip(art).mouseChildren = false;
      }
      var bounds:Rectangle = art.getBounds(art);
      var scale:Number = ART / Math.max(bounds.width, bounds.height, 1);
      art.scaleX = art.scaleY = scale;
      art.x = SIZE / 2 - (bounds.x + bounds.width / 2) * scale;
      art.y = SIZE / 2 - (bounds.y + bounds.height / 2) * scale;
      expired = false;
      addChildAt(art, 0);
    }
  }
}
