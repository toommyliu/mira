using System.Diagnostics;
using Skua.Core.Interfaces;
using Skua.Core.Models.Skills;
using Skua.Core.Skills;

namespace Mira.Skua.Bot.Members;

public sealed class Skills(Game game, Func<IScriptInterface> bot, AdvancedSkills library)
{
    private const string GenericSkills = "0 | 1 | 2 | 3 | 4";
    private const int StopAttackingFallbackMs = 10000;

    private readonly ManualResetEventSlim unpaused = new(true);
    private ISkillProvider? provider;
    private ISkillProvider? baseProvider;
    private CancellationTokenSource? timer;
    private int skillTimeoutMs = -1;
    private int? skillTimeoutIntervals;
    private int lastRank = -1;
    private SkillInfo[]? playerSkills;
    private string? lastFailure;

    private IScriptInterface Bot => bot();

    public ISkillProvider BaseProvider => baseProvider ??= Provider(GenericSkills);
    public ISkillProvider? OverrideProvider { get; set; }
    public bool TimerRunning { get; private set; }
    public bool IsPaused { get; private set; }
    public int SkillInterval { get; set; } = 100;
    public bool ResetComboOnTargetChange { get; set; }
    public SkillUseMode SkillUseMode { get; set; } = SkillUseMode.UseIfAvailable;

    public int SkillTimeoutMs
    {
        get => skillTimeoutIntervals is int intervals
            ? (intervals <= 0 ? intervals : (int)Math.Min(int.MaxValue, (long)intervals * Math.Max(1, SkillInterval)))
            : skillTimeoutMs;
        set
        {
            skillTimeoutMs = value;
            skillTimeoutIntervals = null;
        }
    }

    public int SkillTimeout
    {
        get => skillTimeoutIntervals
            ?? (skillTimeoutMs <= 0 ? skillTimeoutMs : (int)(((long)skillTimeoutMs + Math.Max(1, SkillInterval) - 1) / Math.Max(1, SkillInterval)));
        set => skillTimeoutIntervals = value;
    }

    public bool CanUseSkill(int index) => game.Request("combat", "canUseSkill", new { skill = index })?.GetValue<bool>() ?? false;

    public int SkillCooldownRemaining(int index) => (int)(game.Request("combat", "skillCooldown", new { skill = index })?.GetValue<double>() ?? 0);

    public int GlobalCooldownRemaining() => Bot.Flash.Call<int>("globalCooldownRemaining");

    public bool UseSkill(int index) => game.Request("combat", "useSkill", new { skill = index })?.GetValue<bool>() ?? false;

    public void Start()
    {
        provider = OverrideProvider ?? BaseProvider;
        if (TimerRunning)
            return;
        var stopping = timer = CancellationTokenSource.CreateLinkedTokenSource(game.Stopping);
        TimerRunning = true;
        new Thread(() =>
        {
            try
            {
                Run(stopping.Token);
            }
            catch (Exception error)
            {
                Faults.Report(error);
            }
            finally
            {
                stopping.Dispose();
                TimerRunning = false;
            }
        }) { Name = "Skill Timer", IsBackground = true }.Start();
    }

    public void Stop()
    {
        provider?.Stop();
        try
        {
            timer?.Cancel();
        }
        catch (ObjectDisposedException)
        {
        }
        Bot.Wait.ForTrue(() => !TimerRunning, 2);
    }

    public void Pause()
    {
        if (!TimerRunning || IsPaused)
            return;
        IsPaused = true;
        unpaused.Reset();
    }

    public void Resume()
    {
        if (!TimerRunning || !IsPaused)
            return;
        IsPaused = false;
        unpaused.Set();
    }

    public void SetProvider(ISkillProvider skills) => provider = OverrideProvider = skills;

    public void UseBaseProvider() => provider = BaseProvider;

    public void LoadAdvanced(string className, bool autoEquip, ClassUseMode useMode = ClassUseMode.Base)
    {
        if (className == "generic")
        {
            LoadGeneric();
            return;
        }
        if (autoEquip)
            Equip(className);
        var skills = library.Loaded.Where(skill => string.Equals(skill.ClassName, className, StringComparison.CurrentCultureIgnoreCase)).ToList();
        if (skills.Count == 0)
        {
            LoadGeneric();
            return;
        }
        Load(skills.Find(skill => skill.ClassUseMode == useMode) ?? skills[0]);
    }

    public void LoadAdvanced(string skills, int skillTimeout = -1, SkillUseMode skillMode = SkillUseMode.UseIfAvailable)
    {
        ResetComboOnTargetChange = false;
        OverrideProvider = Provider(skills);
        SkillTimeout = skillTimeout;
        SkillUseMode = skillMode;
    }

    public void LoadAdvanced(string className, string mode, bool autoEquip = true)
    {
        if (Enum.TryParse(mode, ignoreCase: true, out ClassUseMode classMode))
        {
            LoadAdvanced(className, autoEquip, classMode);
            return;
        }
        if (library.Find(className, mode) is not { } skill)
        {
            LoadGeneric();
            return;
        }
        if (autoEquip)
            Equip(className);
        Load(skill);
    }

    private void Equip(string className)
    {
        Bot.Inventory.EquipItem(className);
        Bot.Wait.ForItemEquip(className);
    }

    private void Load(AdvancedSkill skill)
    {
        ResetComboOnTargetChange = skill.ResetComboOnTargetChange;
        OverrideProvider = Provider(skill.Skills);
        SkillTimeoutMs = skill.SkillTimeoutMs;
        SkillUseMode = skill.SkillUseMode;
    }

    private void LoadGeneric()
    {
        ResetComboOnTargetChange = false;
        OverrideProvider = Provider(GenericSkills);
        SkillUseMode = SkillUseMode.UseIfAvailable;
    }

    private AdvancedSkillProvider Provider(string skills)
    {
        var loaded = new AdvancedSkillProvider(Bot.Player, Bot.Self, Bot.Target, Bot.Combat, Bot.Flash) { ResetOnTarget = ResetComboOnTargetChange };
        loaded.Load(skills);
        return loaded;
    }

    private void Run(CancellationToken token)
    {
        while (!token.IsCancellationRequested)
        {
            unpaused.Wait(token);
            var delay = SkillInterval;
            try
            {
                delay = Turn(token);
                lastFailure = null;
            }
            catch (Exception error) when (error is not OperationCanceledException)
            {
                var failure = $"{error.GetType().Name}: {error.Message}";
                if (failure != lastFailure)
                    game.Connection.Log("debug", $"Skill timer: {failure}");
                lastFailure = failure;
            }
            token.WaitHandle.WaitOne(Math.Max(1, delay));
        }
    }

    private int Turn(CancellationToken token)
    {
        if (Bot.Combat.StopAttacking)
        {
            WaitWhileStopped(token);
            Bot.Combat.StopAttacking = false;
        }
        var player = Bot.Player;
        if (!player.Alive)
            provider?.OnPlayerDeath();
        if (provider?.OnTargetReset() == true)
            return 0;
        if (!((Bot.Options.AttackWithoutTarget && player is { Loaded: true, Playing: true }) || player is { HasTarget: true, Loaded: true, Playing: true }))
            return SkillInterval;
        var globalCooldown = GlobalCooldownRemaining();
        var delay = SkillInterval;
        if (globalCooldown <= 0)
        {
            delay = Poll(token);
            globalCooldown = GlobalCooldownRemaining();
        }
        return globalCooldown > 0 ? Math.Min(globalCooldown, SkillInterval) : delay;
    }

    private int Poll(CancellationToken token)
    {
        var rank = Bot.Player.CurrentClassRank;
        if (rank > lastRank || playerSkills is null)
        {
            if (Bot.Player.Skills is { Length: > 0 } skills)
                playerSkills = skills;
        }
        lastRank = rank;
        var tryEachStep = SkillUseMode == SkillUseMode.UseIfAvailable;
        var stepCount = Math.Max(1, provider!.SkillCount);
        var steps = tryEachStep ? stepCount : 1;
        var soonest = int.MaxValue;
        for (int step = 0; step < steps && !token.IsCancellationRequested; step++)
        {
            if (step > 0 && provider.OnTargetReset())
                return 1;
            var (index, skill) = provider.GetNextSkill();
            if (index == -1 || skill == -1)
                return SkillInterval;
            var canUse = Usable(skill);
            if (tryEachStep && !canUse)
            {
                var left = SkillCooldownRemaining(skill);
                if (left > 0)
                    soonest = Math.Min(soonest, left);
                continue;
            }
            switch (provider.ShouldUseSkill(index, canUse))
            {
                case true:
                    if (playerSkills is not null && skill < playerSkills.Length && !playerSkills[skill].IsOk)
                        break;
                    Use(skill, token);
                    return SkillInterval;
                case null:
                    if (steps < stepCount)
                        steps++;
                    break;
            }
        }
        return soonest == int.MaxValue ? SkillInterval : Math.Min(soonest, SkillInterval);
    }

    private void Use(int skill, CancellationToken token)
    {
        if (SkillUseMode == SkillUseMode.UseIfAvailable)
        {
            if (Usable(skill) && !Bot.Combat.StopAttacking)
                UseSkill(skill);
            return;
        }
        if (WaitUntilUsable(skill, token, out var targetReset) && !targetReset && !Bot.Combat.StopAttacking)
            UseSkill(skill);
    }

    private bool Usable(int skill) => Bot.Options.AttackWithoutTarget ? Bot.Flash.Call<bool>("canUseSkill", skill, true) : CanUseSkill(skill);

    private bool WaitUntilUsable(int skill, CancellationToken token, out bool targetReset)
    {
        targetReset = false;
        var deadline = SkillTimeoutMs > 0 ? Environment.TickCount64 + SkillTimeoutMs : long.MaxValue;
        while (!token.IsCancellationRequested)
        {
            if (provider?.OnTargetReset() == true)
            {
                targetReset = true;
                return false;
            }
            if (Usable(skill))
                return true;
            var cooldown = SkillCooldownRemaining(skill);
            if (cooldown <= 0)
                return Usable(skill);
            var left = deadline - Environment.TickCount64;
            if (left <= 0)
                return false;
            token.WaitHandle.WaitOne((int)Math.Max(1, Math.Min(Math.Min(cooldown, SkillInterval), left)));
        }
        return false;
    }

    private void WaitWhileStopped(CancellationToken token)
    {
        var waited = Stopwatch.StartNew();
        while (!token.IsCancellationRequested && Bot.Combat.StopAttacking && waited.ElapsedMilliseconds < StopAttackingFallbackMs)
            token.WaitHandle.WaitOne(50);
    }
}
