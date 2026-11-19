using System.Globalization;
using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models.Auras;

namespace Mira.Skua.Bot.Members;

public class SubjectAuras(IFlashUtil flash, string subject, Func<string?> read)
{
    protected IFlashUtil Flash => flash;

    public List<Aura> Auras => Parse<Aura>(read());

    public List<AuraSnapshot> Snapshots => Parse<AuraSnapshot>(flash.Call("GetAuraSnapshots", subject));

    public AuraSnapshot? GetAuraSnapshot(string auraName) =>
        Snapshots.FirstOrDefault(aura => aura.Name.Equals(auraName, StringComparison.OrdinalIgnoreCase));

    public int GetAuraStacks(string auraName) => GetAuraSnapshot(auraName)?.Stacks ?? 0;

    public float GetAuraRuleStacks(string auraName) =>
        float.TryParse(flash.Call("GetAuraRuleStacks", subject, auraName), NumberStyles.Float, CultureInfo.InvariantCulture, out var stacks) ? stacks : 0;

    public Aura? GetAura(string auraName) => Auras.FirstOrDefault(aura => aura.Name.Equals(auraName, StringComparison.OrdinalIgnoreCase));

    public bool HasActiveAura(string auraName) => GetAura(auraName) is not null;

    public float GetAuraValue(string auraName) => flash.Call<float>("GetAurasValue", subject, auraName);

    public bool HasAnyActiveAura(params string[] auraNames) => flash.Call<bool>("HasAnyActiveAura", subject, string.Join(",", auraNames));

    private static List<T> Parse<T>(string? json)
    {
        if (string.IsNullOrWhiteSpace(json))
            return [];
        try
        {
            return JsonConvert.DeserializeObject<List<T>>(json) ?? [];
        }
        catch (JsonException)
        {
            return [];
        }
    }
}

public sealed class SelfAuras(IFlashUtil flash, Func<IScriptPlayer> player)
    : SubjectAuras(flash, "Self", () => player().Username is { } name ? flash.Call("GetPlayerAura", name.ToLowerInvariant()) : null)
{
    public string GetPlayerAura(string playerName) => Flash.Call("GetPlayerAura", playerName) ?? "[]";
}

public sealed class TargetAuras(IFlashUtil flash, Func<IScriptPlayer> player)
    : SubjectAuras(flash, "Target", () => flash.Call("GetMonsterAuraByID", player().Target?.MapID ?? 0))
{
    public string GetMonsterAura(string monsterName) => Flash.Call("GetMonsterAuraByName", monsterName) ?? "[]";

    public string GetMonsterAura(int monID) => Flash.Call("GetMonsterAuraByID", monID) ?? "[]";
}
