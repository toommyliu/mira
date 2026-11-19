package skua.api {

import skua.Main;

public class Combat {

    public function Combat() {
        super();
    }

    public static function magnetize():void {
        var target:* = Main.instance.game.world.myAvatar.target;
        if (target) {
            target.pMC.x = Main.instance.game.world.myAvatar.pMC.x;
            target.pMC.y = Main.instance.game.world.myAvatar.pMC.y;
        }
    }

    public static function infiniteRange():void {
        var active:Array = Main.instance.game.world.actions.active;
        for (var i:int = 0; i < 6; i++) {
            active[i].range = 20000;
        }
    }
}
}
