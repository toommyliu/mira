using Newtonsoft.Json;
using Skua.Core.Interfaces;
using Skua.Core.Models.Items;
using Skua.Core.Models.Shops;

namespace Mira.Skua.Bot.Members;

public sealed class Shops : IScriptShop
{
    private readonly Game game;
    private readonly Paths paths;
    private readonly Func<IScriptInterface> bot;
    private readonly List<ShopInfo> loaded = [];

    public Shops(Game game, Paths paths, Func<IScriptInterface> bot, Events events)
    {
        this.game = game;
        this.paths = paths;
        this.bot = bot;
        events.Commands += command =>
        {
            if (command == "loadShop")
                Remember();
        };
    }

    public int ID => paths.Get<int>("world.shopinfo.ShopID");
    public string Name => paths.Get("world.shopinfo.sName", string.Empty);
    public List<ShopItem> Items => paths.Get<List<ShopItem>>("world.shopinfo.items", []);

    public bool IsLoaded => game.Request("shops", "isOpen")?.GetValue<bool>() == true;

    public List<ShopInfo> LoadedCache
    {
        get
        {
            lock (loaded) return [.. loaded];
        }
        set
        {
            lock (loaded)
            {
                loaded.Clear();
                loaded.AddRange(value);
            }
        }
    }

    public bool ProtectFavorites { get; set; } = true;

    public void Load(int id) => game.Request("shops", "load", new { shopId = id });

    public void LoadArmourCustomizer() => paths.Call("openArmorCustomize");

    public void LoadHairShop(int id) => throw Unsupported.Member("Shops.LoadHairShop");

    public List<string> GetUnmetPurchaseRequirements(ShopItem item, int quantity = -1)
    {
        var requirements = bot().Flash.Call("getUnmetPurchaseRequirements", item.ID, item.ShopItemID, quantity);
        try
        {
            return (string.IsNullOrWhiteSpace(requirements) ? null : JsonConvert.DeserializeObject<List<string>>(requirements))
                ?? ["Could not read purchase requirements."];
        }
        catch (JsonException)
        {
            return ["Could not read purchase requirements."];
        }
    }

    public void BuyItem(string name, int quantity = -1) => Buy(name, quantity);

    public void BuyItem(int id, int shopItemId = -1, int quantity = -1) =>
        Buy(shopItemId == -1 ? new { itemId = id } : new { shopItemId }, quantity);

    public void EnsureLoadAndBuyItem(int shopId, string itemName, int quantity = -1)
    {
        Load(shopId);
        BuyItem(itemName, quantity);
    }

    public void EnsureLoadAndBuyItem(int shopId, int itemId, int shopItemId = -1, int quantity = -1)
    {
        Load(shopId);
        BuyItem(itemId, shopItemId, quantity);
    }

    public void SellItem(string name, int quantity = -1)
    {
        if (bot().Inventory.TryGetItem(name, out InventoryItem? item))
            Sell(item!, quantity);
    }

    public void SellItem(int id, int quantity = -1)
    {
        if (bot().Inventory.TryGetItem(id, out InventoryItem? item))
            Sell(item!, quantity);
    }

    private void Buy(object query, int quantity) =>
        game.Request("shops", "buy", new { query, quantity = quantity == -1 ? (int?)null : quantity });

    private void Sell(InventoryItem item, int quantity)
    {
        if (ProtectFavorites && bot().Inventory.IsFavorited(item.ID))
            return;
        game.Request("shops", "sell", new { query = new { itemId = item.ID }, quantity = quantity == -1 ? (int?)null : quantity });
    }

    private void Remember()
    {
        var shop = new ShopInfo(ID, Name, Items);
        lock (loaded)
        {
            if (!loaded.Contains(shop))
                loaded.Add(shop);
        }
    }
}
