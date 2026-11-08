package lyra.game {
  import lyra.Main;

  [BridgeNamespace("combat")]
  public class Combat {
    private static const CONSUMABLE_SKILL_INDEX:int = 5;

    private static function isMonsterAttackable(monster:Object):Boolean {
      if (monster == null || monster.dataLeaf == null) {
        return false;
      }

      return monster.dataLeaf.intState > 0 && monster.dataLeaf.intHP > 0;
    }

    private static function getSkillCooldownRemainingValue(skill:*):int {
      var game:* = Main.Game;

      var actionCooldown:* = NaN;
      var now:* = new Date().getTime();
      var haste:* = 1 - Math.min(Math.max(game.world.myAvatar.dataLeaf.sta.$tha, -1), 0.5);
      if (skill.OldCD != null) {
        actionCooldown = Math.round(skill.OldCD * haste);
      }
      else {
        actionCooldown = Math.round(skill.cd * haste);
      }

      var globalCooldown:* = game.world.GCD - (now - game.world.GCDTS);
      if (globalCooldown < 0) {
        globalCooldown = 0;
      }

      var remaining:* = actionCooldown - (now - skill.ts);
      if (remaining < 0) {
        remaining = 0;
      }

      return Math.max(globalCooldown, remaining);
    }

    [BridgeExport]
    public static function hasTarget():Boolean {
      var game:Object = Main.Game;
      var target:Object = game.world.myAvatar.target;
      if (target != null && target.dataLeaf != null) {
        return target.dataLeaf.intHP > 0;
      }

      return false;
    }

    [BridgeExport]
    [BridgeTsReturnType("FlashTypes.TargetInfo | null")]
    public static function getTarget():Object {
      var game:Object = Main.Game;
      var target:Object = game.world.myAvatar.target;
      if (target != null) {
        var dataLeaf:Object = target.dataLeaf;
        var objData:Object = target.objData;

        if (!dataLeaf || !objData) {
          return null;
        }

        if (target.npcType == "monster") {
          return {
            type: "monster",
            hp: dataLeaf.intHP,
            maxHp: dataLeaf.intHPMax,
            state: dataLeaf.intState,
            cell: dataLeaf.strFrame,
            monsterId: dataLeaf.MonID,
            monMapId: dataLeaf.MonMapID,
            level: dataLeaf.iLvl,
            race: objData.sRace,
            name: objData.strMonName
          };
        }

        if (target.npcType == "player") {
          var player:Object = Players.row(dataLeaf);
          player.type = "player";
          return player;
        }
      }

      return null;
    }

    private static function tryUseSkill(game:Object, skill:Object):Boolean {
      if (!skill.isOK || skill.skillLock)
        return false;

      var previousActionId:* = skill.actID;
      var previousTimestamp:* = skill.ts;
      game.world.testAction(skill);
      return skill.actID !== previousActionId || skill.ts !== previousTimestamp;
    }

    [BridgeExport]
    [BridgeTsReturnType("FlashTypes.ConsumableSkillItem | null")]
    public static function getConsumableSkillItem():Object {
      var game:Object = Main.Game;
      if (!game.world.actions || !game.world.actions.active) {
        return null;
      }

      var skill:Object = game.world.actions.active[CONSUMABLE_SKILL_INDEX];
      if (!skill || skill.ref != "i1" || skill.sArg1 == null) {
        return null;
      }

      var itemId:Number = Number(skill.sArg1);
      if (isNaN(itemId) || itemId <= 0) {
        return null;
      }

      return {
        itemId: itemId,
        // sArg1 changes before seia finishes loading the usable-item action.
        ready:
          !game.world.lockdownPots &&
          skill.isOK &&
          !skill.skillLock &&
          !skill.lock &&
          getSkillCooldownRemainingValue(skill) <= 0
      };
    }

    [BridgeTsParamType("selector: FlashTypes.MonsterSelector | null")]
    [BridgeExport]
    public static function useSkill(index:String, selector:Object = null, force:Boolean = false):Boolean {
      var game:Object = Main.Game;
      var skill:Object = game.world.actions.active[parseInt(index)];
      if (!skill) {
        return false;
      }

      if (selector != null) {
        var monster:Object = World.getMonster(selector);
        if (!isMonsterAttackable(monster)) {
          return false;
        }

        game.world.setTarget(monster);
        if (game.world.myAvatar.target !== monster) {
          return false;
        }
      }

      if (force || skill.tgt == "s" || skill.tgt == "f") {
        return tryUseSkill(game, skill);
      }

      if (game.world.myAvatar.target == game.world.myAvatar) {
        game.world.myAvatar.target = null;
        return false;
      }

      if (game.world.myAvatar.target != null && game.world.myAvatar.target.dataLeaf.intHP > 0) {
        game.world.approachTarget();
        return tryUseSkill(game, skill);
      }

      return false;
    }

    [BridgeExport]
    public static function getSkillCooldownRemaining(index:int):int {
      var game:Object = Main.Game;
      var skill:* = game.world.actions.active[index];
      if (!skill) {
        return 0;
      }

      return getSkillCooldownRemainingValue(skill);
    }

    private static function hasSpiritPower(leaf:Object, needed:int):Boolean {
      for each (var aura:Object in leaf.auras) {
        if (aura.nam == "Spirit Power") {
          return needed <= aura.val;
        }
      }

      return false;
    }

    private static function isDisabled(world:Object, leaf:Object):Boolean {
      for each (var aura:Object in leaf.auras) {
        var category:String = world.auraCatOf(aura);
        if (category == "stun" || category == "stone" || category == "paralyze" || category == "disable" || category == "disabled") {
          return true;
        }
      }

      return false;
    }

    // Mirrors the refusals in the client's World.testAction, except range. actionTimeCheck clears skill.OldCD once a
    // skill is ready, so it is called only for the auto attack, whose branch returns before that.
    [BridgeExport]
    public static function canUseSkill(index:int):Boolean {
      var world:Object = Main.Game.world;
      var skill:Object = world.actions && world.actions.active ? world.actions.active[index] : null;
      var self:Object = world.myAvatar;
      var leaf:Object = self.dataLeaf;
      if (!skill || !skill.isOK || skill.skillLock || skill.lock || leaf.intState == 0) {
        return false;
      }

      if (skill.auto ? !world.actionTimeCheck(skill) : getSkillCooldownRemainingValue(skill) > 0) {
        return false;
      }

      if (skill.ref == "i1" && (skill.sArg1 == "" || (world.lockdownPots && new Date().getTime() - world.lockdownPotsTS <= 5000))) {
        return false;
      }

      if (skill.mp != null && Math.round(skill.mp * leaf.sta["$cmc"]) > leaf.intMP) {
        return false;
      }

      if ((skill.sp != null && !hasSpiritPower(leaf, skill.sp)) || isDisabled(world, leaf)) {
        return false;
      }

      if (skill.pet == null && skill.checkPet != null) {
        var pet:Object = self.getItemByEquipSlot("pe");
        if (pet == null || pet.sMeta == null || pet.sMeta.indexOf(skill.checkPet) == -1) {
          return false;
        }
      }

      if (skill.tgt == "s" || skill.tgt == "f") {
        return true;
      }

      var target:Object = self.target;
      if (target == null || target == self || target.dataLeaf == null || target.dataLeaf.intHP <= 0) {
        return false;
      }

      return !("filter" in skill && "sRace" in target.objData) || target.objData.sRace.toLowerCase() == skill.filter.toLowerCase();
    }

    [BridgeTsParamType("selector: FlashTypes.MonsterSelector")]
    [BridgeTsReturnType("FlashTypes.ConsumableCastDispatch | null")]
    [BridgeExport]
    public static function castConsumableOnMonster(selector:Object, expectedItemId:int):Object {
      // Queued requests can outlive the equipped item or target, and testAction
      // may silently decline; only a new action identity proves dispatch.
      if (!selector || expectedItemId <= 0) {
        return null;
      }

      var game:Object = Main.Game;
      var world:Object = game ? game.world : null;
      var player:Object = world ? world.myAvatar : null;
      if (
        !world ||
        !player ||
        !player.dataLeaf ||
        !world.actions ||
        !world.actions.active
      ) {
        return null;
      }

      var playerState:Number = Number(player.dataLeaf.intState);
      var playerHp:Number = Number(player.dataLeaf.intHP);
      if (
        isNaN(playerState) ||
        playerState <= 0 ||
        isNaN(playerHp) ||
        playerHp <= 0
      ) {
        return null;
      }

      var skill:Object = world.actions.active[CONSUMABLE_SKILL_INDEX];
      if (
        !skill ||
        skill.ref != "i1" ||
        skill.sArg1 == null ||
        !skill.isOK ||
        skill.skillLock ||
        skill.lock ||
        world.lockdownPots
      ) {
        return null;
      }

      var itemId:Number = Number(skill.sArg1);
      if (
        isNaN(itemId) ||
        itemId <= 0 ||
        itemId != expectedItemId ||
        getSkillCooldownRemainingValue(skill) > 0
      ) {
        return null;
      }

      var monster:Object = World.getMonster(selector);
      if (
        !isMonsterAttackable(monster) ||
        monster.npcType != "monster" ||
        !monster.pMC ||
        !monster.pMC.visible
      ) {
        return null;
      }

      var monsterMapId:Number = Number(monster.dataLeaf.MonMapID);
      if (isNaN(monsterMapId) || monsterMapId <= 0) {
        return null;
      }

      world.setTarget(monster);
      if (world.myAvatar.target !== monster) {
        return null;
      }

      var previousActionId:* = skill.actID;
      var previousTimestamp:* = skill.ts;
      world.testAction(skill);
      if (
        !skill.lock ||
        skill.actID == null ||
        Number(skill.actID) < 0 ||
        (skill.actID === previousActionId && skill.ts === previousTimestamp)
      ) {
        return null;
      }

      return {
        actionId: Number(skill.actID),
        itemId: itemId,
        monsterMapId: monsterMapId
      };
    }

    [BridgeExport]
    public static function cancelAutoAttack():void {
      var game:Object = Main.Game;
      game.world.cancelAutoAttack();
    }

    [BridgeExport]
    public static function cancelTarget():void {
      // cancelTarget returns after stopping active auto-attack, so another call
      // is needed to clear the selected target.
      var game:Object = Main.Game;
      game.world.cancelTarget();
      game.world.cancelTarget();
    }

    [BridgeTsParamType("selector: FlashTypes.MonsterSelector")]
    [BridgeExport]
    public static function attackMonster(selector:Object):Boolean {
      if (!selector)
        return false;

      var game:Object = Main.Game;
      var monster:Object = World.getMonster(selector);
      if (isMonsterAttackable(monster)) {
        game.world.setTarget(monster);
        if (game.world.myAvatar.target !== monster)
          return false;

        game.world.approachTarget();
        return true;
      }

      return false;
    }
  }
}
