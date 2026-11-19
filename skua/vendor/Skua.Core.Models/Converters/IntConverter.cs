using Newtonsoft.Json;

namespace Skua.Core.Models.Converters;

public class IntConverter : JsonConverter<int>
{
    public override void WriteJson(JsonWriter writer, int value, JsonSerializer serializer)
    {
        writer.WriteValue(value);
    }

    public override int ReadJson(JsonReader reader, Type objectType, int existingValue, bool hasExistingValue, JsonSerializer serializer)
    {
        return reader.Value is null or (object)"null"
            ? 1
            : int.TryParse(reader.Value.ToString(), out int result) ? result : 1;
    }
}