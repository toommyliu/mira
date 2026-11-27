package mira.ui {
  import flash.display.Shape;
  import flash.geom.Matrix;
  import flash.geom.Point;
  import flash.geom.Rectangle;

  public class Wipe extends Shape {
    private static const OPACITY:Number = 0.55;
    private static const CORNER:Number = 0.24;
    private static const CORNER_STEPS:int = 6;

    private var outline:Vector.<Point> = new Vector.<Point>();
    private var area:Rectangle;
    private var rows:int = -1;

    public function Wipe(area:Rectangle, placement:Matrix) {
      this.area = area;
      transform.matrix = placement;
      var radius:Number = Math.min(area.width, area.height) * CORNER;
      corner(area.right - radius, area.top + radius, radius, -90);
      corner(area.right - radius, area.bottom - radius, radius, 0);
      corner(area.left + radius, area.bottom - radius, radius, 90);
      corner(area.left + radius, area.top + radius, radius, 180);
    }

    public function set fraction(value:Number):void {
      var next:int = Math.round(area.height * Math.max(0, Math.min(1, value)));
      if (next == rows) {
        return;
      }

      rows = next;
      graphics.clear();
      var below:Vector.<Point> = clip(area.bottom - rows);
      if (below.length < 3) {
        return;
      }
      graphics.beginFill(0x000000, OPACITY);
      graphics.moveTo(below[0].x, below[0].y);
      for (var i:int = 1; i < below.length; i++) {
        graphics.lineTo(below[i].x, below[i].y);
      }
      graphics.endFill();
    }

    private function corner(cx:Number, cy:Number, radius:Number, from:Number):void {
      for (var step:int = 0; step <= CORNER_STEPS; step++) {
        var angle:Number = (from + 90 * step / CORNER_STEPS) * Math.PI / 180;
        outline.push(new Point(cx + radius * Math.cos(angle), cy + radius * Math.sin(angle)));
      }
    }

    private function clip(top:Number):Vector.<Point> {
      var kept:Vector.<Point> = new Vector.<Point>();
      for (var i:int = 0; i < outline.length; i++) {
        var a:Point = outline[i];
        var b:Point = outline[(i + 1) % outline.length];
        if (a.y >= top) {
          kept.push(a);
        }
        if ((a.y >= top) != (b.y >= top)) {
          kept.push(new Point(a.x + (b.x - a.x) * (top - a.y) / (b.y - a.y), top));
        }
      }
      return kept;
    }
  }
}
