package mira.module {
	import flash.display.MovieClip;
	import flash.utils.Dictionary;
	import flash.utils.getQualifiedClassName;

	public class AnimationLimiter extends Module {
		private static const CATEGORY_COSMETIC:String = "cosmetic";
		private static const CATEGORY_MONSTER:String = "monster";
		private static const CATEGORY_WEAPON:String = "weapon";

		private var preferencesSnapshot:Object = null;
		private var stoppedClips:Dictionary = new Dictionary(true);

		public function AnimationLimiter() {
			super("AnimationLimiter");
		}

		override public function onToggle(game:*):void {
			if (enabled) {
				snapshotPreferences(game);
				applyReducedPreferences(game);
				stoppedClips = new Dictionary(true);
				reconcileAnimations(game);
			}
			else {
				restorePreferences(game);
				restoreStoppedClips();
				preferencesSnapshot = null;
				stoppedClips = new Dictionary(true);
			}
		}

		override public function onFrame(game:*):void {
			applyReducedPreferences(game);
			reconcileAnimations(game);
		}

		private function reconcileAnimations(game:*):void {
			for (var mid:* in game.world.monsters) {
				stopMonster(game.world.monsters[mid], game.world.strFrame);
			}

			for (var aid:* in game.world.avatars) {
				stopAvatar(game.world.avatars[aid]);
			}
		}

		private function stopMonster(monster:*, currentFrame:String):void {
			if (!monster || !monster.dataLeaf || monster.dataLeaf.strFrame != currentFrame || !monster.pMC) {
				return;
			}

			var mc:MovieClip = null;
			try {
				mc = monster.pMC.getChildAt(1) as MovieClip;
			}
			catch (e:Error) {
				return;
			}

			if (!mc) {
				return;
			}

			stopOnce(mc, CATEGORY_MONSTER);
			if (monster.dataLeaf.intState > 0 && mc.currentLabel != "Idle") {
				try {
					mc.gotoAndStop("Idle");
				}
				catch (idleError:Error) {
				}
			}
		}

		private function stopAvatar(avatar:*):void {
			if (!avatar || !avatar.objData || !avatar.pMC || !avatar.pMC.mcChar) {
				return;
			}

			try {
				stopClip(avatar.pMC.mcChar.weapon.mcWeapon, CATEGORY_WEAPON);
				stopChildren(avatar.pMC.mcChar.weapon.mcWeapon, CATEGORY_WEAPON);
				stopChildren(avatar.pMC.mcChar.weapon, CATEGORY_WEAPON);
			}
			catch (weaponError:Error) {
			}

			try {
				stopChildren(avatar.pMC.mcChar.weaponOff, CATEGORY_WEAPON);
			}
			catch (offhandError:Error) {
			}

			try {
				stopClip(avatar.pMC.mcChar.head, CATEGORY_COSMETIC);
				stopChildren(avatar.pMC.mcChar.head.helm, CATEGORY_COSMETIC);
			}
			catch (headError:Error) {
			}

			try {
				stopClip(avatar.pMC.mcChar.cape, CATEGORY_COSMETIC);
				stopChildren(avatar.pMC.mcChar.cape, CATEGORY_COSMETIC);
			}
			catch (capeError:Error) {
			}

			try {
				stopClip(avatar.petMC, CATEGORY_COSMETIC);
				stopChildren(avatar.petMC, CATEGORY_COSMETIC);
			}
			catch (petError:Error) {
			}

			if (hasGroundItem(avatar)) {
				try {
					stopClip(avatar.pMC.cShadow, CATEGORY_COSMETIC);
					stopChildren(avatar.pMC.cShadow, CATEGORY_COSMETIC);
				}
				catch (groundError:Error) {
				}
			}
		}

		private function hasGroundItem(avatar:*):Boolean {
			try {
				return Boolean(avatar.getItemByEquipSlot("mi"));
			}
			catch (e:Error) {
				return false;
			}

			return false;
		}

		private function stopClip(value:*, category:String):void {
			var clip:MovieClip = value as MovieClip;
			if (clip) {
				stopOnce(clip, category);
			}
		}

		private function stopChildren(value:*, category:String):void {
			var clip:MovieClip = value as MovieClip;
			if (!clip) {
				return;
			}

			for (var i:int = 0; i < clip.numChildren; i++) {
				stopClip(clip.getChildAt(i), category);
			}
		}

		private function stopOnce(clip:MovieClip, category:String):void {
			if (!clip || stoppedClips[clip]) {
				return;
			}

			if (getQualifiedClassName(clip).indexOf("Display") != -1) {
				return;
			}

			stoppedClips[clip] = category;
			try {
				clip.gotoAndStop(0);
			}
			catch (stopError:Error) {
			}

			for (var i:int = 0; i < clip.numChildren; i++) {
				var child:MovieClip = clip.getChildAt(i) as MovieClip;
				if (child) {
					stopOnce(child, category);
				}
			}
		}

		private function restoreStoppedClips():void {
			for (var key:* in stoppedClips) {
				var clip:MovieClip = key as MovieClip;
				var category:String = String(stoppedClips[key]);
				if (!clip || !shouldResumeCategory(category)) {
					continue;
				}

				try {
					clip.gotoAndPlay(0);
				}
				catch (playError:Error) {
				}
			}
		}

		private function shouldResumeCategory(category:String):Boolean {
			if (!preferencesSnapshot) {
				return true;
			}

			if (category == CATEGORY_MONSTER) {
				return preferencesSnapshot.bDisMonAnim != true;
			}

			if (category == CATEGORY_WEAPON) {
				return preferencesSnapshot.bDisWepAnim != true;
			}

			return true;
		}

		private function snapshotPreferences(game:*):void {
			if (preferencesSnapshot) {
				return;
			}

			var data:Object = game.litePreference.data;
			var options:Object = ensureOptions(data);
			preferencesSnapshot = {
				bDisAuraAnim: data.bDisAuraAnim,
				bDisMonAnim: data.bDisMonAnim,
				bDisSkillAnim: data.bDisSkillAnim,
				bDisWepAnim: data.bDisWepAnim,
				animSelf: options["animSelf"],
				auraAnimSelf: options["auraAnimSelf"],
				wepSelf: options["wepSelf"]
			};
		}

		private function applyReducedPreferences(game:*):void {
			var data:Object = game.litePreference.data;
			var options:Object = ensureOptions(data);
			data.bDisAuraAnim = true;
			data.bDisMonAnim = true;
			data.bDisSkillAnim = true;
			data.bDisWepAnim = true;
			options["animSelf"] = false;
			options["auraAnimSelf"] = false;
			options["wepSelf"] = false;
		}

		private function restorePreferences(game:*):void {
			if (!preferencesSnapshot) {
				return;
			}

			var data:Object = game.litePreference.data;
			var options:Object = ensureOptions(data);
			data.bDisAuraAnim = preferencesSnapshot.bDisAuraAnim;
			data.bDisMonAnim = preferencesSnapshot.bDisMonAnim;
			data.bDisSkillAnim = preferencesSnapshot.bDisSkillAnim;
			data.bDisWepAnim = preferencesSnapshot.bDisWepAnim;
			options["animSelf"] = preferencesSnapshot.animSelf;
			options["auraAnimSelf"] = preferencesSnapshot.auraAnimSelf;
			options["wepSelf"] = preferencesSnapshot.wepSelf;
		}

		private function ensureOptions(data:Object):Object {
			if (!data.dOptions) {
				data.dOptions = {};
			}

			return data.dOptions;
		}
	}
}
