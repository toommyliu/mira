namespace Mira.Skua.Bot;

public sealed class Settings(Bindings bindings, Game game) : Bag("Options")
{
    private static readonly Dictionary<string, Func<object, (string Key, object? Value)>> Mira = new()
    {
        ["AutoRelogin"] = value => ("autoRelogin:enabled", value),
        ["ReloginServer"] = value => ("autoRelogin:server", value),
        ["InfiniteRange"] = value => ("infiniteRange", value),
        ["Magnetise"] = value => ("enemyMagnet", value),
        ["AggroMonsters"] = value => ("provokeCell", value),
        ["SkipCutscenes"] = value => ("skipCutscenes", value),
        ["HidePlayers"] = value => ("hidePlayers", value),
        ["LagKiller"] = value => ("rendering", value is true ? "interface-only" : "full"),
        ["DisableFX"] = value => ("animations", value is not true),
        ["DisableCollisions"] = value => ("collisions", value is not true),
        ["CustomName"] = value => ("customName", value is string { Length: > 0 } name ? name : null),
        ["CustomGuild"] = value => ("customGuild", value is string { Length: > 0 } guild ? guild : null),
        ["WalkSpeed"] = value => ("walkSpeed", value),
        ["SetFPS"] = value => ("frameRate", value),
    };

    private static readonly Dictionary<string, object> Defaults = new()
    {
        ["SafeTimings"] = true, ["SetFPS"] = 30, ["RetryRelogin"] = true, ["ReloginServer"] = "Twilly",
        ["CustomName"] = string.Empty, ["NameColor"] = 0xFFFFFF, ["CustomGuild"] = string.Empty,
        ["WalkSpeed"] = 8, ["LoadTimeout"] = 30000, ["HuntDelay"] = 1000, ["HuntBuffer"] = 1,
        ["MaximumTries"] = 10, ["ActionDelay"] = 800, ["JoinMapTries"] = 3, ["QuestAcceptAndCompleteTries"] = 30,
        ["ReloginTries"] = 5, ["ReloginTryDelay"] = 800, ["LoginTimeout"] = 30000,
    };

    protected override object? Preset(string name) => Defaults.GetValueOrDefault(name);

    protected override void Changed(string name, object value)
    {
        if (Mira.TryGetValue(name, out var setting))
        {
            var (key, mapped) = setting(value);
            var (domain, field) = key.Split(':') is [var named, var inner] ? (named, inner) : ("settings", key);
            game.Request(domain, "update", new Dictionary<string, object?> { [field] = mapped });
        }
        else
        {
            bindings.Write("Options", name, value);
        }
    }
}
