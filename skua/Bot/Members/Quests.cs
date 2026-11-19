using Skua.Core.Interfaces;
using Skua.Core.Models.Quests;

namespace Mira.Skua.Bot.Members;

public sealed class Quests(Game game, Paths paths, Func<IScriptInterface> bot)
{
    private readonly Dictionary<int, int> registered = [];

    private IScriptInterface Bot => bot();

    public int RegisterCompleteInterval { get; set; } = 2000;
    public List<Quest> Tree => paths.Get<Dictionary<int, Quest>>("world.questTree", []).Values.ToList();
    public List<Quest> Active => Tree.FindAll(quest => quest.Active);
    public List<Quest> Completed => Tree.FindAll(quest => quest.Status == "c");
    public Dictionary<int, QuestData> CachedDictionary { get; set; } = [];
    public List<QuestData> Cached
    {
        get => [.. CachedDictionary.Values];
        set => CachedDictionary = value?.ToDictionary(quest => quest.ID) ?? [];
    }
    public IEnumerable<int> Registered => registered.Keys;
    public IReadOnlyDictionary<int, int> RegisteredRewards => registered;

    public void RegisterQuests(params int[] ids) => RegisterQuests(ids.Select(id => (id, -1)).ToArray());

    public void RegisterQuests(params (int questId, int rewardId)[] quests)
    {
        foreach (var (quest, reward) in quests)
            registered[quest] = reward;
        game.Request("questRegistry", "register", new
        {
            quests = quests.Select(entry => new { questId = entry.questId, rewardItemId = entry.rewardId > 0 ? entry.rewardId : (int?)null }),
        });
        game.Request("questRegistry", "configure", new { enabled = true });
    }

    public void UnregisterQuests(params int[] ids)
    {
        foreach (var id in ids)
            registered.Remove(id);
        game.Request("questRegistry", "unregister", new { questIds = ids });
    }

    public void UnregisterAllQuests()
    {
        registered.Clear();
        game.Request("questRegistry", "unregisterAll");
    }

    public void Load(params int[] ids) => game.Request("quests", "loadBatch", new { questIds = ids });

    public Quest? EnsureLoad(int id)
    {
        game.Request("quests", "load", new { questId = id });
        return Tree.Find(quest => quest.ID == id);
    }

    public bool TryGetQuest(int id, out Quest? quest) => (quest = Tree.Find(entry => entry.ID == id)) is not null;

    public bool Accept(int id) => game.Request("quests", "accept", new { questId = id })?.GetValue<bool>() == true || IsInProgress(id);

    public void Accept(params int[] ids)
    {
        foreach (var id in ids)
        {
            Accept(id);
            Bot.Sleep(Bot.Options.ActionDelay);
        }
    }

    public bool EnsureAccept(int id) => Retry(() => Accept(id), () => IsInProgress(id));

    public void EnsureAccept(params int[] ids)
    {
        foreach (var id in ids)
            EnsureAccept(id);
    }

    public bool CanComplete(int id) => game.Request("quests", "canComplete", new { questId = id })?.GetValue<bool>() == true;

    public bool CanCompleteFullCheck(int id) => CanComplete(id);

    public bool Complete(int id, int itemId = -1, bool special = false) =>
        game.Request("quests", "complete", new { questId = id, rewardItemId = itemId > 0 ? itemId : (int?)null })?.GetValue<bool>() == true
        || !IsInProgress(id);

    public void Complete(params int[] ids)
    {
        foreach (var id in ids)
        {
            Complete(id);
            Bot.Sleep(Bot.Options.ActionDelay);
        }
    }

    public bool EnsureComplete(int id, int itemId = -1, bool special = false) =>
        id != 0 && Retry(() => Complete(id, itemId, special), () => !IsInProgress(id));

    public void EnsureComplete(params int[] ids)
    {
        foreach (var id in ids)
            EnsureComplete(id);
    }

    public bool IsInProgress(int id) => game.Request("quests", "inProgress", new { questId = id })?.GetValue<bool>() == true;

    public bool IsAvailable(int id) => game.Request("quests", "available", new { questId = id })?.GetValue<bool>() == true;

    public bool IsDailyComplete(int id) => EnsureLoad(id) is { } quest && IsDailyComplete(quest);

    public bool IsDailyComplete(Quest quest) => paths.Call<int>("world.getAchievement", quest.Field, quest.Index) > 0;

    public bool IsUnlocked(int id) => EnsureLoad(id) is { } quest && IsUnlocked(quest);

    public bool IsUnlocked(Quest quest) => quest.Slot < 0 || paths.Call<int>("world.getQuestValue", quest.Slot) >= quest.Value - 1;

    public bool HasBeenCompleted(int id) => EnsureLoad(id) is { } quest && HasBeenCompleted(quest);

    public bool HasBeenCompleted(Quest quest) => quest.Slot < 0 || paths.Call<int>("world.getQuestValue", quest.Slot) >= quest.Value;

    public bool UpdateQuest(int id)
    {
        if (EnsureLoad(id) is not { } quest)
            return false;
        UpdateQuest(quest.Value, quest.Slot);
        return true;
    }

    public void UpdateQuest(int value, int slot) =>
        Bot.Send.ClientPacket("{\"t\":\"xt\",\"b\":{\"r\":-1,\"o\":{\"cmd\":\"updateQuest\",\"iValue\":" + value + ",\"iIndex\":" + slot + "}}}", "json");

    private bool Retry(Action attempt, Func<bool> done)
    {
        for (var tries = 0; tries < Bot.Options.QuestAcceptAndCompleteTries && !Bot.ShouldExit; tries++)
        {
            attempt();
            if (done())
                return true;
            Bot.Sleep(Bot.Options.ActionDelay);
        }
        return done();
    }
}
