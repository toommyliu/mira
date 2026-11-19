using Skua.Core.Models.Auras;

namespace Skua.Core.Interfaces;

/// <summary>
/// Comprehensive helper for managing ultra boss mechanics
/// </summary>
public interface IUltraBossHelper
{
    #region Counter Attack Methods
    /// <summary>
    /// Enable counter-attack handling. When enabled, the helper will automatically stop attacks
    /// during counter-attack phases and resume attacking the previous target afterward.
    /// </summary>
    void EnableCounterAttack();

    /// <summary>
    /// Disable counter-attack handling.
    /// </summary>
    void DisableCounterAttack();

    /// <summary>
    /// Gets whether counter-attack handling is currently enabled.
    /// </summary>
    bool IsCounterAttackEnabled { get; }

    /// <summary>
    /// Gets whether a counter-attack is currently active (attacks should be stopped).
    /// </summary>
    bool IsCounterAttackActive { get; }

    /// <summary>
    /// Manually set whether attacks should be stopped (useful for custom counter-attack logic).
    /// </summary>
    /// <param name="shouldStop">Whether attacks should be stopped</param>
    void SetAttacksStopped(bool shouldStop);
    #endregion

    #region Aura Analysis Methods
    /// <summary>
    /// Analyzes charge mechanics based on specified charge aura names.
    /// </summary>
    /// <param name="auras">Auras to check (self or target)</param>
    /// <param name="positiveChargeName">Name of positive charge aura</param>
    /// <param name="negativeChargeName">Name of negative charge aura</param>
    /// <param name="reversedSuffix">Suffix that indicates reversed charges (e.g., "?")</param>
    /// <returns>Tuple indicating charge states</returns>
    (bool hasPositive, bool hasNegative, bool hasReversed) AnalyzeChargeMechanics(
        IScriptSelfAuras auras,
        string positiveChargeName,
        string negativeChargeName,
        string? reversedSuffix = null);

    /// <summary>
    /// Checks if a specific aura value meets a threshold condition.
    /// </summary>
    /// <param name="auras">Auras interface to check</param>
    /// <param name="auraName">Name of the aura to check</param>
    /// <param name="threshold">Threshold value</param>
    /// <param name="comparison">Comparison type ("&lt;", "&gt;", "&lt;=", "&gt;=", "==", "!=")</param>
    /// <returns>True if condition is met</returns>
    bool CheckAuraThreshold(IScriptSelfAuras auras, string auraName, int threshold, string comparison = ">=");

    /// <summary>
    /// Gets all auras matching a pattern from a list of auras (case-insensitive).
    /// </summary>
    /// <param name="auras">List of auras to search</param>
    /// <param name="namePattern">Pattern to match (supports wildcards)</param>
    /// <returns>List of matching auras</returns>
    List<Aura> GetAurasMatchingPattern(List<Aura> auras, string namePattern);

    /// <summary>
    /// Checks if all aura conditions are met for skill usage (case-insensitive).
    /// </summary>
    /// <param name="selfAuras">Self auras interface to check</param>
    /// <param name="conditions">Dictionary of aura names and their required conditions</param>
    /// <returns>True if all conditions are met</returns>
    bool ShouldUseSkill(IScriptSelfAuras selfAuras, Dictionary<string, Func<float, bool>> conditions);

    /// <summary>
    /// Checks if all aura conditions are met for skill usage using target auras (case-insensitive).
    /// </summary>
    /// <param name="targetAuras">Target auras interface to check</param>
    /// <param name="conditions">Dictionary of aura names and their required conditions</param>
    /// <returns>True if all conditions are met</returns>
    bool ShouldUseSkill(IScriptTargetAuras targetAuras, Dictionary<string, Func<float, bool>> conditions);
    #endregion

}
