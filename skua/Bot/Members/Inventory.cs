using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;
using Skua.Core.Models.Shops;

namespace Mira.Skua.Bot.Members;

public sealed class Inventory(Game game, IFlashUtil flash, Paths paths, Func<IScriptInterface> bot) : IScriptInventory
{
    private readonly ItemList items = new(flash, "world.myAvatar.items");

    private IScriptInventory Self => this;

    public List<InventoryItem> Items => items.All;
    public InventoryItem? GetItem(string name) => items.Get(name);
    public InventoryItem? GetItem(int id) => items.Get(id);
    public bool Contains(string name, int quantity = 1) => items.Contains(name, quantity);
    public bool Contains(int id, int quantity = 1) => items.Contains(id, quantity);
    public int Slots => paths.Get<int>("world.myAvatar.objData.iBagSlots");

    public int UsedSlots => flash.Call<int>("inventoryBagUsedSlots");
    public bool HasCategories => flash.Call<bool>("hasInventoryCategories");
    public int MiscSlots => flash.Call<int>("inventoryMiscSlots");
    public int MiscUsedSlots => flash.Call<int>("inventoryMiscUsedSlots");
    public int MiscFreeSlots => MiscSlots - MiscUsedSlots;

    public string GetPool(ItemBase item) => flash.Call<string>("inventoryPool", JsonConvert.SerializeObject(item)) ?? "bag";

    public bool HasSpaceFor(ItemBase item, int quantity = 1) =>
        !bool.TryParse(flash.Call("inventoryHasSpaceFor", JsonConvert.SerializeObject(item), quantity, item is ShopItem), out var space) || space;

    public bool CanBank(InventoryItem item) => item.Category != ItemCategory.Class || !HasCategories;

    public bool IsFavorited(int itemId) => flash.Call<bool>("isFavoriteItem", itemId);

    public void EquipItem(int id) => game.Request("inventory", "equip", new { query = new { itemId = id } });

    public void EquipUsableItem(InventoryItem? item)
    {
        if (item is not null)
            EquipItem(item.ID);
    }

    public bool ToBank(InventoryItem item) =>
        CanBank(item) && game.Request("bank", "deposit", new { query = new { itemId = item.ID } })?.GetValue<bool>() == true;

    public bool EnsureToBank(string name) => Self.TryGetItem(name, out InventoryItem? item) && Ensure(item!, () => !Self.Contains(name));

    public bool EnsureToBank(int id) => Self.TryGetItem(id, out InventoryItem? item) && Ensure(item!, () => !Self.Contains(id));

    private bool Ensure(InventoryItem item, Func<bool> done)
    {
        if (!CanBank(item))
            return false;
        var options = bot().Options;
        for (var attempt = 0; !ToBank(item) && !bot().ShouldExit && bot().Player.Playing && ++attempt < options.MaximumTries;)
            bot().Sleep(options.ActionDelay);
        return done();
    }
}
