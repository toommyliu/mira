package mira.ui {
  import flash.display.Stage;
  import flash.events.KeyboardEvent;
  import flash.text.TextField;
  import flash.ui.Keyboard;

  import mira.Main;

  public class ChatHistory {
    private static const LIMIT:int = 50;

    private static var entries:Array = [];
    private static var cursor:int = 0;
    private static var draft:String = "";

    public static function init(stage:Stage):void {
      stage.addEventListener(KeyboardEvent.KEY_DOWN, onKeyDown, true);
    }

    public static function clear():void {
      entries = [];
      cursor = 0;
      draft = "";
    }

    private static function onKeyDown(event:KeyboardEvent):void {
      var field:TextField = event.target as TextField;
      if (field == null || !isChatInput(field)) {
        return;
      }

      switch (event.keyCode) {
        case Keyboard.ENTER:
          record(field.text);
          break;
        case Keyboard.UP:
          step(field, -1);
          break;
        case Keyboard.DOWN:
          step(field, 1);
          break;
      }
    }

    private static function isChatInput(field:TextField):Boolean {
      var chat:* = Main.resolvePath(Main.Game, ["ui", "mcInterface"], true);
      return chat != null && (field == chat.te || field == chat.ncText);
    }

    private static function record(text:String):void {
      cursor = entries.length;
      draft = "";
      if (text.replace(/^\s+|\s+$/g, "").length == 0 || isCredential(text) || text == entries[entries.length - 1]) {
        return;
      }

      entries.push(text);
      if (entries.length > LIMIT) {
        entries.shift();
      }
      cursor = entries.length;
    }

    private static function step(field:TextField, direction:int):void {
      if (cursor < entries.length && field.text != entries[cursor]) {
        cursor = entries.length;
      }
      if (cursor == entries.length) {
        draft = field.text;
      }

      var target:int = cursor + direction;
      if (target < 0 || target > entries.length) {
        return;
      }

      cursor = target;
      field.text = cursor == entries.length ? draft : entries[cursor];
      field.setSelection(field.length, field.length);
    }

    // Chat.submitMsg refuses to send a message containing the login password or account email; keep those out of history too.
    private static function isCredential(text:String):Boolean {
      var game:* = Main.Game;
      var lower:String = text.toLowerCase();
      return contains(lower, Main.resolvePath(game.constructor, ["loginInfo", "strPassword"], true))
        || contains(lower, Main.resolvePath(game, ["world", "myAvatar", "objData", "strEmail"], true));
    }

    private static function contains(lower:String, secret:*):Boolean {
      return secret is String && secret.length > 0 && lower.indexOf(secret.toLowerCase()) > -1;
    }
  }
}
