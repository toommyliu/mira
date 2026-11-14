package mira.module {
	public class HidePlayers extends Module {
		public function HidePlayers() {
			super("HidePlayers");
		}

		override public function onToggle(game:*):void {
			applyNativePreference(game);
			reconcilePlayers(game);
		}

		override public function onFrame(game:*):void {
			applyNativePreference(game);
			reconcilePlayers(game);
		}

		private function applyNativePreference(game:*):void {
			if (game.litePreference.data.bHidePlayers != enabled) {
				game.litePreference.data.bHidePlayers = enabled;
			}
		}

		private function reconcilePlayers(game:*):void {
			for (var id:* in game.world.avatars) {
				var player:* = game.world.avatars[id];
				reconcilePlayer(player);
			}
		}

		private function reconcilePlayer(player:*):void {
			if (!player || player.isMyAvatar || !player.pMC) {
				return;
			}

			var visible:Boolean = !enabled;
			setVisible(player.pMC.mcChar, visible);
			setVisible(player.pMC.pname, visible);
			setVisible(player.pMC.shadow, visible);
			setVisible(player.petMC, visible && player.pMC.visible);
			setVisible(player.pMC.cShadow, visible);
			setAlpha(player.pMC.shadow, enabled ? 0 : 1);
		}

		private function setVisible(target:*, visible:Boolean):void {
			if (target && target.visible != visible) {
				target.visible = visible;
			}
		}

		private function setAlpha(target:*, alpha:Number):void {
			if (target && target.alpha != alpha) {
				target.alpha = alpha;
			}
		}
	}
}
