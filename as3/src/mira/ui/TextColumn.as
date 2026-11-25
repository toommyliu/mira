package mira.ui {
  import flash.display.Sprite;
  import flash.geom.Point;
  import flash.utils.Dictionary;

  public class TextColumn {
    public var enabled:Boolean;

    private var lanes:Dictionary = new Dictionary();
    private var layer:Sprite;
    private var offsetX:Number;
    private var offsetY:Number;
    private var align:Number;
    private var down:Boolean;
    private var maxLines:int;

    public function TextColumn(layer:Sprite, offsetX:Number, offsetY:Number, align:Number, down:Boolean, maxLines:int) {
      this.layer = layer;
      this.offsetX = offsetX;
      this.offsetY = offsetY;
      this.align = align;
      this.down = down;
      this.maxLines = maxLines;
    }

    public function newest(target:*):FloatingText {
      var lines:Vector.<FloatingText> = lanes[target];
      return lines == null || lines.length == 0 ? null : lines[lines.length - 1];
    }

    public function find(target:*, key:String):FloatingText {
      for each (var line:FloatingText in lanes[target]) {
        if (line.key == key) {
          return line;
        }
      }
      return null;
    }

    public function add(target:*):FloatingText {
      var lines:Vector.<FloatingText> = lanes[target];
      if (lines == null) {
        lines = lanes[target] = new Vector.<FloatingText>();
      }
      if (lines.length >= maxLines) {
        lines.shift().drop();
      }

      var line:FloatingText = FloatingText.take(layer);
      lines.push(line);
      return line;
    }

    public function remove(target:*, line:FloatingText):void {
      var lines:Vector.<FloatingText> = lanes[target];
      lines.splice(lines.indexOf(line), 1);
      line.drop();
      restack(target);
    }

    public function restack(target:*):void {
      var lines:Vector.<FloatingText> = lanes[target];
      if (lines == null) {
        return;
      }

      var stack:Number = 0;
      for (var i:int = lines.length - 1; i >= 0; i--) {
        lines[i].targetStack = stack;
        stack += lines[i].lineHeight;
      }
    }

    public function step(now:int, dt:int):void {
      for (var target:* in lanes) {
        var lines:Vector.<FloatingText> = lanes[target];
        var removed:Boolean = false;
        for (var i:int = lines.length - 1; i >= 0; i--) {
          if (lines[i].expired(now)) {
            lines.splice(i, 1)[0].drop();
            removed = true;
          }
        }

        var clip:* = target.pMC;
        if (lines.length == 0 || clip == null || clip.stage == null || clip.pname == null) {
          clearLane(target);
          continue;
        }
        if (removed) {
          restack(target);
        }

        var anchor:Point = layer.globalToLocal(clip.localToGlobal(new Point(offsetX, clip.pname.y + offsetY)));
        for each (var line:FloatingText in lines) {
          line.step(now, dt, anchor.x, anchor.y, align, down);
        }
      }
    }

    public function clear():void {
      for (var target:* in lanes) {
        clearLane(target);
      }
    }

    private function clearLane(target:*):void {
      for each (var line:FloatingText in lanes[target]) {
        line.drop();
      }
      delete lanes[target];
    }
  }
}
