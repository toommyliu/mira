package lyra.game {
  import lyra.Main;
  import flash.utils.ByteArray;
  import flash.utils.getTimer;

  [BridgeNamespace("quests")]
  public class Quests {
    private static const REWARD_GROUPS:Array = ["itemsS", "itemsC", "itemsR", "itemsrand"];
    private static const REWARD_KINDS:Array = ["guaranteed", "choice", "roll", "random"];
    private static const MAX_TURN_INS:int = 250;
    private static const LOAD_SPACING:int = 1500;

    private static var lastLoad:int = -LOAD_SPACING;

    private static function find(questId:int):Object {
      return Main.Game.world.questTree[questId];
    }

    private static function cadence(quest:Object):String {
      if (quest.sField == null) {
        return "none";
      }

      return quest.sField == "im0" ? "monthly" : quest.sField == "iw0" ? "weekly" : "daily";
    }

    private static function requirements(quest:Object):Array {
      var rows:Array = [];
      for each (var need:Object in quest.turnin) {
        var item:Object = quest.oItems ? quest.oItems[need.ItemID] : null;
        rows.push({itemId: need.ItemID, name: item ? item.sName : null, quantity: need.iQty});
      }

      return rows;
    }

    // Mirrors how the client's quest window labels each reward and its chance.
    private static function rewards(quest:Object):Array {
      var rows:Array = [];
      for (var index:int = 0; index < REWARD_GROUPS.length; index++) {
        var group:Object = quest.oRewards ? quest.oRewards[REWARD_GROUPS[index]] : null;
        var items:Array = [];
        for each (var item:Object in group) {
          items.push(item);
        }

        for each (item in items) {
          var chance:Number = 100;
          if (REWARD_GROUPS[index] == "itemsrand") {
            chance = 100 / items.length;
          }
          else {
            for each (var rate:Object in quest.reward) {
              if (rate.ItemID == item.ItemID && rate.iQty == item.iQty) {
                chance = Number(rate.iRate);
              }
            }
          }

          rows.push({
            itemId: item.ItemID,
            name: item.sName,
            quantity: item.iQty,
            kind: REWARD_KINDS[index],
            dropChance: chance
          });
        }
      }

      return rows;
    }

    [BridgeIgnore]
    public static function row(quest:Object):Object {
      return {
        id: quest.QuestID,
        name: quest.sName,
        cadence: cadence(quest),
        once: quest.bOnce == 1,
        requirements: requirements(quest),
        rewards: rewards(quest)
      };
    }

    // Mirrors the quest window's accept button and the quests its list hides, without message boxes.
    private static function canAccept(quest:Object):Boolean {
      var world:Object = Main.Game.world;
      if (quest.bOnce == 1 && (quest.iSlot < 0 || world.getQuestValue(quest.iSlot) >= quest.iValue)) {
        return false;
      }

      if (quest.hasOwnProperty("sMeta") && String(quest.sMeta).indexOf("EmailConfirm") != -1 && world.myAvatar.objData.intActivationFlag < 5) {
        return false;
      }

      return Main.Game.getQuestValidationString(quest) == "";
    }

    // World.maximumQuestTurnIns, without the message box it shows for equipped items.
    private static function turnInsHeld(quest:Object):int {
      var world:Object = Main.Game.world;
      var most:int = -1;
      for each (var need:Object in quest.turnin) {
        var held:Object = world.invTree[need.ItemID];
        if (!held || int(held.iQty) < int(need.iQty) || world.myAvatar.isItemEquipped(need.ItemID)) {
          return 0;
        }

        var times:int = Math.floor(int(held.iQty) / int(need.iQty));
        if (most == -1 || times < most) {
          most = times;
        }
      }

      return Math.min(most, MAX_TURN_INS);
    }

    // The quest window offers more than one turn-in only for repeatable quests.
    private static function maxTurnIns(quest:Object):int {
      var held:int = turnInsHeld(quest);
      if (held == 0) {
        return 0;
      }

      return quest.sField == null && quest.bOnce != 1 && held > 1 ? held : 1;
    }

    // World.coolDown posts a chat warning when the action is not ready yet, so it is only called once it is.
    private static function ready(action:String):Boolean {
      return World.isActionAvailable(action) && Main.Game.world.coolDown(action);
    }

    [BridgeExport]
    public static function get(questId:int):Object {
      var quest:Object = find(questId);
      return quest ? row(quest) : null;
    }

    [BridgeExport]
    public static function getAccepted():Array {
      var rows:Array = [];
      for each (var quest:Object in Main.Game.world.questTree) {
        if (quest && quest.status != null) {
          rows.push(row(quest));
        }
      }

      return rows;
    }

    [BridgeExport]
    public static function getTree():Array
    {
      var game:Object = Main.Game;
      var quests:Array = [];
      for each (var q:Object in game.world.questTree)
      {
        var quest:Object = cloneObject(q);
        var requirements:Array = [];
        var rewards:Array = [];

        if (q.turnin != null && q.oItems != null)
        {
          for each (var req:Object in q.turnin)
          {
            var _req:Object = new Object();
            var item:Object = q.oItems[req.ItemID];
            _req.sName = item.sName;
            _req.ItemID = item.ItemID;
            _req.iQty = req.iQty;
            requirements.push(_req);
          }
        }

        quest.RequiredItems = requirements;

        if (q.reward != null && q.oRewards != null)
        {
          for each (var rew:Object in q.reward)
          {
            for each (var rewContainer:* in q.oRewards)
            {
              for each (var _item:Object in rewContainer)
              {
                if (_item.ItemID != null && _item.ItemID == rew.ItemID)
                {
                  var reward:Object = new Object();
                  reward.sName = _item.sName;
                  reward.ItemID = rew.ItemID;
                  reward.iQty = rew.iQty;
                  reward.DropChance = String(rew.iRate) + "%";
                  rewards.push(reward);
                }
              }
            }
          }
        }

        quest.Rewards = rewards;
        quests.push(quest);
      }

      return quests;
    }


    [BridgeExport]
    public static function isInProgress(questId:int):Boolean {
      return Main.Game.world.isQuestInProgress(questId);
    }

    [BridgeExport]
    public static function isAvailable(questId:int):Boolean {
      var quest:Object = find(questId);
      return quest != null && canAccept(quest);
    }

    [BridgeExport]
    public static function canComplete(questId:int):Boolean {
      var quest:Object = find(questId);
      return quest != null && isInProgress(questId) && maxTurnIns(quest) > 0;
    }

    [BridgeExport]
    public static function getMaxTurnIns(questId:int):int {
      var quest:Object = find(questId);
      return quest ? maxTurnIns(quest) : 0;
    }

    // The server ignores a quest load sent too soon after the last one, so loads wait their turn here.
    // The client's quest log does nothing while a greedy window is open.
    [BridgeExport]
    public static function load(questIds:String, silent:Boolean):String {
      var game:Object = Main.Game;
      if (!silent && game.isGreedyModalInStack()) {
        return "blocked";
      }

      if (getTimer() - lastLoad < LOAD_SPACING) {
        return "wait";
      }

      lastLoad = getTimer();
      if (silent) {
        game.world.getQuests(questIds.split(","));
        return "sent";
      }

      closeQuestFrame(game);
      game.world.showQuests(questIds, "q");
      return "sent";
    }

    // A quest window that is still waiting for its quest data throws when closed, so it is removed instead.
    private static function closeQuestFrame(game:Object):void {
      var frame:Object = game.getInstanceFromModalStack("QFrameMC");
      if (!frame) {
        return;
      }

      try {
        frame.fClose();
      }
      catch (error:Error) {
        if (frame.parent) {
          frame.parent.removeChild(frame);
        }
      }
    }

    [BridgeExport]
    public static function accept(questId:int):Boolean {
      var quest:Object = find(questId);
      if (!quest || isInProgress(questId) || !canAccept(quest) || !ready("acceptQuest")) {
        return false;
      }

      Main.Game.world.acceptQuest(questId);
      return true;
    }

    // Mirrors the quest window's complete button, which requires a reward pick when the quest offers one.
    [BridgeExport]
    public static function complete(questId:int, turnIns:int, rewardItemId:int):Boolean {
      var quest:Object = find(questId);
      if (!quest || !isInProgress(questId) || turnIns < 1 || turnIns > maxTurnIns(quest)) {
        return false;
      }

      var choices:Object = quest.oRewards ? quest.oRewards.itemsC : null;
      var choice:int = -1;
      if (choices != null) {
        for each (var item:Object in choices) {
          if (int(item.ItemID) == rewardItemId) {
            choice = rewardItemId;
          }
        }

        if (choice == -1) {
          return false;
        }
      }

      if (!ready("tryQuestComplete")) {
        return false;
      }

      Main.Game.world.tryQuestComplete(questId, choice, false, turnIns);
      return true;
    }

    [BridgeExport]
    public static function abandon(questId:int):Boolean {
      if (!isInProgress(questId)) {
        return false;
      }

      Main.Game.world.abandonQuest(questId);
      return true;
    }

    private static function cloneObject(source:Object):Object
    {
      var ba:ByteArray = new ByteArray();
      ba.writeObject(source);
      ba.position = 0;
      return ba.readObject();
    }
  }
}
