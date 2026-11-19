using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class Wait
{
    private static readonly Dictionary<GameActions, string> Locks = new()
    {
        [GameActions.LoadShop] = "loadShop", [GameActions.LoadEnhShop] = "loadEnhShop", [GameActions.LoadHairShop] = "loadHairShop",
        [GameActions.EquipItem] = "equipItem", [GameActions.UnequipItem] = "unequipItem", [GameActions.BuyItem] = "buyItem",
        [GameActions.SellItem] = "sellItem", [GameActions.GetMapItem] = "getMapItem", [GameActions.TryQuestComplete] = "tryQuestComplete",
        [GameActions.AcceptQuest] = "acceptQuest", [GameActions.DoIA] = "doIA", [GameActions.Rest] = "rest",
        [GameActions.Who] = "who", [GameActions.Transfer] = "tfer",
    };

    private readonly Func<IScriptInterface> bot;
    private readonly AutoResetEvent bought = new(false);
    private readonly AutoResetEvent sold = new(false);
    private readonly AutoResetEvent bankLoaded = new(false);

    public Wait(Func<IScriptInterface> bot, Events events)
    {
        this.bot = bot;
        events.Commands += Signal;
    }

    private void Signal(string command)
    {
        switch (command)
        {
            case "buyItem":
                bought.Set();
                break;
            case "sellItem":
                sold.Set();
                break;
            case "loadBank":
                bankLoaded.Set();
                break;
        }
    }

    private IScriptInterface Bot => bot();
    private IScriptPlayer Player => Bot.Player;

    public int WAIT_SLEEP { get; set; } = 100;
    public bool OverrideTimeout { get; set; }
    public int PlayerActionTimeout { get; set; } = 15;
    public int MonsterActionTimeout { get; set; } = 15;
    public int MapActionTimeout { get; set; } = 25;
    public int DropActionTimeout { get; set; } = 10;
    public int ItemActionTimeout { get; set; } = 14;
    public int QuestActionTimeout { get; set; } = 14;
    public int GameActionTimeout { get; set; } = 40;

    private int Pick(int custom, int timeout) => OverrideTimeout ? custom : timeout;

    public bool ForPlayerPosition(int x, int y, int timeout = 10) =>
        ForTrue(() => !Player.Playing || (Player.X == x && Player.Y == y), Pick(PlayerActionTimeout, timeout));

    public bool ForCombatExit(int timeout = 10) => ForTrue(() => !Player.Playing || !Player.InCombat, Pick(PlayerActionTimeout, timeout));

    public bool ForMonsterDeath(int timeout = -1) =>
        ForTrue(() => !Player.Playing || !Player.HasTarget, () => Bot.Combat.UntargetSelf(), timeout, WAIT_SLEEP / 5);

    public bool ForMonsterSpawn(string name, int timeout = 10) =>
        ForTrue(() => !Player.Playing || Bot.Monsters.Exists(name), Pick(MonsterActionTimeout, timeout));

    public bool ForMonsterSpawn(int id, int timeout = 10) =>
        ForTrue(() => !Player.Playing || Bot.Monsters.Exists(id), Pick(MonsterActionTimeout, timeout));

    public bool ForFullyRested(int timeout = -1) =>
        ForTrue(() => !Player.Playing || (Player.Health >= Player.MaxHealth && Player.Mana >= Player.MaxMana), timeout);

    public bool ForMapLoad(string name, int timeout = 20)
    {
        var dash = name.IndexOf('-');
        var clean = (dash >= 0 ? name[..dash] : name).ToLower();
        return ForTrue(() => Bot.Map.Name == clean, Pick(MapActionTimeout, timeout))
            && ForTrue(() => !Player.Playing || Bot.Map.Loaded, Pick(MapActionTimeout, timeout));
    }

    public bool ForCellChange(string name) => ForTrue(() => !Player.Playing || Player.Cell == name, Pick(MapActionTimeout, WAIT_SLEEP / 4));

    public bool ForPickup(string name, int timeout = 10) => ForTrue(() => !Player.Playing || !Bot.Drops.Exists(name), Pick(DropActionTimeout, timeout));

    public bool ForPickup(int id, int timeout = 10) => ForTrue(() => !Player.Playing || !Bot.Drops.Exists(id), Pick(DropActionTimeout, timeout));

    public bool ForDrop(string name, int timeout = 10) => ForTrue(() => !Player.Playing || Bot.Drops.Exists(name), Pick(DropActionTimeout, timeout));

    public bool ForDrop(int id, int timeout = 10) => ForTrue(() => !Player.Playing || Bot.Drops.Exists(id), Pick(DropActionTimeout, timeout));

    public bool ForItemBuy(int timeout = 10) => bought.WaitOne(Pick(ItemActionTimeout, timeout) * WAIT_SLEEP);

    public bool ForItemSell(int timeout = 10) => sold.WaitOne(Pick(ItemActionTimeout, timeout) * WAIT_SLEEP);

    public bool ForItemEquip(int id, int timeout = 10) =>
        ForTrue(() => !Player.Playing || (Bot.Inventory.TryGetItem(id, out InventoryItem? item) && item!.Equipped), Pick(ItemActionTimeout, timeout));

    public bool ForItemEquip(string name, int timeout = 10) =>
        ForTrue(() => !Player.Playing || !Bot.Inventory.TryGetItem(name, out InventoryItem? item) || item!.Equipped, Pick(ItemActionTimeout, timeout));

    public bool ForBankToInventory(string name, int timeout = 14) =>
        ForTrue(() => !Player.Playing || Bot.Inventory.Contains(name), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForBankToInventory(int id, int timeout = 14) =>
        ForTrue(() => !Player.Playing || Bot.Inventory.Contains(id), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForBankToHouseInventory(string name, int timeout = 14) =>
        ForTrue(() => !Player.Playing || Bot.House.Contains(name), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForBankToHouseInventory(int id, int timeout = 14) =>
        ForTrue(() => !Player.Playing || Bot.House.Contains(id), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForInventoryToBank(string name, int timeout = 14) =>
        ForTrue(() => !Player.Playing || !Bot.Inventory.Contains(name), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForInventoryToBank(int id, int timeout = 14) =>
        ForTrue(() => !Player.Playing || !Bot.Inventory.Contains(id), Pick(ItemActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForHouseInventoryToBank(string name, int timeout = 14) =>
        ForTrue(() => !Player.Playing || !Bot.House.Contains(name), Pick(ItemActionTimeout, timeout), WAIT_SLEEP);

    public bool ForHouseInventoryToBank(int id, int timeout = 14) =>
        ForTrue(() => !Player.Playing || !Bot.House.Contains(id), Pick(ItemActionTimeout, timeout), WAIT_SLEEP);

    public bool ForBankLoad(int timeout = 20) => bankLoaded.WaitOne(timeout * WAIT_SLEEP);

    public bool ForQuestAccept(int id, int timeout = 14) =>
        ForTrue(() => !Player.Playing || Bot.Quests.IsInProgress(id), Pick(QuestActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForQuestComplete(int id, int timeout = 10) =>
        ForTrue(() => !Player.Playing || !Bot.Quests.IsInProgress(id), Pick(QuestActionTimeout, timeout), WAIT_SLEEP / 2);

    public bool ForQuestLoad(int id, int timeout = 20) =>
        ForTrue(() => !Player.Playing || Bot.Quests.Tree.Exists(quest => quest.ID == id), timeout);

    public bool ForQuestLoad(int startId, int endId, int timeout = 20)
    {
        var before = Bot.Quests.Tree.Count;
        return ForTrue(() => !Player.Playing || Bot.Quests.Tree.Count > before
            || Bot.Quests.Tree.Exists(quest => quest.ID >= startId && quest.ID <= endId), timeout);
    }

    public bool ForSkillCooldown(int index, int timeout = 50) => ForTrue(() => Bot.Skills.CanUseSkill(index), timeout, WAIT_SLEEP);

    public bool For(Func<object> function, object value, int timeout = 10) => ForTrue(() => function() == value, timeout);

    public ValueTask<bool> ForAsync(Func<object> function, object value, int timeout = 10, CancellationToken token = default) =>
        ForTrueAsync(() => function() == value, timeout, token: token);

    public bool ForActionCooldown(GameActions action, int timeout = 40) => ForActionCooldown(Locks[action], Pick(GameActionTimeout, timeout));

    public bool ForActionCooldown(string action, int timeout = 40) => ForTrue(() => IsActionAvailable(action), Pick(GameActionTimeout, timeout));

    public bool IsActionAvailable(GameActions action) => IsActionAvailable(Locks[action]);

    public bool IsActionAvailable(string action)
    {
        if (!Player.Playing)
            return false;
        try
        {
            var locked = JsonConvert.DeserializeObject<GameActionLock>(Bot.Flash.GetGameObject($"world.lock.{action}")!);
            return DateTimeOffset.Now.ToUnixTimeMilliseconds() - locked!.TS >= locked.CD;
        }
        catch (Exception error) when (error is JsonException or ArgumentNullException or NullReferenceException)
        {
            return false;
        }
    }

    public bool ForTrue(Func<bool> predicate, int timeout, int sleepOverride = -1) => ForTrue(predicate, null, timeout, sleepOverride);

    public bool ForTrue(Func<bool> predicate, Action? loopFunction, int timeout, int sleepOverride = -1)
    {
        for (var counter = 0; !predicate() && !Bot.ShouldExit; counter++)
        {
            if (timeout > 0 && counter >= timeout)
                return false;
            loopFunction?.Invoke();
            Bot.Sleep(sleepOverride == -1 ? WAIT_SLEEP : sleepOverride);
        }
        return true;
    }

    public bool ForTrue(Func<bool> predicate, Action? loopFunction, int sleepOverride, CancellationToken token)
    {
        while (!predicate() && !Bot.ShouldExit && !token.IsCancellationRequested)
        {
            loopFunction?.Invoke();
            Bot.Sleep(sleepOverride == -1 ? WAIT_SLEEP : sleepOverride);
        }
        return true;
    }

    public ValueTask<bool> ForTrueAsync(Func<bool> predicate, int timeout, int sleepOverride = -1, CancellationToken token = default) =>
        ForTrueAsync(predicate, null, timeout, sleepOverride, token);

    public async ValueTask<bool> ForTrueAsync(Func<bool> predicate, Action? loopFunction, int timeout, int sleepOverride = -1, CancellationToken token = default)
    {
        for (var counter = 0; !predicate() && !Bot.ShouldExit && !token.IsCancellationRequested; counter++)
        {
            if (timeout > 0 && counter >= timeout)
                return false;
            loopFunction?.Invoke();
            await Task.Delay(sleepOverride == -1 ? WAIT_SLEEP : sleepOverride, token);
        }
        return true;
    }
}
