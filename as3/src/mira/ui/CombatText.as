package mira.ui {
  import flash.display.DisplayObject;
  import flash.display.Sprite;
  import flash.events.Event;
  import flash.system.ApplicationDomain;
  import flash.utils.Dictionary;
  import flash.utils.getTimer;

  import mira.Main;
  import mira.util.SFSEvent;

  public class CombatText {
    private static const AVOIDS:Object = {miss: true, dodge: true, parry: true, block: true};
    private static const MERGE_MS:int = 250;
    private static const DAMAGE_LIFE:int = 900;
    private static const DAMAGE_RISE:Number = 18;
    private static const OVER_TIME_OPACITY:Number = 0.75;
    private static const AURA_LIFE:int = 1600;
    private static const FADE_LIFE:int = 1100;
    private static const FADE_OPACITY:Number = 0.7;
    private static const AURA_SIZE:int = 12;

    private static const DEALT:uint = 0xFFFFFF;
    private static const DEALT_CRIT:uint = 0xFF9944;
    private static const TAKEN:uint = 0xF07171;
    private static const TAKEN_CRIT:uint = 0xFF4545;
    private static const HEALED:uint = 0x00FFAA;
    private static const MUTED:uint = 0xA1A5B0;
    private static const GAINED:uint = 0xCFEA5B;
    private static const FADED:uint = 0x999999;

    private static var layer:Sprite = new Sprite();
    private static var damage:TextColumn = new TextColumn(layer, 0, -4, 0.5, false, 6);
    private static var auras:TextColumn = new TextColumn(layer, 36, 24, 0, true, 5);
    private static var mine:Object = {};
    private static var theirs:Object = {};
    private static var nativeClips:Dictionary;
    private static var listening:Boolean;
    private static var world:*;
    private static var sfc:*;
    private static var lastFrame:int;

    public static function setDamageEnabled(enabled:Boolean):void {
      damage.enabled = enabled;
      if (!enabled) {
        damage.clear();
      }
      listen(damage.enabled || auras.enabled);
    }

    public static function setAurasEnabled(enabled:Boolean):void {
      auras.enabled = enabled;
      mine.leaf = theirs.leaf = null;
      if (!enabled) {
        auras.clear();
      }
      listen(damage.enabled || auras.enabled);
    }

    private static function listen(active:Boolean):void {
      if (active == listening) {
        return;
      }

      listening = active;
      var main:Main = Main.getInstance();
      if (active) {
        layer.mouseEnabled = false;
        layer.mouseChildren = false;
        nativeClips ||= findNativeClips(main.getGameDomain());
        lastFrame = getTimer();
        main.getStage().addEventListener(Event.ENTER_FRAME, onFrame);
        follow(Main.Game);
        return;
      }

      main.getStage().removeEventListener(Event.ENTER_FRAME, onFrame);
      follow(null);
    }

    private static function findNativeClips(domain:ApplicationDomain):Dictionary {
      var owners:Object = {
        hitDisplay: damage,
        critDisplay: damage,
        avoidDisplay: damage,
        dotDisplay: damage,
        auraDisplay: auras
      };
      var classes:Dictionary = new Dictionary();
      for (var name:String in owners) {
        if (domain.hasDefinition(name)) {
          classes[domain.getDefinition(name)] = owners[name];
        }
      }
      return classes;
    }

    private static function follow(game:*):void {
      var nextSfc:* = game == null ? null : game.sfc;
      if (nextSfc !== sfc) {
        if (sfc != null) {
          sfc.removeEventListener(SFSEvent.onExtensionResponse, onPacket);
        }
        sfc = nextSfc;
        if (sfc != null) {
          sfc.addEventListener(SFSEvent.onExtensionResponse, onPacket);
        }
      }

      var nextWorld:* = game == null ? null : game.world;
      if (nextWorld !== world) {
        if (world != null) {
          world.removeEventListener(Event.ADDED, hideNative);
        }
        world = nextWorld;
        if (world != null) {
          world.addEventListener(Event.ADDED, hideNative);
        }
      }

      if (world == null) {
        if (layer.parent != null) {
          layer.parent.removeChild(layer);
        }
        return;
      }
      if (layer.parent !== world || world.getChildIndex(layer) != world.numChildren - 1) {
        world.addChild(layer);
      }
    }

    private static function hideNative(event:Event):void {
      var clip:DisplayObject = event.target as DisplayObject;
      var owner:TextColumn = clip == null ? null : nativeClips[Object(clip).constructor];
      if (owner != null && owner.enabled) {
        clip.visible = false;
      }
    }

    private static function onFrame(event:Event):void {
      follow(Main.Game);
      var now:int = getTimer();
      var dt:int = now - lastFrame;
      lastFrame = now;
      if (auras.enabled) {
        watchAuras(now);
      }
      damage.step(now, dt);
      auras.step(now, dt);
    }

    private static function onPacket(event:*):void {
      if (event.params.type != "json") {
        return;
      }

      var data:* = event.params.dataObj;
      try {
        switch (data.cmd) {
          case "sar":
            showSar(data);
            break;
          case "sars":
            showSars(data);
            break;
          case "ct":
            for each (var sar:* in data.sara || []) {
              showSar(sar);
            }
            for each (var sars:* in data.sarsa || []) {
              showSars(sars);
            }
            showImmunes(data.a);
            break;
          case "cb":
            showImmunes(data.a);
            break;
          case "showAuraResult":
            showDamage(data.cInf, data.tInf, data.hp, "hit", true);
            break;
          case "aura*":
            showImmune(data);
            break;
        }
      }
      catch (error:Error) {
        Main.getInstance().getExternal().debug("Combat text skipped a " + data.cmd + " packet: " + error.message);
      }
    }

    private static function showSar(sar:*):void {
      if (sar.iRes != 1) {
        return;
      }

      var result:* = sar.actionResult;
      var overTime:Boolean = result.typ == "d";
      showDamage(result.cInf, result.tInf, result.hp, overTime ? "hit" : result.type, overTime);
    }

    private static function showSars(sars:*):void {
      if (sars.iRes != 1) {
        return;
      }

      for each (var hit:* in sars.a) {
        showDamage(sars.cInf, hit.tInf, hit.hp, hit.type, false);
      }
    }

    private static function showDamage(source:String, targetKey:String, hp:int, type:String, overTime:Boolean):void {
      if (!damage.enabled) {
        return;
      }

      var target:* = entityOf(targetKey);
      if (target == null || (source != "p:" + sfc.myUserId && !isShown(target))) {
        return;
      }

      var now:int = getTimer();
      var line:FloatingText;
      if (AVOIDS[type]) {
        line = damage.add(target);
        line.start(null, type, MUTED, 1, DAMAGE_LIFE, DAMAGE_RISE, false, now);
        line.write(type, null, 12);
        damage.restack(target);
        return;
      }

      var crit:Boolean = type == "crit";
      if (!crit && type != "hit") {
        return;
      }

      var me:* = world.myAvatar;
      var healed:Boolean = hp < 0;
      var amount:int = Math.abs(hp);
      var max:Number = target.dataLeaf == null ? 0 : target.dataLeaf.intHPMax;
      var color:uint = healed ? HEALED : target === me ? (crit ? TAKEN_CRIT : TAKEN) : (crit ? DEALT_CRIT : DEALT);
      var key:String = color + (crit ? "!" : "") + (overTime ? "~" : "");
      var prefix:String = healed ? "+" : "";

      line = damage.newest(target);
      if (line != null && line.key == key && now - line.born < MERGE_MS) {
        line.amount += amount;
        line.count++;
        line.write(prefix + formatAmount(line.amount), "×" + line.count, sizeFor(line.amount, max, crit, overTime));
      }
      else {
        line = damage.add(target);
        line.start(key, type, color, overTime ? OVER_TIME_OPACITY : 1, DAMAGE_LIFE, DAMAGE_RISE, crit, now);
        line.amount = amount;
        line.write(prefix + formatAmount(amount), null, sizeFor(amount, max, crit, overTime));
      }
      damage.restack(target);
    }

    private static function showImmunes(changes:Array):void {
      if (changes == null) {
        return;
      }
      for each (var change:* in changes) {
        if (change.cmd == "aura*") {
          showImmune(change);
        }
      }
    }

    private static function showImmune(change:*):void {
      var target:* = entityOf(change.tInf);
      if (!auras.enabled || target == null || !isShown(target)) {
        return;
      }

      var now:int = getTimer();
      var line:FloatingText = auras.find(target, "immune");
      if (line != null) {
        line.touch(now);
        return;
      }
      line = auras.add(target);
      line.start("immune", "immune", MUTED, 1, FADE_LIFE, 0, false, now);
      line.write("immune", null, AURA_SIZE);
      auras.restack(target);
    }

    private static function watchAuras(now:int):void {
      var me:* = world == null ? null : world.myAvatar;
      watch(mine, me, now);
      watch(theirs, me == null || me.target === me ? null : me.target, now);
    }

    private static function watch(seen:Object, target:*, now:int):void {
      var leaf:* = target == null || target.pMC == null ? null : target.dataLeaf;
      if (leaf == null) {
        seen.leaf = null;
        return;
      }

      var version:int = int(leaf.hudAurasV);
      if (seen.leaf === leaf && seen.version == version) {
        return;
      }

      var current:Object = {};
      for (var name:String in leaf.hudAuras) {
        current[name] = int(leaf.hudAuras[name].n);
      }
      var previous:Object = seen.leaf === leaf ? seen.stacks : null;
      seen.leaf = leaf;
      seen.version = version;
      seen.stacks = current;
      if (previous == null) {
        return;
      }

      for (name in current) {
        if (previous[name] !== current[name]) {
          gained(target, name, current[name], now);
        }
      }
      for (name in previous) {
        if (!(name in current)) {
          faded(target, name, now);
        }
      }
      auras.restack(target);
    }

    private static function gained(target:*, name:String, count:int, now:int):void {
      var suffix:String = count > 1 ? "\u00d7" + count : null;
      var line:FloatingText = auras.find(target, name);
      if (line != null && line.kind == "gained") {
        line.write(name, suffix, AURA_SIZE);
        line.touch(now);
        return;
      }
      if (line != null) {
        auras.remove(target, line);
      }

      line = auras.add(target);
      line.start(name, "gained", GAINED, 1, AURA_LIFE, 0, false, now);
      line.write(name, suffix, AURA_SIZE);
    }

    private static function faded(target:*, name:String, now:int):void {
      var line:FloatingText = auras.find(target, name);
      if (line != null) {
        if (line.kind == "faded") {
          line.touch(now);
        }
        return;
      }

      line = auras.add(target);
      line.start(name, "faded", FADED, FADE_OPACITY, FADE_LIFE, 0, false, now);
      line.write(name, "fades", AURA_SIZE);
    }

    private static function isShown(target:*):Boolean {
      var me:* = world.myAvatar;
      return me != null && (target === me || target === me.target);
    }

    private static function entityOf(key:String):* {
      if (world == null || key == null) {
        return null;
      }

      var parts:Array = key.split(":");
      var id:int = int(parts[1]);
      var entity:* = null;
      switch (parts[0]) {
        case "m":
          entity = world.getMonster(id);
          break;
        case "p":
          entity = world.avatars[id];
          break;
      }
      return entity == null || entity.pMC == null ? null : entity;
    }

    private static function sizeFor(amount:int, max:Number, crit:Boolean, overTime:Boolean):int {
      var share:Number = max > 0 ? Math.min(1, amount / max) : 0;
      var size:Number = 13 + 9 * Math.log(1 + share * 100) / Math.log(101);
      return Math.round(size + (crit ? 2 : 0) - (overTime ? 2 : 0));
    }

    private static function formatAmount(amount:int):String {
      if (amount >= 1000000) {
        return compact(amount / 1000000) + "m";
      }
      if (amount >= 10000) {
        return compact(amount / 1000) + "k";
      }
      if (amount >= 1000) {
        return int(amount / 1000) + "," + String(1000 + amount % 1000).substr(1);
      }
      return String(amount);
    }

    private static function compact(value:Number):String {
      if (value >= 100) {
        return String(int(value));
      }
      var rounded:String = value.toFixed(1);
      return rounded.substr(-2) == ".0" ? rounded.substr(0, rounded.length - 2) : rounded;
    }
  }
}
