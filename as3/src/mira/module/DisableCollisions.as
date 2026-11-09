package mira.module {
	public class DisableCollisions extends Module {
		private var _empty:Array = [];
		private var _emptyR:Array = [];
		private var _old:*;
		private var _oldR:*;

		public function DisableCollisions() {
			super("DisableCollisions");
		}

		override public function onToggle(game:*):void {
			var world:* = game.world;
			if (enabled) {
				disableWorldCollisions(world);
			}
			else {
				if (world.arrSolid === _empty) {
					world.arrSolid = _old;
				}
				if (world.arrSolidR === _emptyR) {
					world.arrSolidR = _oldR;
				}
				_old = null;
				_oldR = null;
			}
		}

		override public function onFrame(game:*):void {
			disableWorldCollisions(game.world);
		}

		private function disableWorldCollisions(world:*):void {
			// Map loading may replace or repopulate these arrays while enabled.
			// Preserve the latest collision data so disabling restores the current map.
			if (world.arrSolid !== _empty) {
				_old = world.arrSolid;
				world.arrSolid = _empty;
			}
			else if (_empty.length > 0) {
				_old = _empty;
				_empty = [];
				world.arrSolid = _empty;
			}
			if (world.arrSolidR !== _emptyR) {
				_oldR = world.arrSolidR;
				world.arrSolidR = _emptyR;
			}
			else if (_emptyR.length > 0) {
				_oldR = _emptyR;
				_emptyR = [];
				world.arrSolidR = _emptyR;
			}
		}
	}

}
