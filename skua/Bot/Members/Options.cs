using System.Collections.Immutable;
using System.Runtime.CompilerServices;
using Skua.Core.Interfaces;
using Skua.Core.Models;

namespace Mira.Skua.Bot.Members;

public sealed class Options(Game game, Paths paths, IFlashUtil flash) : IScriptOption
{
    private static readonly Dictionary<string, Func<object, (string Key, object? Value)>> Mira = new()
    {
        [nameof(AutoRelogin)] = value => ("autoRelogin:enabled", value),
        [nameof(ReloginServer)] = value => ("autoRelogin:server", value),
        [nameof(InfiniteRange)] = value => ("infiniteRange", value),
        [nameof(Magnetise)] = value => ("enemyMagnet", value),
        [nameof(AggroMonsters)] = value => ("provokeCell", value),
        [nameof(SkipCutscenes)] = value => ("skipCutscenes", value),
        [nameof(HidePlayers)] = value => ("hidePlayers", value),
        [nameof(LagKiller)] = value => ("rendering", value is true ? "interface-only" : "full"),
        [nameof(DisableFX)] = value => ("animations", value is not true),
        [nameof(DisableCollisions)] = value => ("collisions", value is not true),
        [nameof(CustomName)] = value => ("customName", value is string { Length: > 0 } name ? name : null),
        [nameof(CustomGuild)] = value => ("customGuild", value is string { Length: > 0 } guild ? guild : null),
        [nameof(WalkSpeed)] = value => ("walkSpeed", value),
        [nameof(SetFPS)] = value => ("frameRate", value),
    };

    private static readonly Dictionary<string, string> GamePaths = new()
    {
        [nameof(NameColor)] = "world.myAvatar.pMC.pname.ti.textColor",
        [nameof(GuildColor)] = "world.myAvatar.pMC.pname.tg.textColor",
    };

    private static readonly Dictionary<string, object> Defaults = new()
    {
        [nameof(SafeTimings)] = true, [nameof(SetFPS)] = 30, [nameof(RetryRelogin)] = true, [nameof(ReloginServer)] = "Twilly",
        [nameof(CustomName)] = string.Empty, [nameof(NameColor)] = 0xFFFFFF, [nameof(CustomGuild)] = string.Empty,
        [nameof(WalkSpeed)] = 8, [nameof(LoadTimeout)] = 30000, [nameof(HuntDelay)] = 1000, [nameof(HuntBuffer)] = 1,
        [nameof(MaximumTries)] = 10, [nameof(ActionDelay)] = 800, [nameof(JoinMapTries)] = 3, [nameof(QuestAcceptAndCompleteTries)] = 30,
        [nameof(ReloginTries)] = 5, [nameof(ReloginTryDelay)] = 800, [nameof(LoginTimeout)] = 30000,
    };

    private readonly Dictionary<string, object?> values = [];

    public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged { add { } remove { } }

    public ImmutableDictionary<string, Func<object>> OptionDictionary => typeof(IScriptOption).GetProperties()
        .Where(property => property.CanWrite)
        .ToImmutableDictionary(property => property.Name, property => (Func<object>)(() => property.GetValue(this)!));

    public int ReloginTryDelay { get => Get<int>(); set => Set(value); }
    public int LoginTimeout { get => Get<int>(); set => Set(value); }
    public bool AcceptACDrops { get => Get<bool>(); set => Set(value); }
    public bool AcceptAllDrops
    {
        get => Get<bool>();
        set
        {
            Set(value);
            if (value)
                RejectAllDrops = false;
        }
    }
    public bool RejectAllDrops
    {
        get => Get<bool>();
        set
        {
            Set(value);
            if (value)
                AcceptAllDrops = false;
        }
    }
    public bool AggroAllMonsters { get => Get<bool>(); set => Set(value); }
    public bool AggroMonsters { get => Get<bool>(); set => Set(value); }
    public bool AttackWithoutTarget { get => Get<bool>(); set => Set(value); }
    public bool AutoRelogin { get => Get<bool>(); set => Set(value); }
    public bool AutoReloginAny { get => Get<bool>(); set => Set(value); }
    public bool RetryRelogin { get => Get<bool>(); set => Set(value); }
    public string CustomGuild { get => Get<string>(); set => Set(value); }
    public string CustomName { get => Get<string>(); set => Set(value); }
    public bool DisableCollisions { get => Get<bool>(); set => Set(value); }
    public bool DisableDeathAds
    {
        get => Get<bool>();
        set
        {
            Set(value);
            flash.Call("disableDeathAd", value);
        }
    }
    public bool DisableFX { get => Get<bool>(); set => Set(value); }
    public int MaximumTries { get => Get<int>(); set => Set(value); }
    public int ActionDelay { get => Get<int>(); set => Set(value); }
    public int GuildColor { get => Get<int>(); set => Set(value); }
    public bool HidePlayers { get => Get<bool>(); set => Set(value); }
    public int HuntBuffer { get => Get<int>(); set => Set(value); }
    public int HuntDelay { get => Get<int>(); set => Set(value); }
    public bool InfiniteRange { get => Get<bool>(); set => Set(value); }
    public bool LagKiller { get => Get<bool>(); set => Set(value); }
    public int LoadTimeout { get => Get<int>(); set => Set(value); }
    public string? ReloginServer { get => Get<string>(); set => Set(value); }
    public bool Magnetise { get => Get<bool>(); set => Set(value); }
    public int NameColor { get => Get<int>(); set => Set(value); }
    public bool PrivateRooms { get => Get<bool>(); set => Set(value); }
    public bool RestPackets { get => Get<bool>(); set => Set(value); }
    public bool SafeRelogin { get => Get<bool>(); set => Set(value); }
    public bool SafeTimings { get => Get<bool>(); set => Set(value); }
    public int SetFPS { get => Get<int>(); set => Set(value); }
    public bool ShowFPS
    {
        get => paths.Get<bool>("ui.mcFPS.visible");
        set => paths.Set("ui.mcFPS.visible", value);
    }
    public bool SkipCutscenes { get => Get<bool>(); set => Set(value); }
    public int WalkSpeed { get => Get<int>(); set => Set(value); }
    public int PrivateNumber { get => Get<int>(); set => Set(value); }
    public int JoinMapTries { get => Get<int>(); set => Set(value); }
    public int QuestAcceptAndCompleteTries { get => Get<int>(); set => Set(value); }
    public int ReloginTries { get => Get<int>(); set => Set(value); }
    public HuntPriorities HuntPriority { get => Get<HuntPriorities>(); set => Set(value); }

    public void Reset() => values.Clear();

    public void ResetToDefault() => values.Clear();

    public void Save() { }

    private T Get<T>([CallerMemberName] string name = "") =>
        values.TryGetValue(name, out var value) ? (T)value! : Defaults.TryGetValue(name, out var preset) ? (T)preset : default!;

    private void Set<T>(T value, [CallerMemberName] string name = "")
    {
        values[name] = value;
        if (value is null)
            return;
        if (Mira.TryGetValue(name, out var setting))
        {
            var (key, mapped) = setting(value);
            var (domain, field) = key.Split(':') is [var named, var inner] ? (named, inner) : ("settings", key);
            game.Request(domain, "update", new Dictionary<string, object?> { [field] = mapped });
        }
        else if (GamePaths.TryGetValue(name, out var path))
        {
            paths.Set(path, value);
        }
    }
}
