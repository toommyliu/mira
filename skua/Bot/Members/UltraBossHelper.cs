using Skua.Core.Interfaces;
using Skua.Core.Models.Auras;

namespace Mira.Skua.Bot.Members;

public sealed class UltraBossHelper(Game game, Func<IScriptCombat> combat) : IUltraBossHelper
{
    public bool IsCounterAttackEnabled => game.Request("settings", "get")?["antiCounter"]?.GetValue<bool>() ?? false;
    public bool IsCounterAttackActive => throw Unsupported.Member("UltraBossHelper.IsCounterAttackActive");

    public void EnableCounterAttack() => game.Request("settings", "update", new { antiCounter = true });

    public void DisableCounterAttack() => game.Request("settings", "update", new { antiCounter = false });

    public void SetAttacksStopped(bool shouldStop) => combat().StopAttacking = shouldStop;

    public (bool hasPositive, bool hasNegative, bool hasReversed) AnalyzeChargeMechanics(
        IScriptSelfAuras auras, string positiveChargeName, string negativeChargeName, string? reversedSuffix = null) =>
        throw Unsupported.Member("UltraBossHelper.AnalyzeChargeMechanics");

    public bool CheckAuraThreshold(IScriptSelfAuras auras, string auraName, int threshold, string comparison = ">=") =>
        throw Unsupported.Member("UltraBossHelper.CheckAuraThreshold");

    public List<Aura> GetAurasMatchingPattern(List<Aura> auras, string namePattern) =>
        throw Unsupported.Member("UltraBossHelper.GetAurasMatchingPattern");

    public bool ShouldUseSkill(IScriptSelfAuras selfAuras, Dictionary<string, Func<float, bool>> conditions) =>
        throw Unsupported.Member("UltraBossHelper.ShouldUseSkill");

    public bool ShouldUseSkill(IScriptTargetAuras targetAuras, Dictionary<string, Func<float, bool>> conditions) =>
        throw Unsupported.Member("UltraBossHelper.ShouldUseSkill");
}
