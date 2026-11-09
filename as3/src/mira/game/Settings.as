package mira.game {
  import mira.Main;
  import mira.module.CustomName;
  import mira.module.Modules;

  [BridgeNamespace("settings")]
  public class Settings {
    private static var worldVisible:Boolean = true;

    private static function sanitizeText(value:String):String {
      return value ? value : "";
    }

    private static function setModuleEnabled(moduleName:String, enabled:Boolean):void {
      if (enabled) {
        Modules.enable(moduleName);
      }
      else {
        Modules.disable(moduleName);
      }
    }

    private static function applyWorldVisibility():void {
      Main.Game.world.visible = worldVisible;
    }

    [BridgeExport]
    public static function infiniteRange():void {
      var world:Object = Main.Game.world;
      for (var i:int = 0; i <= 5; i++) {
        world.actions.active[i].range = 20000;
      }
    }

    [BridgeExport]
    public static function provokeCell():void {
      Main.Game.world.aggroAllMon();
    }

    [BridgeExport]
    public static function enemyMagnet():void {
      var world:Object = Main.Game.world;
      if (world.myAvatar && world.myAvatar.target != null && world.myAvatar.target.npcType == "monster") {
        world.myAvatar.target.pMC.x = world.myAvatar.pMC.x;
        world.myAvatar.target.pMC.y = world.myAvatar.pMC.y;
      }
    }

    [BridgeExport]
    public static function skipCutscenes():void {
      Main.Game.clearExternamSWF();
      applyWorldVisibility();
    }

    [BridgeExport]
    public static function setCustomName(name:String):void {
      CustomName.instance.customName = sanitizeText(name);
    }

    [BridgeExport]
    public static function setCustomGuild(name:String):void {
      CustomName.instance.customGuild = sanitizeText(name);
    }

    [BridgeExport]
    public static function resetCustomName():void {
      CustomName.instance.resetName();
    }

    [BridgeExport]
    public static function resetCustomGuild():void {
      CustomName.instance.resetGuild();
    }

    [BridgeExport]
    public static function setWalkSpeed(speed:int):void {
      Main.Game.world.WALKSPEED = speed;
    }

    [BridgeExport]
    public static function setFrameRate(fps:int):void {
      var game:* = Main.Game;
      var preference:* = game && game.litePreference ? game.litePreference.data : null;
      if (preference) {
        // The client's Max FPS option saves only 30, 48, 60, or 0 for the rate the stage had
        // when the client loaded, which is the loader's own frame rate. Game.defaultFPS is private.
        var own:int = Main.getInstance().loaderInfo.frameRate;
        var option:int = fps == 30 || fps == 48 || fps == 60 ? fps : (fps == own ? 0 : -1);
        if (option >= 0 && preference.nMaxFPS != option) {
          preference.nMaxFPS = option;
          game.litePreference.flush();
        }
      }
      Main.getInstance().getStage().frameRate = fps;
    }

    [BridgeExport]
    public static function setAdsVisible(visible:Boolean):void {
      setModuleEnabled("HideAds", !visible);
    }

    [BridgeExport]
    public static function setCollisionsEnabled(enabled:Boolean):void {
      setModuleEnabled("DisableCollisions", !enabled);
    }

    [BridgeExport]
    public static function setAnimationsEnabled(enabled:Boolean):void {
      setModuleEnabled("AnimationLimiter", !enabled);
    }

    [BridgeExport]
    public static function setOtherPlayersVisible(visible:Boolean):void {
      setModuleEnabled("HidePlayers", !visible);
    }

    [BridgeExport]
    public static function setWorldVisible(visible:Boolean):void {
      worldVisible = visible;
      applyWorldVisibility();
    }
  }
}
