package mira.ui {
  import flash.display.Sprite;
  import flash.filters.GlowFilter;
  import flash.text.TextField;
  import flash.text.TextFieldAutoSize;
  import flash.text.TextFormat;

  public class FloatingText extends Sprite {
    private static const OUTLINE:Array = [new GlowFilter(0x000000, 0.9, 2, 2, 8, 1)];
    private static const FADE_IN:int = 80;
    private static const FADE_OUT:int = 250;
    private static const PULSE:int = 140;
    private static const PULSE_SCALE:Number = 0.15;
    private static const SETTLE_MS:Number = 90;

    private static var spare:Vector.<FloatingText> = new Vector.<FloatingText>();

    public var key:String;
    public var kind:String;
    public var amount:int;
    public var count:int;
    public var born:int;
    public var stack:Number;
    public var targetStack:Number;

    private var life:int;
    private var rise:Number;
    private var pulse:Boolean;
    private var opacity:Number;
    private var field:TextField = new TextField();
    private var format:TextFormat = new TextFormat("Arial", 13, 0xFFFFFF, true);
    private var suffixFormat:TextFormat = new TextFormat("Arial", 10);

    public function FloatingText() {
      mouseEnabled = false;
      mouseChildren = false;
      field.autoSize = TextFieldAutoSize.LEFT;
      field.selectable = false;
      field.filters = OUTLINE;
      addChild(field);
    }

    public static function take(layer:Sprite):FloatingText {
      var line:FloatingText = spare.length > 0 ? spare.pop() : new FloatingText();
      layer.addChild(line);
      return line;
    }

    public function drop():void {
      parent.removeChild(this);
      spare.push(this);
    }

    public function start(
      key:String,
      kind:String,
      color:uint,
      opacity:Number,
      life:int,
      rise:Number,
      pulse:Boolean,
      now:int
    ):void {
      this.key = key;
      this.kind = kind;
      this.opacity = opacity;
      this.life = life;
      this.rise = rise;
      this.pulse = pulse;
      amount = 0;
      count = 1;
      born = now;
      stack = 0;
      targetStack = 0;
      format.color = color;
      alpha = 0;
    }

    public function write(label:String, suffix:String, size:int):void {
      format.size = size;
      suffixFormat.size = Math.max(9, Math.round(size * 0.7));
      field.defaultTextFormat = format;
      field.text = suffix == null ? label : label + " " + suffix;
      if (suffix != null) {
        field.setTextFormat(suffixFormat, label.length, field.text.length);
      }
    }

    public function touch(now:int):void {
      born = Math.max(born, now - FADE_IN);
    }

    public function expired(now:int):Boolean {
      return now - born >= life;
    }

    public function get lineHeight():Number {
      return height / scaleY;
    }

    public function step(now:int, dt:int, anchorX:Number, anchorY:Number, align:Number, down:Boolean):void {
      var age:int = now - born;
      var eased:Number = 1 - Math.pow(1 - Math.min(1, age / life), 3);
      var grow:Number = pulse && age < PULSE ? PULSE_SCALE * (1 - age / PULSE) : 0;

      stack += (targetStack - stack) * Math.min(1, dt / SETTLE_MS);
      if (scaleX != 1 + grow) {
        scaleX = scaleY = 1 + grow;
      }
      alpha = opacity * Math.min(1, age / FADE_IN, (life - age) / FADE_OUT);
      x = Math.round(anchorX - width * align);
      y = Math.round((down ? anchorY + stack : anchorY - stack - height) - rise * eased);
    }
  }
}
