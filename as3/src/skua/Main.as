package skua {
import mira.Externalizer;
import mira.game.Skua;

public class Main {
    public static const instance:Main = new Main();

    public var customBGLagKiller:* = null;

    public function get game():* {
        return Skua.game;
    }

    public function get external():Externalizer {
        return Skua.external;
    }

    public static function getGameObjectS(path:String):String {
        return Skua.gameObjectS(path);
    }
}
}
