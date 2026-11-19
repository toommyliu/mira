using System.Globalization;
using System.Text.Json.Nodes;
using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class ScriptOptions(JsonObject supplied) : IScriptOptionContainer
{
    private const string SkipOption = "SkipOption";

    public string Storage { get; set; } = "default";
    public Dictionary<string, List<IOption>> MultipleOptions { get; } = [];
    public List<IOption> Options { get; } = [];
    public string OptionsFile { get; set; } = string.Empty;
    public Dictionary<IOption, string> OptionValues { get; } = [];
    public List<string> ResetOptions { get; } = [];

    public void Initialize(object script)
    {
        var type = script.GetType();
        if (type.GetField("MultiOptions")?.GetValue(script) is string[] categories)
            foreach (var category in categories)
                if (type.GetField(category)?.GetValue(script) is List<IOption> listed)
                {
                    listed.ForEach(option => option.Category = category.Replace('_', ' '));
                    MultipleOptions[category] = listed;
                }
        if (type.GetField("Options")?.GetValue(script) is List<IOption> options)
            Options.AddRange(options);
        if (type.GetField("OptionsStorage")?.GetValue(script) is string storage)
            Storage = storage;
        SetDefaults();
        foreach (var (key, option) in Keyed())
        {
            if (option.Name == SkipOption)
                OptionValues[option] = bool.TrueString;
            else if (supplied[key] is { } value)
                OptionValues[option] = Text(value);
        }
    }

    public JsonObject Describe()
    {
        var fields = new JsonArray();
        foreach (var (key, option) in Keyed().Where(pair => pair.Option.Name != SkipOption))
        {
            var type = Nullable.GetUnderlyingType(option.Type) ?? option.Type;
            var field = new JsonObject
            {
                ["key"] = key,
                ["group"] = key.Contains(':') ? option.Category : null,
                ["label"] = option.ToString(),
                ["description"] = string.IsNullOrWhiteSpace(option.Description) ? null : option.Description,
                ["kind"] = Kind(type),
                ["default"] = Default(option.DefaultValue, type),
            };
            if (type.IsEnum)
                field["options"] = new JsonArray(Enum.GetNames(type).Select(name => (JsonNode)name.Replace('_', ' ')).ToArray());
            fields.Add(field);
        }
        return new JsonObject { ["fields"] = fields };
    }

    private IEnumerable<(string Key, IOption Option)> Keyed() =>
        Options.Select(option => (option.Name, option))
            .Concat(MultipleOptions.SelectMany(pair => pair.Value.Select(option => ($"{pair.Key}:{option.Name}", option))));

    private static string Kind(Type type) =>
        type == typeof(bool) ? "boolean"
        : type.IsEnum ? "select"
        : type == typeof(int) || type == typeof(long) || type == typeof(short) || type == typeof(byte) ? "integer"
        : type == typeof(double) || type == typeof(float) || type == typeof(decimal) ? "number"
        : "text";

    private static JsonNode? Default(object? value, Type type) => value switch
    {
        null => null,
        bool flag => flag,
        Enum choice => choice.ToString().Replace('_', ' '),
        IConvertible number when Kind(type) is "integer" or "number" => number.ToDouble(CultureInfo.InvariantCulture),
        _ => value.ToString(),
    };

    private static string Text(JsonNode value) => value is JsonValue scalar && scalar.TryGetValue(out bool flag)
        ? flag.ToString()
        : Json.Text(value) ?? string.Empty;

    public void SetDefaults()
    {
        foreach (var (_, option) in Keyed())
            OptionValues[option] = option.DefaultValue?.ToString() ?? string.Empty;
    }

    public T? Get<T>(IOption? option) where T : IConvertible
    {
        if (option is null || !OptionValues.TryGetValue(option, out var value) || string.IsNullOrEmpty(value))
            return default;
        return typeof(T).IsEnum
            ? (T)Enum.Parse(typeof(T), value.Replace(' ', '_'))
            : (T)System.Convert.ChangeType(value, typeof(T), CultureInfo.InvariantCulture);
    }

    public T? Get<T>(string name) where T : IConvertible => Get<T>(Options.Find(option => option.Name == name));

    public T? Get<T>(string category, string name) where T : IConvertible =>
        category == "Options" ? Get<T>(name) : Get<T>(MultipleOptions[category].Find(option => option.Name == name));

    public string GetDirect(IOption? option) => option is null ? string.Empty : OptionValues[option];

    public void Set(string name, object value) => Set(Options.Find(option => option.Name == name), value);

    public void Set(string category, string name, object value) =>
        Set(MultipleOptions[category].Find(option => option.Name == name), value);

    public void Set<T>(IOption? option, T value)
    {
        if (option is not null)
            OptionValues[option] = value?.ToString() ?? string.Empty;
    }

    public void Load() { }

    public void Save() { }

    public void Configure() { }
}
