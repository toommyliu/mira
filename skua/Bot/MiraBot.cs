using Skua.Core.Interfaces;
using Skua.Core.Interfaces.Services;
using Skua.Core.Models;
using Skua.Core.Scripts;
using Skua.Core.Skills;

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
        var flash = new Flash(game);
        var paths = new Members.Paths(flash);
        Flash = flash;
        Config = options;
        EventSource = new Events(() => this);
        Services = new Services(this, game, dialogs);
        Options = new Members.Options(game, paths, flash);
        Combat = new Members.Combat(game, flash, paths, () => Monsters!);
        Player = new Members.Player(game, paths, flash, () => Inventory!);
        Map = new Members.Map(game, paths, flash, () => Player);
        Monsters = new ScriptMonster(new(() => Flash), new(() => Map), new(() => Player));
        Inventory = new Members.Inventory(game, flash, paths, () => this);
        Bank = new Members.Bank(game, flash, paths, () => this);
        TempInv = new ScriptTempInv(new(() => Flash));
        House = new ScriptHouseInv(new(() => Flash), new(() => Send!), new(() => Options), new(() => Wait!), new(() => Map),
            new(() => status), new(() => Player), new(() => Inventory));
        InvHelper = new ScriptInventoryHelper(new(() => Inventory), new(() => House), new(() => TempInv), new(() => Bank));
        Reputation = new ScriptFaction(new(() => Flash));
        Lite = new ScriptLite(new(() => Flash));
        Quests = new Members.Quests(game, paths, () => this);
        Send = new Members.Send(game, paths, game.Stopping);
        Shops = new Members.Shops(game, paths, () => this, EventSource);
        Servers = new Members.Servers(game, paths, () => this);
        Drops = new Members.Drops(game);
        Boosts = new Members.Boosts(game, () => this);
        Self = new Members.SelfAuras(flash, () => Player);
        Target = new Members.TargetAuras(flash, () => Player);
        UltraBossHelper = new Members.UltraBossHelper(game, () => Combat);
        Accounts = new Members.Accounts(() => Player);
        Handlers = HandlerSource;
        Wait = new ScriptWait(new(() => Flash), new(() => Player), new(() => status), new(() => Map), new(() => Combat), new(() => Inventory),
            new(() => Monsters), new(() => House), new(() => Quests), new(() => Drops), new(() => Skills!));
        Kill = new ScriptKill(new(() => Wait), new(() => Options), new(() => Combat), new(() => Player), new(() => Drops),
            new(() => Inventory), new(() => TempInv), new(() => status), new(() => Map), Services.Logs, new(() => Monsters));
        Hunt = new Members.Hunt(game, () => this, new(() => Options), new(() => Combat), new(() => Player), new(() => Monsters), new(() => Drops),
            new(() => Inventory), new(() => TempInv), new(() => status), new(() => Map), new(() => Kill), Services.Logs);
        Skills = new ScriptSkill(AdvancedSkills.FromScripts(), new(() => Flash), new(() => Player), new(() => Self), new(() => Target),
            new(() => Combat), new(() => Options), new(() => Inventory), new(() => Wait));
        Auto = Unsupported.Area<IScriptAuto>("Auto");
        Stats = Unsupported.Area<IScriptBotStats>("Stats");
        AuraMonitor = Unsupported.Area<IAuraMonitorService>("AuraMonitor");
        GameProxy = Unsupported.Area<ICaptureProxy>("GameProxy");
        IScriptInterface.Instance = this;
    }

    internal void Detach()
    {
        if (IScriptInterface.Instance == this)
            IScriptInterface.Instance = null!;
    }

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
    public IAuraMonitorService AuraMonitor { get; }
    public IUltraBossHelper UltraBossHelper { get; }
    public IScriptCombat Combat { get; }
    public IScriptKill Kill { get; }
    public IScriptHunt Hunt { get; }
    public IScriptDrop Drops { get; }
    public IScriptEvent Events => EventSource;
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

    public void SellJunk() => throw Unsupported.Member("SellJunk");

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
