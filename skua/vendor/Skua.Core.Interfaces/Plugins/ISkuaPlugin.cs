namespace Skua.Core.Interfaces;

/// <summary>
/// Defines the contract for a Skua plugin, including metadata, lifecycle methods, and configuration options.
/// </summary>
/// <remarks>
/// Implement this interface to create a plugin that can be loaded and managed by the Skua plugin system.
/// The interface provides properties for plugin identification and description, as well as methods for handling plugin
/// initialization and cleanup. Plugins can expose configurable options and specify a unique storage key for their
/// settings.
/// </remarks>
public interface ISkuaPlugin
{
    /// <summary>
    /// The name of the plugin.
    /// </summary>
    string Name { get; }

    /// <summary>
    /// The author of the plugin.
    /// </summary>
    string Author { get; }

    /// <summary>
    /// The description of the plugin.
    /// </summary>
    string Description { get; }

    /// <summary>
    /// Indicates what file name the options of this plugin should be stored under. This needs to be unique to your plugin.
    /// </summary>
    string OptionsStorage => $"{Author}_{Name}".Replace(' ', '_').Trim();

    /// <summary>
    /// Called when the plugin is loaded.
    /// </summary>
    void Load(IServiceProvider provider, IPluginHelper helper);

    /// <summary>
    /// Called when the plugin is unloaded.
    /// </summary>
    void Unload();

    /// <summary>
    /// A list of options this plugin uses. This is only queried once, before Load is called.
    /// </summary>
    List<IOption>? Options { get; }
}