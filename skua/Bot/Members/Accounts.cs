using Skua.Core.Interfaces;

namespace Mira.Skua.Bot.Members;

public sealed class Accounts(Func<IScriptPlayer> player) : IScriptAccounts
{
    private readonly Dictionary<string, HashSet<string>> tags = new(StringComparer.OrdinalIgnoreCase);

    private HashSet<string> For(string? username)
    {
        var key = username ?? string.Empty;
        if (!tags.TryGetValue(key, out var set))
            tags[key] = set = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        return set;
    }

    private string Me => player().Username ?? string.Empty;

    public List<string> GetTags() => GetTags(Me);
    public List<string> GetTags(string username) => [.. For(username)];
    public bool HasTag(string tag) => HasTag(Me, tag);
    public bool HasTag(string username, string tag) => For(username).Contains(tag);
    public bool AddTag(string tag) => AddTag(Me, tag);
    public bool AddTag(string username, string tag) => For(username).Add(tag);
    public bool RemoveTag(string tag) => RemoveTag(Me, tag);
    public bool RemoveTag(string username, string tag) => For(username).Remove(tag);
    public void AddTags(params string[] added) => AddTags(Me, added);
    public bool AddTags(string username, params string[] added) => added.Select(tag => For(username).Add(tag)).ToList().Any(changed => changed);
    public void RemoveTags(params string[] removed) => RemoveTags(Me, removed);
    public bool RemoveTags(string username, params string[] removed) => removed.Select(tag => For(username).Remove(tag)).ToList().Any(changed => changed);
    public void SetTags(params string[] set) => SetTags(Me, set);
    public bool SetTags(string username, params string[] set)
    {
        For(username).Clear();
        For(username).UnionWith(set);
        return true;
    }
    public void ClearTags() => ClearTags(Me);
    public bool ClearTags(string username) => tags.Remove(username ?? string.Empty);
    public List<ManagedAccount> GetAllAccounts() => [];
}
