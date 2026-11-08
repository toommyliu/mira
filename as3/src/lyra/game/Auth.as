package lyra.game
{
  import lyra.Main;
  import flash.events.Event;
  import flash.events.IOErrorEvent;
  import flash.events.MouseEvent;
  import flash.events.ProgressEvent;

  [BridgeNamespace("auth")]
  public class Auth
  {

    private static function removeLoaderListener(loader:*, eventType:String, handler:*):void
    {
      if (handler is Function)
      {
        loader.contentLoaderInfo.removeEventListener(eventType, handler as Function);
      }
    }

    private static function clearCharacterSelect(game:Object):void
    {
      // The client loads character select outside its timeline, so cancel the loader
      // before it can attach the screen again during an automated transition.
      var loader:* = Main.resolvePath(game, ["csLoader"], true);
      if (loader != null)
      {
        removeLoaderListener(
          loader,
          Event.COMPLETE,
          Main.resolvePath(game, ["onCSComplete"], true)
        );
        removeLoaderListener(
          loader,
          ProgressEvent.PROGRESS,
          Main.resolvePath(game, ["onCSProgress"], true)
        );
        removeLoaderListener(
          loader,
          IOErrorEvent.IO_ERROR,
          Main.resolvePath(game, ["onCSError"], true)
        );

        try
        {
          loader.close();
        }
        catch (closeError:Error)
        {
        }

        try
        {
          loader.unloadAndStop(true);
        }
        catch (unloadError:Error)
        {
        }

        game.csLoader = null;
      }

      var characterSelect:* = Main.resolvePath(game, ["mcCharSelect"], true);
      if (characterSelect != null && characterSelect.parent != null)
      {
        characterSelect.parent.removeChild(characterSelect);
      }
      game.mcCharSelect = null;
    }

    [BridgeExport]
    public static function isLoggedIn():Boolean
    {
      var game:Object = Main.Game;
      var sfc:Object = game ? game.sfc : null;
      return Boolean(sfc) && Boolean(sfc.isConnected);
    }

    [BridgeExport]
    public static function isTemporarilyKicked():Boolean
    {
      var game:Object = Main.Game;
      var mcLogin:* = game.mcLogin;
      return mcLogin !== null && mcLogin.btnLogin !== null &&
        !mcLogin.btnLogin.visible;
    }

    [BridgeExport]
    public static function login(username:String, password:String):void
    {
      var game:Object = Main.Game;
      clearCharacterSelect(game);
      if (game.mcLogin == null || game.mcLogin.btnLogin == null)
      {
        game.removeAllChildren();
        game.gotoAndPlay("Login");
      }
      game.login(username, password);
    }

    [BridgeExport]
    public static function logout():void
    {
      var game:Object = Main.Game;
      if (game.sfc != null && game.sfc.isConnected)
      {
        game.sfc.disconnect();
      }
      clearCharacterSelect(game);
      game.removeAllChildren();
      game.gotoAndPlay("Login");
    }

    [BridgeExport]
    [BridgeTsReturnType("unknown[] | null")]
    public static function getServers():Array
    {
      var game:Object = Main.Game;
      if (game.serialCmd != null && game.serialCmd.servers is Array)
      {
        return game.serialCmd.servers;
      }

      return null;
    }

    private static function connectResult(status:String, message:String, serverName:String = null, reason:String = null):Object
    {
      var result:Object = {
        ok: status == "selected",
        status: status,
        message: message
      };

      if (reason != null)
      {
        result.reason = reason;
      }

      if (serverName != null)
      {
        result.serverName = serverName;
      }

      return result;
    }

    private static function getServerListSource(currentGame:*):*
    {
      return Main.resolvePath(currentGame, ["mcLogin", "sl", "iList"], true);
    }

    private static function getLoginInfo(currentGame:*):Object
    {
      var login:Object = Main.resolvePath(currentGame, ["objLogin"], true);
      if (login != null)
      {
        return login;
      }

      try
      {
        var gameClass:Class = Class(Main.getInstance().getGameDomain().getDefinition("Game"));
        login = gameClass["objLogin"];
      }
      catch (e:Error)
      {
        login = null;
      }

      return login;
    }

    private static function getServerData(row:*):Object
    {
      return Main.resolvePath(row, ["obj"], true);
    }

    private static function getServerName(row:*):String
    {
      var data:Object = getServerData(row);
      if (data != null && data.sName is String)
      {
        return data.sName;
      }

      var label:* = Main.resolvePath(row, ["tName", "ti", "text"], true);
      return label is String ? label : "";
    }

    private static function findServerRow(source:*, normalizedServer:String):*
    {
      for (var i:int = 0; i < source.numChildren; i++)
      {
        var row:* = source.getChildAt(i);
        if (getServerName(row).toLowerCase() == normalizedServer)
        {
          return row;
        }
      }

      return null;
    }

    private static function serverSelectionFailure(selected:Object, login:Object, serverName:String):Object
    {
      if (selected.bOnline == 0)
      {
        return connectResult("blocked", "server is offline", serverName, "offline");
      }

      if (selected.iCount >= selected.iMax)
      {
        return connectResult("blocked", "server is full", serverName, "full");
      }

      if (login != null && selected.iChat > 0 && login.bCCOnly == 1)
      {
        return connectResult("blocked", "account is restricted to canned-chat servers", serverName, "chat-restricted");
      }

      if (login != null && selected.iChat > 0 && login.iAge < 13 && login.iUpgDays < 0)
      {
        return connectResult("blocked", "account is not authorized for chat-enabled servers", serverName, "underage-chat");
      }

      if (login != null && selected.bUpg == 1 && login.iUpgDays < 0)
      {
        return connectResult("blocked", "account is not authorized for member-only servers", serverName, "member-only");
      }

      // Intentional check.
      if (selected.iMax % 2 > 0)
      {
        return connectResult("blocked", "server requires the testing game client", serverName, "test-client-required");
      }

      if (login != null && selected.iLevel > 0 && login.iEmailStatus <= 2)
      {
        return connectResult("blocked", "server requires a confirmed email address", serverName, "email-unconfirmed");
      }

      return null;
    }

    [BridgeExport]
    [BridgeTsReturnType("FlashTypes.ConnectToSelectionResult | null")]
    public static function connectTo(server:String):Object
    {
      var result:Object;
      try
      {
        result = connectToUnsafe(server);
      }
      catch (e:Error)
      {
        result = connectResult("not-ready", "server selection failed");
      }

      return result;
    }

    private static function connectToUnsafe(server:String):Object
    {
      if (!server)
      {
        return connectResult("not-found", "server is required");
      }

      var currentGame:* = Main.Game;
      var source:* = getServerListSource(currentGame);
      if (source == null)
      {
        return connectResult("not-ready", "server selection is not ready");
      }

      var child:* = findServerRow(source, server.toLowerCase());
      if (child == null)
      {
        return connectResult("not-found", "server was not found");
      }

      var selected:Object = getServerData(child);
      if (selected == null)
      {
        return connectResult("not-found", "server was not found");
      }

      var serverName:String = getServerName(child);
      var failure:Object = serverSelectionFailure(selected, getLoginInfo(currentGame), serverName);
      if (failure != null)
      {
        return failure;
      }

      child.dispatchEvent(new MouseEvent(MouseEvent.CLICK));
      return connectResult("selected", "server selected", serverName);
    }
  }
}
