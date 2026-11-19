using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using Mira.Skua.Bot;
using Skua.Core.Models.Skills;
using Skua.Core.Skills;

namespace Mira.Skua.Tests;

sealed class Fight
{
    public volatile bool TargetAlive = true;

    public int SkillListFailures;

    public ConcurrentDictionary<int, bool> Unusable { get; } = new();

    public ConcurrentQueue<int> Used { get; } = new();

    public JsonNode? Answer(string key, JsonObject request) => key switch
    {
        "skua.isLoggedIn:" => true,
        "flash.isNull:world.myAvatar" => false,
        "flash.getGameObject:world.myAvatar.items.length" => "3",
        "flash.getGameObject:world.mapLoadInProgress" => "false",
        "flash.callGameFunction0:world.myAvatar.pMC.artLoaded" => "true",
        "flash.getGameObject:world.myAvatar.dataLeaf.intState" => "1",
        "skua.getTargetMonster:" => TargetAlive ? Frogzard : "null",
        "flash.getGameObject:world.actions.active" when Interlocked.Decrement(ref SkillListFailures) >= 0 => throw new InvalidOperationException("Mira refused the read."),
        "skua.availableMonsters:" => $"[{Frogzard}]",
        "skua.globalCooldownRemaining:" => 0,
        "combat.attack" => true,
        "combat.canUseSkill" => !Unusable.ContainsKey(Skill(request)),
        "combat.skillCooldown" => 0,
        "combat.useSkill" => Use(Skill(request)),
        _ => null,
    };

    private const string Frogzard = """{"MonMapID":1,"MonID":2,"intHP":100,"intState":1,"strMonName":"Frogzard"}""";

    private static int Skill(JsonObject request) => request["skill"]!.GetValue<int>();

    private bool Use(int skill)
    {
        Used.Enqueue(skill);
        return true;
    }
}

public class SkillTests
{
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(5);

    [Fact]
    public void A_rotation_uses_its_skills_in_turn_until_stopped()
    {
        var fight = new Fight();
        var bot = new Offline(fight.Answer).Bot();

        bot.Skills.StartAdvanced("1 | 2 | 3", -1, SkillUseMode.UseIfAvailable);
        Assert.True(SpinWait.SpinUntil(() => fight.Used.Count >= 6, Patience));
        bot.Skills.Stop();

        Assert.Equal([1, 2, 3, 1, 2, 3], fight.Used.Take(6));
        Assert.False(bot.Skills.TimerRunning);
        var used = fight.Used.Count;
        Thread.Sleep(300);
        Assert.Equal(used, fight.Used.Count);
    }

    [Fact]
    public void A_failed_read_does_not_stop_the_timer()
    {
        var fight = new Fight { SkillListFailures = 1 };
        var bot = new Offline(fight.Answer).Bot();

        bot.Skills.StartAdvanced("1 | 2", -1, SkillUseMode.UseIfAvailable);
        Assert.True(SpinWait.SpinUntil(() => fight.Used.Count >= 2, Patience));
        bot.Skills.Stop();

        Assert.Equal([1, 2], fight.Used.Take(2));
    }

    [Fact]
    public void Use_if_available_passes_over_a_skill_that_cannot_be_used()
    {
        var fight = new Fight();
        fight.Unusable[2] = true;
        var bot = new Offline(fight.Answer).Bot();

        bot.Skills.StartAdvanced("1 | 2 | 3", -1, SkillUseMode.UseIfAvailable);
        Assert.True(SpinWait.SpinUntil(() => fight.Used.Count >= 4, Patience));
        bot.Skills.Stop();

        Assert.Equal([1, 3, 1, 3], fight.Used.Take(4));
    }

    [Fact]
    public void A_kill_attacks_and_leaves_the_skills_to_the_timer()
    {
        var fight = new Fight();
        var game = new Offline(fight.Answer);
        var bot = game.Bot();
        bot.Skills.StartAdvanced("1 | 2", -1, SkillUseMode.UseIfAvailable);

        var kill = Task.Run(() => bot.Kill.Monster("Frogzard"));
        Assert.True(SpinWait.SpinUntil(() => fight.Used.Count >= 2, Patience));
        fight.TargetAlive = false;
        Assert.True(kill.Wait(Patience));
        bot.Skills.Stop();

        Assert.Contains(game.Calls, call => call["op"]?.ToString() == "attack");
        Assert.DoesNotContain(game.Calls, call => call["op"]?.ToString() == "kill");
    }

    [Fact]
    public void Class_rotations_read_as_Skua_writes_them()
    {
        var path = Path.Combine(Path.GetTempPath(), $"mira-skills-{Guid.NewGuid():N}.json");
        File.WriteAllText(path, """
            {"ArchMage":{"Base":{"skillUseMode":"WaitForCooldown","skillTimeout":100,"skills":[
              {"skillId":1,"rules":[{"type":"Aura","value":0,"comparison":"less","auraName":"incinerate","auraTarget":"target"}]},
              {"skillId":4,"rules":[{"type":"Health","value":70,"comparison":"greater","isPercentage":true},{"type":"Mana","value":70,"comparison":"less","isPercentage":true}]},
              {"skillId":2,"skipOnMatch":true,"rules":[{"type":"Wait","timeout":250}]},
              {"skillId":5,"isMultiAura":true,"multiAuraOperator":"OR","rules":[
                {"type":"MultiAura","value":3,"comparison":"greater","auraName":"Fire"},
                {"type":"MultiAura","value":1,"comparison":"less","auraName":"Ice","auraTarget":"target"}]},
              {"skillId":3}]}}}
            """);
        try
        {
            var skill = Assert.Single(new AdvancedSkills(path).Loaded);

            Assert.Equal("ArchMage", skill.ClassName);
            Assert.Equal(ClassUseMode.Base, skill.ClassUseMode);
            Assert.Equal(SkillUseMode.WaitForCooldown, skill.SkillUseMode);
            Assert.Equal("""1 A<"incinerate" 0 TARGET | 4 H>70% M<70% | 2 WW250S | 5 MA>"Fire" 3: MA<"Ice" 1 TARGET | 3""", skill.Skills);
        }
        finally
        {
            File.Delete(path);
        }
    }

    [Fact]
    public void Every_class_rotation_in_the_scripts_loads()
    {
        var home = Environment.GetEnvironmentVariable("MIRA_SKUA_TEST_HOME");
        Assert.SkipWhen(home is null, "Set MIRA_SKUA_TEST_HOME to a folder with a Skua Scripts checkout.");
        var path = Path.Combine(home!, "Scripts", "Skills", "AdvancedSkills.json");
        var config = System.Text.Json.JsonSerializer.Deserialize<AdvancedSkillsConfigJson>(File.ReadAllText(path))!;
        var bot = Offline.Bot();

        var loaded = new AdvancedSkills(path).Loaded;

        Assert.Equal(config.Values.Sum(modes => modes.Count), loaded.Count);
        foreach (var skill in loaded)
        {
            var provider = new AdvancedSkillProvider(bot.Player, bot.Self, bot.Target, bot.Combat, bot.Flash);
            provider.Load(skill.Skills);
            Assert.Equal(config[skill.ClassName][skill.ClassUseMode.ToString()].Skills.Count, provider.SkillCount);
        }
    }
}
