using System.Text.Json;
using Skua.Core.Models;
using Skua.Core.Models.Skills;

namespace Mira.Skua.Bot;

public sealed class AdvancedSkills(Func<string> path)
{
    private readonly Lazy<List<AdvancedSkill>> loaded = new(() => Load(path()));

    public AdvancedSkills(string path)
        : this(() => path)
    {
    }

    public static AdvancedSkills FromScripts() => new(() => ClientFileSources.SkuaAdvancedSkillsFile);

    public List<AdvancedSkill> Loaded => loaded.Value;

    public AdvancedSkill? Find(string className, string mode) =>
        Loaded.FirstOrDefault(skill => skill.ClassName == className && skill.ClassUseMode.ToString() == mode);

    private static List<AdvancedSkill> Load(string path)
    {
        if (!File.Exists(path))
            return [];
        var config = JsonSerializer.Deserialize<AdvancedSkillsConfigJson>(File.ReadAllText(path)) ?? [];
        var skills = new List<AdvancedSkill>();
        foreach (var (className, modes) in config)
        {
            foreach (var (classUseMode, mode) in modes)
            {
                var skill = new AdvancedSkill(
                    className,
                    SkillsText(mode.Skills),
                    mode.SkillTimeout,
                    classUseMode,
                    mode.SkillUseMode == "UseIfAvailable" ? SkillUseMode.UseIfAvailable : SkillUseMode.WaitForCooldown,
                    mode.ResetComboOnTargetChange);
                if (mode.SkillTimeoutMs is int timeoutMs)
                    skill.SkillTimeoutMs = timeoutMs;
                skills.Add(skill);
            }
        }
        return skills;
    }

    private static string SkillsText(List<AdvancedSkillJson> skills)
    {
        List<string> parts = [];
        foreach (var skill in skills)
        {
            var text = skill.SkillId.ToString();
            if (skill.Rules?.Count > 0)
                text += " " + RulesText(skill.Rules, skill.MultiAura, skill.MultiAuraOperator, skill.RuleOperator == "OR");
            if (skill.SkipOnMatch)
                text += "S";
            parts.Add(text);
        }
        return string.Join(" | ", parts);
    }

    private static string RulesText(List<SkillRuleJson> rules, bool isMultiAura, string? multiAuraOperator, bool matchAny)
    {
        List<string> parts = [];
        var multiAura = rules.Where(rule => isMultiAura && rule.Type == "MultiAura").ToList();
        var singleAura = rules.Where(rule => !isMultiAura && rule.Type == "Aura").ToList();
        foreach (var rule in rules.Where(rule => rule.Type is not "MultiAura" and not "Aura" and not "Skip"))
        {
            var unit = (rule.IsPercentage ?? true) ? "%" : "#";
            var compare = rule.Comparison == "greater" ? ">" : "<";
            switch (rule.Type)
            {
                case "Health":
                    parts.Add($"H{compare}{rule.Value}{unit}");
                    break;
                case "Mana":
                    parts.Add($"M{compare}{rule.Value}{unit}");
                    break;
                case "PartyHealth":
                    parts.Add($"PH{compare}{rule.Value}{unit}");
                    break;
                case "Wait":
                    parts.Add($"WW{rule.Timeout}");
                    break;
            }
        }
        if (isMultiAura && multiAura.Count > 0)
            parts.Add(AuraGroup(multiAura, multiAuraOperator == "OR" ? ":" : "&"));
        else if (singleAura.Count > 1)
            parts.Add(AuraGroup(singleAura, matchAny ? ":" : "&"));
        else if (singleAura.Count == 1)
            parts.Add($"A{Aura(singleAura[0])}");
        return string.Join(matchAny ? " OR " : " ", parts);
    }

    private static string AuraGroup(List<SkillRuleJson> rules, string join) =>
        string.Join(" ", rules.Select((rule, index) => $"MA{Aura(rule)}{(index < rules.Count - 1 ? join : "")}"));

    private static string Aura(SkillRuleJson rule) =>
        $"{(rule.Comparison == "greater" ? ">" : "<")}\"{rule.AuraName}\" {rule.Value}{(rule.AuraTarget == "target" ? " TARGET" : "")}";
}
