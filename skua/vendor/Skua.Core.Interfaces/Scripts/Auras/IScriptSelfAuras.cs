namespace Skua.Core.Interfaces;

/// <summary>
/// Defines functionality for managing auras that are applied to the player.
/// </summary>
/// <remarks>
/// This interface extends <see cref="IScriptAuras"/> to provide operations specific to self-applied
/// auras. Implementations may offer additional methods or properties for handling auras that affect the player itself,
/// as opposed to those applied to others.
/// </remarks>
public interface IScriptSelfAuras : IScriptAuras
{
    /// <summary>
    /// Retrieves the auras currently applied to the player.
    /// </summary>
    /// <param name="playerName">The name of the aura to retrieve. This value is case-sensitive and cannot be null or empty.</param>
    /// <returns>A string representing the value of the specified aura if it is active on the player; otherwise, null.</returns>
    string GetPlayerAura(string playerName);
}