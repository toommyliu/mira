using System.Text.Json.Nodes;
using Newtonsoft.Json.Linq;

namespace Mira.Skua.Bot.Members;

public static class Items
{
    private static readonly (string Mira, string Game)[] Names =
    [
        ("itemId", "ItemID"), ("charItemId", "CharItemID"), ("name", "sName"), ("description", "sDesc"),
        ("quantity", "iQty"), ("maxStack", "iStk"), ("member", "bUpg"), ("wearable", "bWear"),
        ("coins", "bCoins"), ("category", "sType"), ("equipped", "bEquip"), ("link", "sLink"),
        ("file", "sFile"), ("meta", "sMeta"),
    ];

    public static T? From<T>(JsonNode? item) where T : class
    {
        if (item is not JsonObject fields)
            return null;
        var game = new JObject();
        foreach (var (mira, name) in Names)
            if (fields[mira] is { } value)
                game[name] = JToken.Parse(value.ToJsonString());
        return game.ToObject<T>();
    }

    public static List<T> Many<T>(JsonNode? items) where T : class =>
        (items as JsonArray ?? []).Select(From<T>).OfType<T>().ToList();
}
