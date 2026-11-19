using System.Reflection;

namespace Mira.Skua.Bot;

public class Bag(string area) : IFallback
{
    private readonly Dictionary<string, object?> values = [];

    protected virtual object? Preset(string name) => null;

    protected virtual void Changed(string name, object value) { }

    public void Reset() => values.Clear();

    public void ResetToDefault() => values.Clear();

    public void Save() { }

    public object? Invoke(MethodInfo method, object?[] args)
    {
        if (method.Name is "add_PropertyChanged" or "remove_PropertyChanged")
            return null;
        if (method.IsSpecialName && method.Name.StartsWith("set_"))
        {
            var name = method.Name[4..];
            values[name] = args[0];
            if (args[0] is { } value)
                Changed(name, value);
            return null;
        }
        if (method.IsSpecialName && method.Name.StartsWith("get_"))
        {
            var name = method.Name[4..];
            return values.TryGetValue(name, out var value) ? value
                : Preset(name) is { } preset ? preset
                : method.ReturnType.IsValueType ? Activator.CreateInstance(method.ReturnType) : null;
        }
        throw new NotSupportedException($"Mira doesn't support Bot.{area}.{method.Name} yet.");
    }
}
