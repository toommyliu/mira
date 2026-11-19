namespace Skua.Core.Interfaces;

/// <summary>
/// Represents a collection of aura-related operations that can be performed on a target entity.
/// </summary>
/// <remarks>
/// This interface extends <see cref="IScriptAuras"/> to provide aura management functionality specific
/// to the target entity. Implementations may offer additional context or behaviors relevant to the target entity within a
/// scripting environment.
/// </remarks>
public interface IScriptTargetAuras : IScriptAuras
{
    /// <summary>
    /// Retrieves the aura description associated with the specified monster.
    /// </summary>
    /// <param name="monsterName">The name of the monster whose aura description is to be retrieved. Cannot be null or empty.</param>
    /// <returns>A string containing the aura description of the specified monster, or null if the monster does not have an aura.</returns>
    string GetMonsterAura(string monsterName);

    /// <summary>
    /// Retrieves the aura description associated with the specified monster identifier.
    /// </summary>
    /// <param name="monID">The unique identifier of the monster whose aura description is to be retrieved.</param>
    /// <returns>A string containing the aura description of the specified monster. Returns an empty string if the monster does
    /// not have an associated aura.</returns>
    string GetMonsterAura(int monID);
}