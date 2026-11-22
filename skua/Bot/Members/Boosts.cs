using System.ComponentModel;
using System.Text.Json.Nodes;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class Boosts(Game game, Func<IScriptInterface> bot) : IScriptBoost
{
    private static readonly Dictionary<BoostType, (string Kind, string Link)> Types = new()
    {
        [BoostType.Gold] = ("gold", "gboost"),
        [BoostType.Class] = ("classPoints", "cpboost"),
        [BoostType.Reputation] = ("rep", "repboost"),
        [BoostType.Experience] = ("xp", "xpboost"),
    };

    private readonly object gate = new();
    private readonly Dictionary<BoostType, (bool Use, int Id)> chosen = Types.Keys.ToDictionary(type => type, _ => (false, 0));
    private readonly HashSet<int> registered = [];
    private readonly HashSet<int> kept = [];
    private readonly HashSet<int> owned = [];
    private bool started;
    private JsonNode? configBefore;
    private bool enabledChanged;

    public event PropertyChangedEventHandler? PropertyChanged { add { } remove { } }

    public bool Enabled
    {
        get
        {
            lock (gate) return started;
        }
    }

    public bool UseGoldBoost { get => Get(BoostType.Gold).Use; set => Set(BoostType.Gold, use: value); }
    public bool UseClassBoost { get => Get(BoostType.Class).Use; set => Set(BoostType.Class, use: value); }
    public bool UseReputationBoost { get => Get(BoostType.Reputation).Use; set => Set(BoostType.Reputation, use: value); }
    public bool UseExperienceBoost { get => Get(BoostType.Experience).Use; set => Set(BoostType.Experience, use: value); }
    public int GoldBoostID { get => Get(BoostType.Gold).Id; set => Set(BoostType.Gold, id: value); }
    public int ClassBoostID { get => Get(BoostType.Class).Id; set => Set(BoostType.Class, id: value); }
    public int ReputationBoostID { get => Get(BoostType.Reputation).Id; set => Set(BoostType.Reputation, id: value); }
    public int ExperienceBoostID { get => Get(BoostType.Experience).Id; set => Set(BoostType.Experience, id: value); }

    public bool IsBoostActive(BoostType boost) =>
        game.Request("player", "hasBoost", new { kind = Types[boost].Kind })?.GetValue<bool>() == true;

    public void UseBoost(int id) => game.Request("inventory", "use", new { query = new { itemId = id } });

    public int GetBoostID(BoostType boostType, bool searchBank = true)
    {
        if (!Types.TryGetValue(boostType, out var type) || !bot().Player.LoggedIn)
            return 0;
        bool Matches(InventoryItem item) => item.Category == ItemCategory.ServerUse
            && string.Equals(item.FileName?.Split("::")[0].Trim(), type.Link, StringComparison.OrdinalIgnoreCase);
        var id = bot().Inventory.Items?.FirstOrDefault(Matches)?.ID ?? 0;
        if (id != 0 || !searchBank)
            return id;
        var bank = bot().Bank;
        if (!bank.Loaded)
            bank.Load();
        id = bank.Items?.FirstOrDefault(Matches)?.ID ?? 0;
        if (id > 0)
            bank.EnsureToInventory(id, false);
        return id;
    }

    public void Start()
    {
        lock (gate)
        {
            enabledChanged = true;
            game.Request("boostRegistry", "configure", new { enabled = true });
        }
        Run(true);
    }

    public void Stop() => Run(false);

    public ValueTask StopAsync()
    {
        Run(false);
        return ValueTask.CompletedTask;
    }

    private (bool Use, int Id) Get(BoostType type)
    {
        lock (gate) return chosen[type];
    }

    private void Set(BoostType type, bool? use = null, int? id = null)
    {
        lock (gate)
        {
            var (oldUse, oldId) = chosen[type];
            chosen[type] = (use ?? oldUse, id ?? oldId);
            Sync();
        }
    }

    private void Run(bool start)
    {
        lock (gate)
        {
            started = start;
            Sync();
        }
    }

    private void Sync()
    {
        var wanted = started ? chosen.Values.Where(boost => boost.Use && boost.Id > 0 && !kept.Contains(boost.Id)).Select(boost => boost.Id).ToHashSet() : [];
        owned.UnionWith(wanted);
        var dropped = registered.Except(wanted).Select(id => new { itemId = id }).ToArray();
        var added = wanted.Except(registered).Select(id => new { itemId = id }).ToArray();
        if (dropped.Length > 0)
            game.Request("boostRegistry", "unregister", new { items = dropped });
        if (added.Length > 0)
            game.Request("boostRegistry", "register", new { items = added });
        registered.Clear();
        registered.UnionWith(wanted);
    }

    internal void Remember()
    {
        lock (gate)
        {
            foreach (var item in game.Request("boostRegistry", "registered")?.AsArray() ?? [])
                if (item is JsonObject entry && entry["itemId"] is JsonValue id)
                    kept.Add(id.GetValue<int>());
            configBefore = game.Request("boostRegistry", "config");
        }
    }

    internal void Restore(CancellationToken token)
    {
        lock (gate)
        {
            if (owned.Count > 0)
                game.Request("boostRegistry", "unregister", new { items = owned.Select(id => new { itemId = id }).ToArray() }, token);
            owned.Clear();
            registered.Clear();
            if (enabledChanged && configBefore?["enabled"] is JsonValue enabled)
                game.Request("boostRegistry", "configure", new { enabled = enabled.GetValue<bool>() }, token);
        }
    }
}
