package lyra.module {
  import flash.events.Event;
  import lyra.Main;
  import lyra.util.SFSEvent;

  public class CustomName extends Module {

    public function CustomName() {
      super("CustomName");
    }

    public static var instance:CustomName = new CustomName();

    public var customName:* = null;

    public var customGuild:* = null;

    public function attach(game:*):void {
      game.stage.addEventListener(Event.RENDER, onRender);
      game.sfc.addEventListener(SFSEvent.onExtensionResponse, invalidate);
    }

    public function resetName():void {
      customName = null;
      var game:* = Main.Game;
      var avatar:* = shownAvatar(game);
      if (avatar == null) {
        return;
      }

      avatar.pMC.updateName();
      game.world.updatePortrait(avatar);
    }

    public function resetGuild():void {
      customGuild = null;
      var avatar:* = shownAvatar(Main.Game);
      if (avatar == null) {
        return;
      }

      avatar.pMC.pname.tg.text = avatar.objData.guild == null
        ? ""
        : "< " + String(avatar.objData.guild.Name) + " >";
    }

    override public function onToggle(game:*):void {
      var avatar:* = shownAvatar(game);
      if (avatar == null) {
        return;
      }

      if (customName !== null) {
        avatar.pMC.pname.ti.text = customName;
        game.ui.mcPortrait.strName.text = String(customName).toUpperCase();
        if (avatar.target == avatar) {
          game.ui.mcPortraitTarget.strName.text = String(customName).toUpperCase();
        }
      }

      if (customGuild !== null) {
        avatar.pMC.pname.tg.text = customGuild == ""
          ? ""
          : "< " + String(customGuild) + " >";
      }
    }

    override public function onFrame(game:*):void {
      game.stage.invalidate();
    }

    private function invalidate(event:*):void {
      Main.Game.stage.invalidate();
    }

    private function onRender(event:Event):void {
      if (enabled) {
        onToggle(Main.Game);
      }
    }

    private static function shownAvatar(game:*):* {
      var avatar:* = game && game.world ? game.world.myAvatar : null;
      if (avatar == null || avatar.objData == null || avatar.pMC == null || game.ui == null) {
        return null;
      }
      return avatar;
    }
  }

}
