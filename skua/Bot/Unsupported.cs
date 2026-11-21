using System.Reflection;

namespace Mira.Skua.Bot;

public class Unsupported : DispatchProxy
{
    private string area = null!;

    public static NotSupportedException Member(string member) => new($"Mira doesn't support Bot.{member} yet.");

    public static T Area<T>(string area) where T : class
    {
        var proxy = Create<T, Unsupported>();
        ((Unsupported)(object)proxy).area = area;
        return proxy;
    }

    protected override object? Invoke(MethodInfo? method, object?[]? args) => method!.Name switch
    {
        "add_PropertyChanged" or "remove_PropertyChanged" => null,
        var name => throw Member($"{area}.{(method.IsSpecialName ? name[4..] : name)}"),
    };
}
