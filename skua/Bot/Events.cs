using CommunityToolkit.Mvvm.Messaging;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Skua.Core.Interfaces;
using Skua.Core.Messaging;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot;

public sealed class Events(Func<IScriptInterface> bot) : IScriptEvent
{
    private const int GameEvents = (int)MessageChannels.GameEvents;

    private readonly object gate = new();
    private readonly Dictionary<string, Delegate> handlers = [];

    public event LogoutEventHandler? Logout { add => Add(nameof(Logout), value); remove => Remove(nameof(Logout), value); }
    public event PlayerDeathEventHandler? PlayerDeath { add => Add(nameof(PlayerDeath), value); remove => Remove(nameof(PlayerDeath), value); }
    public event MonsterKilledEventHandler? MonsterKilled { add => Add(nameof(MonsterKilled), value); remove => Remove(nameof(MonsterKilled), value); }
    public event QuestAcceptedEventHandler? QuestAccepted { add => Add(nameof(QuestAccepted), value); remove => Remove(nameof(QuestAccepted), value); }
    public event QuestTurnInEventHandler? QuestTurnedIn { add => Add(nameof(QuestTurnedIn), value); remove => Remove(nameof(QuestTurnedIn), value); }
    public event MapChangedEventHandler? MapChanged { add => Add(nameof(MapChanged), value); remove => Remove(nameof(MapChanged), value); }
    public event CellChangedEventHandler? CellChanged { add => Add(nameof(CellChanged), value); remove => Remove(nameof(CellChanged), value); }
    public event ReloginTriggeredEventHandler? ReloginTriggered { add => Add(nameof(ReloginTriggered), value); remove => Remove(nameof(ReloginTriggered), value); }
    public event ExtensionPacketEventHandler? ExtensionPacketReceived { add => Add(nameof(ExtensionPacketReceived), value); remove => Remove(nameof(ExtensionPacketReceived), value); }
    public event AFKEventHandler? PlayerAFK { add => Add(nameof(PlayerAFK), value); remove => Remove(nameof(PlayerAFK), value); }
    public event TryBuyItemHandler? TryBuyItem { add => Add(nameof(TryBuyItem), value); remove => Remove(nameof(TryBuyItem), value); }
    public event CounterAttackHandler? CounterAttack { add => Add(nameof(CounterAttack), value); remove => Remove(nameof(CounterAttack), value); }
    public event ItemDroppedHandler? ItemDropped { add => Add(nameof(ItemDropped), value); remove => Remove(nameof(ItemDropped), value); }
    public event ItemSoldHandler? ItemSold { add => Add(nameof(ItemSold), value); remove => Remove(nameof(ItemSold), value); }
    public event ItemBoughtHandler? ItemBought { add => Add(nameof(ItemBought), value); remove => Remove(nameof(ItemBought), value); }
    public event ItemAddedToBankHandler? ItemAddedToBank { add => Add(nameof(ItemAddedToBank), value); remove => Remove(nameof(ItemAddedToBank), value); }
    public event ScriptStoppingHandler? ScriptStopping { add => Add(nameof(ScriptStopping), value); remove => Remove(nameof(ScriptStopping), value); }
    public event RunToAreaHandler? RunToArea { add => Add(nameof(RunToArea), value); remove => Remove(nameof(RunToArea), value); }

    private static IMessenger Messenger => StrongReferenceMessenger.Default;

    public void ClearHandlers()
    {
        lock (gate)
        {
            var stopping = handlers.GetValueOrDefault(nameof(ScriptStopping));
            handlers.Clear();
            if (stopping is not null)
                handlers[nameof(ScriptStopping)] = stopping;
        }
    }

    internal void Extension(JObject packet)
    {
        try
        {
            switch (packet.SelectToken("params.type")?.ToString(), packet.SelectToken("params.dataObj"))
            {
                case ("json", JObject data):
                    Json(data);
                    break;
                case ("str", JArray { Count: > 0 } data):
                    Text(data);
                    break;
            }
        }
        catch (Exception error) when (error is not OperationCanceledException)
        {
            Faults.Failed("packet", error);
        }
        Raise(nameof(ExtensionPacketReceived), packet);
    }

    internal void Sent(string packet)
    {
        var parts = packet.Split('%');
        if (parts.Length < 4)
            return;
        switch (parts[3])
        {
            case "moveToCell" when parts.Length > 6 && parts[5].Length > 0 && parts[6].Length > 0:
                CellMoved(bot().Map.Name, parts[5], parts[6]);
                break;
            case "buyItem" when parts.Length > 7 && int.TryParse(parts[5], out var itemId) && int.TryParse(parts[6], out var shopId)
                && int.TryParse(parts[7], out var shopItemId):
                Raise(nameof(TryBuyItem), shopId, itemId, shopItemId);
                break;
            case "acceptQuest" when parts.Length > 5 && int.TryParse(parts[5], out var questId):
                Raise(nameof(QuestAccepted), questId);
                break;
            case "cmd" when parts.Length > 5 && parts[5] == "logout":
                Raise(nameof(Logout));
                break;
        }
    }

    internal event Action<string>? Commands;

    private void Json(JObject data)
    {
        var command = data.Value<string>("cmd");
        if (command is not null)
            Commands?.Invoke(command);
        switch (command)
        {
            case "event" when data.SelectToken("args.zoneSet")?.ToString() is { } zone:
                Raise(nameof(RunToArea), zone);
                break;
            case "moveToArea":
                var map = data.Value<string>("strMapName") ?? string.Empty;
                bot().Map.FilePath = data.Value<string>("strMapFileName") ?? string.Empty;
                bot().Map.LastMap = map;
                Raise(nameof(MapChanged), map);
                break;
            case "ct" when Subscribed(nameof(PlayerDeath)) && data["p"] is JObject players
                && players.SelectToken($"['{bot().Player.Username.ToLower()}'].intHP")?.Value<int>() == 0:
                Raise(nameof(PlayerDeath));
                break;
            case "sellItem":
                var sold = (CharItemID: data.Value<int>("CharItemID"), Quantity: data.Value<int>("iQty"), Now: data.Value<int>("iQtyNow"),
                    Amount: data.Value<int>("intAmount"), Coins: data.Value<int>("bCoins") == 1);
                Messenger.Send(new ItemSoldMessage(sold.CharItemID, sold.Quantity, sold.Now, sold.Amount, sold.Coins));
                Raise(nameof(ItemSold), sold.CharItemID, sold.Quantity, sold.Now, sold.Amount, sold.Coins);
                break;
            case "buyItem" when data.Value<int>("bitSuccess") == 1:
                Messenger.Send(new ItemBoughtMessage(data.Value<int>("CharItemID")));
                Raise(nameof(ItemBought), data.Value<int>("CharItemID"));
                break;
            case "dropItem" when data["items"] is JObject items:
                foreach (var item in items.Properties())
                    Dropped(item.Value.ToObject<InventoryItem>()!, false, 0);
                break;
            case "addItems" when data["items"] is JObject added:
                foreach (var item in added.Properties())
                    Added(item);
                break;
            case "getDrop" when data.Value<bool>("bBank") && data.Value<int>("bSuccess") == 1
                && bot().Bank.GetItem(data.Value<int>("ItemID")) is { } toBank:
                Raise(nameof(ItemAddedToBank), toBank, data.Value<int>("iQtyNow"));
                break;
            case "addGoldExp" when data.Value<string>("typ") == "m":
                Raise(nameof(MonsterKilled), data.Value<int>("id"));
                break;
            case "ccqr" when data.Value<int>("bSuccess") == 1:
                Raise(nameof(QuestTurnedIn), data.Value<int>("QuestID"));
                break;
            case "loadBank":
                Messenger.Send(new BankLoadedMessage());
                break;
        }
    }

    private void Text(JArray data)
    {
        switch (data[0].ToString())
        {
            case "uotls" when data.Count > 3 && data[3].ToString() == "afk:true" && Subscribed(nameof(PlayerAFK))
                && string.Equals(data[2].ToString(), bot().Player.Username, StringComparison.OrdinalIgnoreCase):
                Raise(nameof(PlayerAFK));
                break;
        }
    }

    private void Added(JProperty item)
    {
        if (!int.TryParse(item.Name, out var id))
            return;
        if (((ItemBase?)bot().Inventory.GetItem(id) ?? bot().TempInv.GetItem(id)) is { } held)
            Dropped(held, true, item.Value.Value<int>("iQtyNow"));
        else if (bot().Bank.GetItem(id) is { } banked)
            Raise(nameof(ItemAddedToBank), banked, banked.Quantity);
    }

    private void CellMoved(string map, string cell, string pad)
    {
        Messenger.Send(new CellChangedMessage(map, cell, pad), GameEvents);
        Raise(nameof(CellChanged), map, cell, pad);
    }

    private void Dropped(ItemBase item, bool addedToInventory, int quantityNow)
    {
        Messenger.Send(new ItemDroppedMessage(item, addedToInventory, quantityNow), GameEvents);
        Raise(nameof(ItemDropped), item, addedToInventory, quantityNow);
    }

    internal void Stopping(Exception? error) => Raise(nameof(ScriptStopping), error);

    private void Raise(string name, params object?[] args)
    {
        Delegate? handler;
        lock (gate) handler = handlers.GetValueOrDefault(name);
        Faults.Invoke(name, handler, args);
    }

    private bool Subscribed(string name)
    {
        lock (gate) return handlers.ContainsKey(name);
    }

    private void Add(string name, Delegate? value)
    {
        lock (gate) handlers[name] = Delegate.Combine(handlers.GetValueOrDefault(name), value)!;
    }

    private void Remove(string name, Delegate? value)
    {
        lock (gate)
        {
            var left = Delegate.Remove(handlers.GetValueOrDefault(name), value);
            if (left is null) handlers.Remove(name); else handlers[name] = left;
        }
    }
}
