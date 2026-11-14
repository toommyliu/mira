package mira.module {
	public class HideAds extends Module {
		private var newsInfo:*;
		private var carousel:*;

		public function HideAds() {
			super("HideAds");
		}

		override public function onFrame(game:*):void {
			if (!game) {
				return;
			}
			keepCarouselClosed(game);
			hideDeathAd(game);
		}

		private function keepCarouselClosed(game:*):void {
			var info:* = game.world ? game.world.objInfo : null;
			if (info && info != newsInfo) {
				newsInfo = info;
				// The game menu opens its news carousel on its own only while firstNews is unset.
				info.firstNews = true;
			}
			var built:* = game.mcGameMenu ? game.mcGameMenu.mcImages : null;
			if (!built || built == carousel) {
				return;
			}
			carousel = built;
			built.visible = false;
		}

		private function hideDeathAd(game:*):void {
			var content:* = game.ui && game.ui.mcRes ? game.ui.mcRes.mcContent : null;
			if (content && content.mcAd && content.mcAd.visible) {
				content.mcAd.visible = false;
			}
		}
	}
}
