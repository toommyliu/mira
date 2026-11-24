using Mira.Skua.Protocol;
using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class ItemList(IFlashUtil flash, string path)
{
    public List<InventoryItem> All => flash.GetGameObject<List<InventoryItem>>(path, []) ?? [];

    public InventoryItem? Get(string name) => Named(name).FirstOrDefault();

    public InventoryItem? Get(int id) => WithId(id).FirstOrDefault();

    public bool Contains(string name, int quantity) => quantity == 0 || Named(name).Exists(item => Holds(item, quantity));

    public bool Contains(int id, int quantity) => quantity == 0 || WithId(id).Exists(item => Holds(item, quantity));

    private static bool Holds(InventoryItem item, int quantity) => item.Quantity >= quantity || item.Category == ItemCategory.Class;

    private List<InventoryItem> Named(string name) => Find("findItemsByName", name).FindAll(item => item.Name == name);

    private List<InventoryItem> WithId(int id) => Find("findItemsById", id).FindAll(item => item.ID == id);

    private List<InventoryItem> Find(string function, object key)
    {
        try
        {
            return flash.Call(function, path, key) is { } found ? JsonConvert.DeserializeObject<List<InventoryItem>>(found) ?? [] : [];
        }
        catch (Exception error) when (error is MiraException or JsonException)
        {
            return [];
        }
    }
}
