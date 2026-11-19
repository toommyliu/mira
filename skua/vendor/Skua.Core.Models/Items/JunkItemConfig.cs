using System.Text.Json.Serialization;

namespace Skua.Core.Models.Items;

public sealed class JunkItemConfig
{
    [JsonPropertyName("ID")]
    public int ID { get; set; }

    [JsonPropertyName("Name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("Category")]
    public string Category { get; set; } = string.Empty;

    [JsonPropertyName("Meta")]
    public string Meta { get; set; } = string.Empty;
}
