using System.ComponentModel;
using System.Text.Json.Nodes;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class Drops(Game game) : IScriptDrop
{
    private readonly HashSet<string> names = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<int> ids = [];
    private readonly HashSet<string> keptNames = new(StringComparer.OrdinalIgnoreCase);
    private readonly HashSet<int> keptIds = [];
    private JsonNode? configBefore;
    private bool enabledChanged;
    private bool rejectChanged;
    private bool rejectElse;

    public event PropertyChangedEventHandler? PropertyChanged { add { } remove { } }

    public IEnumerable<ItemBase> CurrentDropInfos => Items.Many<ItemBase>(game.Request("drops", "items"));
    public IEnumerable<string> CurrentDrops => CurrentDropInfos.Select(item => item.Name.Trim()).ToList();
    public bool Enabled { get; private set; }
    public int Interval { get; set; } = 1000;
    public IEnumerable<string> ToPickup => names;
    public IEnumerable<int> ToPickupIDs => ids;

    public bool RejectElse
    {
        get => rejectElse;
        set
        {
            rejectElse = value;
            rejectChanged = true;
            game.Request("dropRegistry", "configure", new { rejectOthers = value });
        }
    }

    public void Add(params string[] added)
    {
        names.UnionWith(added.Select(name => name.Trim()));
        game.Request("dropRegistry", "register", new { items = added });
    }

    public void Add(params int[] added)
    {
        ids.UnionWith(added);
        game.Request("dropRegistry", "register", new { items = added.Select(id => new { itemId = id }) });
    }

    public void Remove(params string[] removed)
    {
        var owned = removed.Where(name => names.Contains(name.Trim()) && !keptNames.Contains(name.Trim())).ToArray();
        if (owned.Length > 0)
            game.Request("dropRegistry", "unregister", new { items = owned });
        names.ExceptWith(removed.Select(name => name.Trim()));
    }

    public void Remove(params int[] removed)
    {
        var owned = removed.Where(id => ids.Contains(id) && !keptIds.Contains(id)).ToArray();
        if (owned.Length > 0)
            game.Request("dropRegistry", "unregister", new { items = owned.Select(id => new { itemId = id }) });
        ids.ExceptWith(removed);
    }

    public void Clear() => Clear(game.Calls);

    private void Clear(CancellationToken token)
    {
        object[] owned = [.. names.Where(name => !keptNames.Contains(name)), .. ids.Where(id => !keptIds.Contains(id)).Select(id => new { itemId = id })];
        if (owned.Length > 0)
            game.Request("dropRegistry", "unregister", new { items = owned }, token);
        names.Clear();
        ids.Clear();
    }

    public void Start()
    {
        Enabled = true;
        enabledChanged = true;
        game.Request("dropRegistry", "configure", new { enabled = true });
    }

    public void Stop()
    {
        Enabled = false;
        enabledChanged = true;
        game.Request("dropRegistry", "configure", new { enabled = false });
    }

    internal void Remember()
    {
        foreach (var item in game.Request("dropRegistry", "registered")?.AsArray() ?? [])
        {
            if (item is JsonValue value && value.TryGetValue(out string? name))
                keptNames.Add(name.Trim());
            else if (item?["itemId"] is JsonValue id)
                keptIds.Add(id.GetValue<int>());
        }
        configBefore = game.Request("dropRegistry", "config");
    }

    internal void Restore(CancellationToken token)
    {
        Clear(token);
        if ((enabledChanged || rejectChanged) && configBefore is not null)
            game.Request("dropRegistry", "configure", new
            {
                enabled = enabledChanged ? configBefore["enabled"]?.GetValue<bool>() : null,
                rejectOthers = rejectChanged ? configBefore["rejectOthers"]?.GetValue<bool>() : null,
            }, token);
    }

    public ValueTask StopAsync()
    {
        Stop();
        return ValueTask.CompletedTask;
    }

    public void Pickup(string name) => game.Request("drops", "accept", new { query = name });

    public void Pickup(int id) => game.Request("drops", "accept", new { query = new { itemId = id } });

    public void Pickup(params string[] picked)
    {
        foreach (var name in picked)
            Pickup(name);
    }

    public void Pickup(params int[] picked)
    {
        foreach (var id in picked)
            Pickup(id);
    }

    public void RejectExcept(params string[] kept) => Reject(item => kept.Contains(item.Name.Trim(), StringComparer.OrdinalIgnoreCase));

    public void RejectExcept(params int[] kept) => Reject(item => kept.Contains(item.ID));

    public void RejectAll(bool skipWait = false) => Reject(_ => false);

    private void Reject(Func<ItemBase, bool> keep)
    {
        foreach (var item in CurrentDropInfos.Where(item => !keep(item)))
            game.Request("drops", "reject", new { query = new { itemId = item.ID } });
    }

    public void PickupAll(bool skipWait = false)
    {
        foreach (var item in CurrentDropInfos)
            Pickup(item.ID);
    }
}
