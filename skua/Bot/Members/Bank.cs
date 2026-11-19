using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class Bank(Game game, IFlashUtil flash, Paths paths, Func<IScriptInterface> bot) : IScriptBank
{
    private IScriptBank Self => this;

    public List<InventoryItem> Items => paths.Get<List<InventoryItem>>("world.bankinfo.items", []);
    public int Slots => paths.Get<int>("world.myAvatar.objData.iBankSlots");
    public int UsedSlots => paths.Get<int>("world.myAvatar.iBankCount");

    public bool Loaded { get; set; }

    public void Open() => game.Request("bank", "open");

    public void Load(bool waitForLoad = true) => Loaded = game.Request("bank", "load")?.GetValue<bool>() ?? false;

    public bool Swap(string invItem, string bankItem) =>
        bot().Inventory.TryGetItem(invItem, out InventoryItem? inventory) && Self.TryGetItem(bankItem, out InventoryItem? bank) && Swap(inventory!, bank!);

    public bool Swap(int invItem, int bankItem) =>
        bot().Inventory.TryGetItem(invItem, out InventoryItem? inventory) && Self.TryGetItem(bankItem, out InventoryItem? bank) && Swap(inventory!, bank!);

    public bool Swap(InventoryItem invItem, InventoryItem bankItem) =>
        game.Request("bank", "swap", new { inventoryQuery = new { itemId = invItem.ID }, bankQuery = new { itemId = bankItem.ID } })?.GetValue<bool>() == true;

    public bool EnsureSwap(string invItem, string bankItem, bool loadBank = true) => Ensure(loadBank, () => Swap(invItem, bankItem));

    public bool EnsureSwap(int invItem, int bankItem, bool loadBank = true) => Ensure(loadBank, () => Swap(invItem, bankItem));

    public bool ToInventory(InventoryItem item) =>
        game.Request("bank", "withdraw", new { query = new { itemId = item.ID } })?.GetValue<bool>() == true;

    public bool EnsureToInventory(string name, bool loadBank = true) =>
        Ensure(loadBank, () => Self.TryGetItem(name, out InventoryItem? item) && ToInventory(item!)) || bot().Inventory.Contains(name);

    public bool EnsureToInventory(int id, bool loadBank = true) =>
        Ensure(loadBank, () => Self.TryGetItem(id, out InventoryItem? item) && ToInventory(item!)) || bot().Inventory.Contains(id);

    public bool HasSpaceFor(InventoryItem bankItem, InventoryItem invItem) =>
        !bool.TryParse(flash.Call("inventoryHasSpaceFor", JsonConvert.SerializeObject(bankItem), 1, false, JsonConvert.SerializeObject(invItem)), out var space) || space;

    private bool Ensure(bool loadBank, Func<bool> attempt)
    {
        if (loadBank && !Loaded)
            Load();
        var options = bot().Options;
        for (var tries = 0; tries < options.MaximumTries && !bot().ShouldExit && bot().Player.Playing; tries++)
        {
            if (attempt())
                return true;
            bot().Sleep(options.ActionDelay);
        }
        return false;
    }
}
