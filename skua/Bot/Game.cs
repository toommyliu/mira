using System.Text.Json;
using System.Text.Json.Nodes;
using Mira.Skua.Protocol;

namespace Mira.Skua.Bot;

public sealed class Game
{
    private readonly IConnection connection;

    public Game(IConnection connection, CancellationToken stopping)
    {
        this.connection = connection;
        Stopping = stopping;
        Calls = stopping;
    }

    public IConnection Connection => connection;

    public CancellationToken Stopping { get; }

    public CancellationToken Calls { get; set; }

    public JsonNode? Request(string domain, string op, object? fields = null) => Request(domain, op, fields, Calls);

    public JsonNode? Request(string domain, string op, object? fields, CancellationToken cancel)
    {
        var request = fields is null ? new JsonObject() : JsonSerializer.SerializeToNode(fields, Json.Options)!.AsObject();
        request["domain"] = domain;
        request["op"] = op;
        return connection.Call(request, cancel);
    }

    public JsonNode? Ask(JsonObject dialog) => connection.Ask(dialog, Calls);

    public JsonNode? Loader(string callback, IEnumerable<object?> args) =>
        Request("skua", "call", new { function = callback, args = args.Select(Json.Node).ToArray() });

    public void Log(string text) => connection.Log("info", text);

    public void Warn(string text) => connection.Log("warn", text);
}

public static class Json
{
    public static readonly JsonSerializerOptions Options = new(JsonSerializerDefaults.Web);

    public static JsonNode? Node(object? value) => value is null ? null : JsonSerializer.SerializeToNode(value, value.GetType(), Options);

    public static string? Text(JsonNode? node) => node switch
    {
        null => null,
        JsonValue value when value.TryGetValue(out string? text) => text,
        JsonValue value when value.TryGetValue(out bool flag) => flag.ToString(),
        JsonValue value when value.TryGetValue(out double number) => number.ToString(System.Globalization.CultureInfo.InvariantCulture),
        _ => node.ToJsonString(),
    };

    public static object? Convert(JsonNode? node, Type type)
    {
        if (type == typeof(string))
            return Text(node);
        var text = node is JsonValue value && value.TryGetValue(out string? inner) ? inner : node?.ToJsonString();
        if (string.IsNullOrEmpty(text))
            return type.IsValueType ? Activator.CreateInstance(type) : null;
        try
        {
            return Newtonsoft.Json.JsonConvert.DeserializeObject(text, type);
        }
        catch (Newtonsoft.Json.JsonException)
        {
            return System.Convert.ChangeType(text, Nullable.GetUnderlyingType(type) ?? type, System.Globalization.CultureInfo.InvariantCulture);
        }
    }
}
