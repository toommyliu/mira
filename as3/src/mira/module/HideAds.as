package mira.module {
	import mira.Main;

	// TODO: remove debug tracing once this lands.
	public class HideAds extends Module {
		private var newsInfo:*;
		private var carousel:*;
		private var carouselWouldOpen:Boolean = false;
		private var dead:Boolean = false;

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
				if (!info.firstNews) {
					// The game menu opens its news carousel on its own only while firstNews is unset.
					info.firstNews = true;
					carouselWouldOpen = hoursSinceLogin(game) > 1;
				}
			}
			var built:* = game.mcGameMenu ? game.mcGameMenu.mcImages : null;
			if (!built || built == carousel) {
				return;
			}
			carousel = built;
			if (built.visible) {
				built.visible = false;
				carouselWouldOpen = true;
			}
			report("newsCarousel", carouselWouldOpen ? "mira" : "game");
			carouselWouldOpen = false;
		}

		private function hideDeathAd(game:*):void {
			var avatar:* = game.world ? game.world.myAvatar : null;
			var died:Boolean = avatar && avatar.dataLeaf && avatar.dataLeaf.intState == 0;
			if (died && !dead && game.userPreference && !game.userPreference.data.bDeathAd) {
				report("deathAd", "game");
			}
			dead = died;
			var content:* = game.ui && game.ui.mcRes ? game.ui.mcRes.mcContent : null;
			if (content && content.mcAd && content.mcAd.visible) {
				content.mcAd.visible = false;
				report("deathAd", "mira");
			}
		}

		private function report(ad:String, by:String):void {
			Main.getInstance().emitAdHidden(ad, by);
		}

		private function hoursSinceLogin(game:*):Number {
			try {
				return game.getLogin().intHours;
			}
			catch (error:Error) {
			}
			return 0;
		}
	}
}
