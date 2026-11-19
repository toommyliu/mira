using Skua.Core.Interfaces;
using Skua.Core.Models;

namespace Mira.Skua.Bot;

public sealed class MiraBot : IScriptInterface
{
    private readonly Game game;
    private readonly Status status;
    private readonly Dialogs dialogs;

    public MiraBot(Game game, Status status, ScriptOptions options, Dialogs dialogs)
    {
        this.game = game;
        this.status = status;
        this.dialogs = dialogs;
        EventSource = new Events(() => Player?.Username);
        Flash = new Flash(game);
        Paths = new Members.Paths(Flash);
        Bindings = new Bindings(Flash);
        Config = options;
        Options = Surface.Over<IScriptOption>("Options", Settings, Bindings);
        Events = Surface.Over<IScriptEvent>("Events", EventSource, Bindings);
        Handlers = Surface.Over<IScriptHandlers>("Handlers", HandlerSource, Bindings);
        Auto = Surface.Over<IScriptAuto>("Auto", new object(), Bindings);
        Boosts = Surface.Over<IScriptBoost>("Boosts", new Members.Boosts(game, () => this), Bindings);
        Stats = Surface.Over<IScriptBotStats>("Stats", new object(), Bindings);
        Self = Surface.Over<IScriptSelfAuras>("Self", new Members.SelfAuras(Flash, () => Player!), Bindings);
        Target = Surface.Over<IScriptTargetAuras>("Target", new Members.TargetAuras(Flash, () => Player!), Bindings);
        AuraMonitor = Surface.Over<global::Skua.Core.Interfaces.Services.IAuraMonitorService>("AuraMonitor", new object(), Bindings);
        UltraBossHelper = Surface.Over<IUltraBossHelper>("UltraBossHelper", new Members.UltraBossHelper(game, () => this), Bindings);
        Combat = Surface.Over<IScriptCombat>("Combat", new Members.Combat(game, Flash), Bindings);
        Kill = Surface.Over<IScriptKill>("Kill", new Members.Kill(() => this), Bindings);
        Hunt = Surface.Over<IScriptHunt>("Hunt", new Members.Hunt(game, () => this), Bindings);
        Drops = Surface.Over<IScriptDrop>("Drops", new Members.Drops(game), Bindings);
        Reputation = Surface.Over<IScriptFaction>("Reputation", new object(), Bindings);
        House = Surface.Over<IScriptHouseInv>("House", new Members.House(() => this), Bindings);
        Inventory = Surface.Over<IScriptInventory>("Inventory", new Members.Inventory(game, Flash, () => this), Bindings);
        TempInv = Surface.Over<IScriptTempInv>("TempInv", new object(), Bindings);
        Bank = Surface.Over<IScriptBank>("Bank", new Members.Bank(game, Flash, () => this), Bindings);
        InvHelper = Surface.Over<IScriptInventoryHelper>("InvHelper", new object(), Bindings);
        Lite = Surface.Over<IScriptLite>("Lite", new Bag("Lite"), Bindings);
        Map = Surface.Over<IScriptMap>("Map", new Members.Map(game, Paths, () => this), Bindings);
        Monsters = Surface.Over<IScriptMonster>("Monsters", new Members.Monsters(() => this), Bindings);
        Player = Surface.Over<IScriptPlayer>("Player", new Members.Player(game, Paths, () => Player!, () => Inventory!), Bindings);
        Quests = Surface.Over<IScriptQuest>("Quests", new Members.Quests(game, Paths, () => this), Bindings);
        Send = Surface.Over<IScriptSend>("Send", new Members.Send(game, Paths, game.Stopping), Bindings);
        Shops = Surface.Over<IScriptShop>("Shops", new Members.Shops(game, () => this, EventSource), Bindings);
        Skills = Surface.Over<IScriptSkill>("Skills", new Members.Skills(game, () => this, AdvancedSkills.FromScripts()), Bindings);
        Wait = Surface.Over<IScriptWait>("Wait", new Members.Wait(() => this, EventSource), Bindings);
        Servers = Surface.Over<IScriptServers>("Servers", new Members.Servers(game, () => this), Bindings);
        GameProxy = Surface.Over<ICaptureProxy>("GameProxy", new object(), Bindings);
        Accounts = Surface.Over<IScriptAccounts>("Accounts", new Members.Accounts(() => this), Bindings);
        Services = new Services(this, game, dialogs);
        IScriptInterface.Instance = this;
    }

    internal void Detach()
    {
        if (IScriptInterface.Instance == this)
            IScriptInterface.Instance = null!;
    }

    internal Members.Paths Paths { get; }
    internal Bindings Bindings { get; }
    internal Settings Settings => settings ??= new Settings(Bindings, game);
    private Settings? settings;
    internal Events EventSource { get; }
    internal Handlers HandlerSource { get; } = new();
    internal Services Services { get; }

    public IFlashUtil Flash { get; }
    public IScriptStatus Manager => status;
    public IScriptAuto Auto { get; }
    public IScriptBoost Boosts { get; }
    public IScriptBotStats Stats { get; }
    public IScriptSelfAuras Self { get; }
    public IScriptTargetAuras Target { get; }
    public global::Skua.Core.Interfaces.Services.IAuraMonitorService AuraMonitor { get; }
    public IUltraBossHelper UltraBossHelper { get; }
    public IScriptCombat Combat { get; }
    public IScriptKill Kill { get; }
    public IScriptHunt Hunt { get; }
    public IScriptDrop Drops { get; }
    public IScriptEvent Events { get; }
    public IScriptFaction Reputation { get; }
    public IScriptHouseInv House { get; }
    public IScriptInventory Inventory { get; }
    public IScriptTempInv TempInv { get; }
    public IScriptBank Bank { get; }
    public IScriptInventoryHelper InvHelper { get; }
    public IScriptLite Lite { get; }
    public IScriptOption Options { get; }
    public IScriptMap Map { get; }
    public IScriptMonster Monsters { get; }
    public IScriptPlayer Player { get; }
    public IScriptQuest Quests { get; }
    public IScriptSend Send { get; }
    public IScriptShop Shops { get; }
    public IScriptSkill Skills { get; }
    public IScriptWait Wait { get; }
    public IScriptServers Servers { get; }
    public IScriptHandlers Handlers { get; }
    public ICaptureProxy GameProxy { get; }
    public IScriptAccounts Accounts { get; }
    public IScriptOptionContainer? Config { get; }
    public bool ShouldExit => status.ShouldExit;
    public Version Version => null!;
    public Random Random { get; set; } = new();

    public void SellJunk() => throw new NotSupportedException("Mira doesn't support Bot.SellJunk yet.");

    public void Sleep(int ms)
    {
        if (game.Stopping.WaitHandle.WaitOne(Math.Max(ms, 0)))
            throw new OperationCanceledException(game.Stopping);
    }

    public Task SleepAsync(int ms) => Task.Delay(Math.Max(ms, 0), game.Stopping);

    public void Stop(bool runScriptStoppingEvent = true) => status.StopScript(runScriptStoppingEvent);

    public Task StopAsync(bool runScriptStoppingEvent = true) => status.StopScript(runScriptStoppingEvent).AsTask();

    public void Log(string message) => Services.Logs.ScriptLog(message);

    public Task Schedule(int delay, Action<IScriptInterface> action) =>
        Task.Run(async () =>
        {
            await Task.Delay(delay, game.Stopping);
            action(this);
        }, game.Stopping);

    public Task Schedule(int delay, Func<IScriptInterface, Task> function) =>
        Task.Run(async () =>
        {
            await Task.Delay(delay, game.Stopping);
            await function(this);
        }, game.Stopping);

    public bool? ShowMessageBox(string message, string caption, bool yesAndNo = false)
    {
        if (yesAndNo)
            return dialogs.Confirm(message, caption, "Yes", "No");
        dialogs.Alert(message, caption);
        return null;
    }

    public DialogResult ShowMessageBox(string message, string caption, params string[] buttons) =>
        dialogs.Choose(message, caption, buttons);
}
