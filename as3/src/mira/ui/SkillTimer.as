package mira.ui {
  import flash.display.Bitmap;
  import flash.display.DisplayObject;
  import flash.filters.GlowFilter;
  import flash.geom.Rectangle;
  import flash.text.TextField;
  import flash.text.TextFieldAutoSize;
  import flash.text.TextFormat;

  public class SkillTimer {
    private static const OUTLINE:Array = [new GlowFilter(0x000000, 0.9, 2, 2, 8, 1)];
    private static const SIZE:int = 14;
    private static const CAP_HEIGHT:Number = 0.72;

    public var stamp:int;

    private var dimmed:Bitmap;
    private var wipe:Wipe;
    private var center:Rectangle;
    private var text:TextField;
    private var shown:int = -1;
    private var empty:Boolean;
    private var gameLabel:TextField;

    public function SkillTimer(icon:*, dimmed:Bitmap, empty:Boolean, gameLabel:TextField) {
      this.dimmed = dimmed;
      this.empty = empty;
      this.gameLabel = gameLabel;
      var frame:DisplayObject = icon.bg != null ? icon.bg : icon;
      var area:Rectangle = frame.getBounds(icon);
      area.inflate(-1, -1);
      wipe = new Wipe(area, dimmed.transform.matrix);
      dimmed.parent.addChildAt(wipe, dimmed.parent.getChildIndex(dimmed) + 1);
      dimmed.visible = false;
      center = frame.getBounds(dimmed.parent);
    }

    public function update(left:Number, remaining:Number):void {
      wipe.fraction = empty ? 0 : left;
      var labelShown:Boolean = gameLabel != null && gameLabel.visible;
      if (labelShown && gameLabel.alpha != 0) {
        gameLabel.alpha = 0;
      }

      var next:int = !labelShown || empty || remaining <= 0 ? 0 : remaining >= 10000 ? 1000 + Math.ceil(remaining / 1000) : Math.ceil(remaining / 100);
      if (next == shown) {
        return;
      }

      shown = next;
      text ||= createText();
      text.visible = next != 0;
      if (next == 0) {
        return;
      }
      text.text = next > 1000 ? String(next - 1000) : (next / 10).toFixed(1);
      text.x = Math.round(center.x + (center.width - text.width) / 2);
      var cap:Number = SIZE * CAP_HEIGHT;
      var glyphTop:Number = 2 + text.getLineMetrics(0).ascent - cap;
      text.y = Math.round(center.y + (center.height - cap) / 2 - glyphTop);
    }

    private function createText():TextField {
      var field:TextField = new TextField();
      field.defaultTextFormat = new TextFormat("Arial", SIZE, 0xFFFFFF, true);
      field.filters = OUTLINE;
      field.autoSize = TextFieldAutoSize.LEFT;
      field.selectable = false;
      field.mouseEnabled = false;
      dimmed.parent.addChild(field);
      return field;
    }

    public function remove():void {
      if (wipe.parent != null) {
        wipe.parent.removeChild(wipe);
      }
      if (text != null && text.parent != null) {
        text.parent.removeChild(text);
      }
      dimmed.visible = true;
      if (gameLabel != null) {
        gameLabel.alpha = 1;
      }
    }
  }
}
