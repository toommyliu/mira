using System.ComponentModel;

namespace Skua.Core.Interfaces;

/// <summary>
/// Represents a set of statistics related to script bot activity, including counts of deaths, kills, drops, quests, and
/// re-logins.
/// </summary>
/// <remarks>
/// Implementations of this interface provide real-time tracking of key gameplay events for a script bot.
/// Properties are typically updated as the bot performs actions in the game. Changes to property values raise the
/// PropertyChanged event, allowing clients to observe updates. The Reset method resets all statistics to their initial
/// state.
/// </remarks>
public interface IScriptBotStats : INotifyPropertyChanged
{
    /// <summary>
    /// The number of times the player has died.
    /// </summary>
    int Deaths { get; set; }

    /// <summary>
    /// The number of drops picked up.
    /// </summary>
    int Drops { get; set; }

    /// <summary>
    /// The number of monsters killed by the bot.
    /// </summary>
    int Kills { get; set; }

    /// <summary>
    /// The number of quests accepted (not unique).
    /// </summary>
    int QuestsAccepted { get; set; }

    /// <summary>
    /// The number of quests completed and turned in (not unique).
    /// </summary>
    int QuestsCompleted { get; set; }

    /// <summary>
    /// The number of times the player has been relogged in.
    /// </summary>
    int Relogins { get; set; }

    /// <summary>
    /// The max inventory space.
    /// </summary>
    int InventorySpace { get; }

    /// <summary>
    /// The filled inventory space.
    /// </summary>
    int InventoryFilledSpace { get; }

    /// <summary>
    /// The free inventory space.
    /// </summary>
    int InventoryFreeSpace { get; }

    /// <summary>
    /// The max bank space.
    /// </summary>
    int BankSpace { get; }

    /// <summary>
    /// The filled bank space.
    /// </summary>
    int BankFilledSpace { get; }

    /// <summary>
    /// The free bank space.
    /// </summary>
    int BankFreeSpace { get; }


    /// <summary>
    /// Resets all values.
    /// </summary>
    void Reset();

    /// <summary>
    /// Gets Inventory & bank space
    /// </summary>
    void GetSpace();
}