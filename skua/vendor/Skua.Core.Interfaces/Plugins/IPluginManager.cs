namespace Skua.Core.Interfaces;

/// <summary>
/// Defines methods and properties for managing the lifecycle and access of plugins within the application.
/// </summary>
/// <remarks>
/// The IPluginManager interface provides functionality to load, unload, and retrieve information about
/// plugins at runtime. It enables dynamic management of plugins, allowing for extensibility and modularity in the
/// application. Implementations are responsible for handling plugin discovery, loading from assemblies, and maintaining
/// references to plugin containers.
/// </remarks>
public interface IPluginManager
{
    /// <summary>
    /// Gets a list of currently loaded plugins' containers.
    /// </summary>
    List<IPluginContainer> Containers { get; }

    /// <summary>
    /// Loads all the plugins in the plugins folder.
    /// </summary>
    void Initialize();

    /// <summary>
    /// Loads the plugin at the given path.
    /// </summary>
    /// <param name="path">The path to the plugin assembly.</param>
    /// <returns>Whether the plugin was loaded successfully or not.</returns>
    Exception? Load(string path);

    /// <summary>
    /// Unloads the given plugin.
    /// </summary>
    /// <param name="plugin">The plugin to unload.</param>
    void Unload(ISkuaPlugin plugin);

    /// <summary>
    /// Unloads the plugin by its given name.
    /// </summary>
    /// <param name="pluginName">The name o the plugin to unload.</param>
    void Unload(string pluginName);

    /// <summary>
    /// Gets the container for the given plugin.
    /// </summary>
    /// <param name="plugin">The plugin to get the container for.</param>
    /// <returns>The plugin's container.</returns>
    IPluginContainer? GetContainer(ISkuaPlugin plugin);

    /// <summary>
    /// Gets the container for the given plugin name.
    /// </summary>
    /// <param name="pluginName">Name of the plugin to get the container for.</param>
    /// <returns>The plugin's container</returns>
    IPluginContainer? GetContainer(string pluginName);

    /// <summary>
    /// Gets the container for the plugin with the given type.
    /// </summary>
    /// <typeparam name="T">The type of the plugin to get the container for.</typeparam>
    /// <returns>The container for the plugin with the given type.</returns>
    IPluginContainer GetContainer<T>() where T : ISkuaPlugin;
}