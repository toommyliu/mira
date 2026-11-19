using System.Reflection;
using System.Text.Json.Nodes;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Bindings(IFlashUtil flash)
{
    private static readonly Dictionary<string, JsonObject> Table = Load();

    private static Dictionary<string, JsonObject> Load()
    {
        using var stream = typeof(Bindings).Assembly.GetManifestResourceStream("bindings.json")!;
        return JsonNode.Parse(stream)!.AsObject().ToDictionary(pair => pair.Key, pair => pair.Value!.AsObject());
    }

    public bool Has(string area, string member) => Table.ContainsKey($"{area}.{member}");

    public bool TryInvoke(string area, MethodInfo method, object?[] args, out object? result)
    {
        result = null;
        var getter = method.IsSpecialName && method.Name.StartsWith("get_");
        var setter = method.IsSpecialName && method.Name.StartsWith("set_");
        var member = getter || setter ? method.Name[4..] : method.Name;
        if (!Table.TryGetValue($"{area}.{member}", out var binding))
            return false;
        var fallback = Text(binding["fallback"]);
        switch (Text(binding["kind"]))
        {
            case "object" when getter && binding["get"]!.GetValue<bool>():
                result = Read(binding, method.ReturnType, fallback);
                return true;
            case "object" when setter && binding["set"]!.GetValue<bool>():
                foreach (var path in binding["paths"]!.AsArray())
                    flash.SetGameObject(Text(path)!, args[0]!);
                return true;
            case "call" or "json" when getter:
                result = flash.Call(Text(binding["function"])!, method.ReturnType) ?? Fallback(fallback, method.ReturnType);
                return true;
            case "method" when binding["parameters"]!.GetValue<int>() == method.GetParameters().Length:
                var function = Text(binding["function"])!;
                var type = method.ReturnType == typeof(void) ? typeof(string) : method.ReturnType;
                var called = binding["game"]!.GetValue<bool>()
                    ? flash.Call("callGameFunction", type, [function, .. args.Select(arg => arg!)])
                    : flash.Call(function, type, args.Select(arg => arg!).ToArray());
                result = method.ReturnType == typeof(void) ? null : called;
                return true;
            default:
                return false;
        }
    }

    public void Write(string area, string member, object value)
    {
        if (Table.TryGetValue($"{area}.{member}", out var binding) && binding["set"]?.GetValue<bool>() == true)
            foreach (var path in binding["paths"]!.AsArray())
                flash.SetGameObject(Text(path)!, value);
    }

    private object? Read(JsonObject binding, Type type, string? fallback)
    {
        if (Text(binding["requireNotNull"]) is { } required && flash.IsNull(required))
            return Fallback(fallback, type);
        var path = Text(binding["paths"]![0])!;
        var text = binding["static"]!.GetValue<bool>() ? flash.GetGameObjectStatic(path) : flash.GetGameObject(path);
        if (text is null)
            return Fallback(fallback, type);
        try
        {
            if (Text(binding["select"]) is { } select)
                return new JArray(JArray.Parse(text).Select(item => item[select])).ToObject(type) ?? Fallback(fallback, type);
            return JsonConvert.DeserializeObject(text, type) ?? Fallback(fallback, type);
        }
        catch (JsonException)
        {
            return Fallback(fallback, type);
        }
    }

    private static object? Fallback(string? text, Type type)
    {
        if (text is "string.Empty")
            return string.Empty;
        if (text is not null && (text.StartsWith("new") || text.StartsWith("Array.Empty")))
            return type.IsArray ? Array.CreateInstance(type.GetElementType()!, 0) : Activator.CreateInstance(type);
        if (text is not null && type.IsPrimitive)
            return System.Convert.ChangeType(text.StartsWith("0x") ? System.Convert.ToInt64(text, 16) : (object)text, type, System.Globalization.CultureInfo.InvariantCulture);
        return type.IsValueType ? Activator.CreateInstance(type) : null;
    }

    private static string? Text(JsonNode? node) => node is JsonValue value && value.TryGetValue(out string? text) ? text : null;
}
