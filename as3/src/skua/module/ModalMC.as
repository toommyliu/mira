package skua.module {
import flash.display.MovieClip;

import skua.Main;

public class ModalMC extends MovieClip {
    public function init(params:Object):void {
        Main.instance.game.MsgBox.notify(params.strBody);
    }
}
}
