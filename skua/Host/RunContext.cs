using System.Reflection;
using System.Runtime.Loader;
using Mira.Skua.Protocol;

namespace Mira.Skua;

sealed class RunContext(string name, string? core) : AssemblyLoadContext(name, isCollectible: true)
{
    public static readonly string[] Private =
        ["Mira.Skua.Bot", "Skua.Core", "Skua.Core.Interfaces", "Skua.Core.Models", "Skua.Core.Utils", "Newtonsoft.Json", "CommunityToolkit.Mvvm"];

    public const string Core = "SkuaCore";

    protected override Assembly? Load(AssemblyName assembly)
    {
        if (assembly.Name == Core && core is not null)
            return LoadFromAssemblyPath(core);
        return Private.Contains(assembly.Name) ? LoadFromAssemblyPath(Path.Combine(AppContext.BaseDirectory, assembly.Name + ".dll")) : null;
    }

    public IRunner Runner()
    {
        var bot = LoadFromAssemblyName(new AssemblyName("Mira.Skua.Bot"));
        return (IRunner)Activator.CreateInstance(bot.GetType("Mira.Skua.Bot.Runner", throwOnError: true)!)!;
    }
}
