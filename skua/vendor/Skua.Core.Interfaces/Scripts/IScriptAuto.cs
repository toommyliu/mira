using Skua.Core.Models.Skills;
using System.ComponentModel;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines the contract for controlling automated attack and hunt features, including starting, stopping, and
/// monitoring the running state.
/// </summary>
/// <remarks>
/// Implementations of this interface provide mechanisms to automate attack or hunt actions in a game
/// context. The interface supports both synchronous and asynchronous stopping operations, making it suitable for
/// integration with UI components that require non-blocking behavior. The interface also notifies listeners of property
/// changes via the <see cref="INotifyPropertyChanged"/> interface, allowing consumers to react to changes in the running
/// state.
/// </remarks>
public interface IScriptAuto : INotifyPropertyChanged
{
    /// <summary>
    /// Whether the Auto Attack/Hunt is running.
    /// </summary>
    bool IsRunning { get; }

    /// <summary>
    /// Starts the Auto Attack function.
    /// </summary>
    /// <param name="className">The name of the class to use.</param>
    /// <param name="classUseMode">The <see cref="ClassUseMode"/> to use the class.</param>
    /// <param name="manualMapIDs">Optional array of MapIDs to target with priority order.</param>
    void StartAutoAttack(string? className = null, ClassUseMode classUseMode = ClassUseMode.Base, int[]? manualMapIDs = null);

    /// <summary>
    /// Starts the Auto Hunt function. The player will hunt the current target or all the monsters in the current room throughout the map.
    /// </summary>
    /// <param name="className">The name of the class to use.</param>
    /// <param name="classUseMode">The <see cref="ClassUseMode"/> to use the class.</param>
    /// <param name="manualMapIDs">Optional array of MapIDs to target with priority order.</param>
    void StartAutoHunt(string? className = null, ClassUseMode classUseMode = ClassUseMode.Base, int[]? manualMapIDs = null);

    /// <summary>
    /// Stops the Auto Attack/Hunt.
    /// </summary>
    void Stop();

    /// <summary>
    /// Stops the Auto Attack/Hunt asynchronously, use for UI elements.
    /// </summary>
    ValueTask StopAsync();
}