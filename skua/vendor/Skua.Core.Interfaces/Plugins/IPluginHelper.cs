namespace Skua.Core.Interfaces;

/// <summary>
/// Provides methods for managing custom menu buttons within a plugin environment.
/// </summary>
/// <remarks>
/// Implementations of this interface allow plugins to add or remove interactive buttons to a host
/// application's menu system.
/// </remarks>
public interface IPluginHelper
{
    /// <summary>
    /// Adds a new button to the "Plugins" dropdown with the specified display text and associated action.
    /// </summary>
    /// <param name="text">The text to display on the menu button. Cannot be null or empty.</param>
    /// <param name="action">The action to execute when the menu button is selected. Cannot be null.</param>
    void AddMenuButton(string text, Action action);

    /// <summary>
    /// Removes the menu button with the previous set display text from the menu.
    /// </summary>
    /// <param name="text">The display text of the menu button to remove. The comparison is case-sensitive.</param>
    void RemoveMenuButton(string text);
}