using Newtonsoft.Json.Linq;
using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Events(Func<string?> username)
{
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

    internal void Raise(string name, params object?[] args)
    {
        Delegate? handler;
        lock (gate) handler = handlers.GetValueOrDefault(name);
        Faults.Invoke(name, handler, args);
    }

    internal event Action<string>? Commands;

    internal void Extension(JObject packet)
    {
        if (Command(packet) is { } command)
        {
            Commands?.Invoke(command);
            Game(command, packet.SelectToken("params.dataObj"));
        }
        Raise(nameof(ExtensionPacketReceived), packet);
    }

    private void Game(string command, JToken? data)
    {
        switch (command)
        {
            case "moveToArea" when data is JObject area:
                Raise(nameof(MapChanged), area["strMapName"]?.ToString() ?? string.Empty);
                break;
            case "addGoldExp" when data is JObject reward && reward["typ"]?.ToString() == "m"
                && int.TryParse(reward["id"]?.ToString(), out var monster):
                Raise(nameof(MonsterKilled), monster);
                break;
            case "uotls" when data is JArray { Count: > 3 } update && update[3].ToString() == "afk:true"
                && Subscribed(nameof(PlayerAFK)) && IsMe(update[2].ToString()):
                Raise(nameof(PlayerAFK));
                break;
        }
    }

    private bool Subscribed(string name)
    {
        lock (gate) return handlers.ContainsKey(name);
    }

    private bool IsMe(string name)
    {
        try
        {
            return string.Equals(name, username(), StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception error) when (error is not OperationCanceledException)
        {
            return false;
        }
    }

    private static string? Command(JObject packet) => packet.SelectToken("params.dataObj") switch
    {
        JObject data => data.Value<string>("cmd"),
        JArray { Count: > 0 } data => data[0].Type == JTokenType.String ? data[0].Value<string>() : null,
        _ => null,
    };

    internal void Stopping(Exception? error) => Raise(nameof(ScriptStopping), error);

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
