using System.Text.Json.Nodes;
using Mira.Skua.Protocol;
using Skua.Core.Models.Items;

namespace Mira.Skua.Tests;

public class InventoryTests
{
    [Fact]
    public void A_usable_item_is_equipped_rather_than_used()
    {
        var game = new Offline();
        var bot = game.Bot();

        bot.Inventory.EquipUsableItem(new InventoryItem { ID = 18927 });

        var request = Assert.Single(game.Calls);
        Assert.Equal("inventory", request["domain"]?.ToString());
        Assert.Equal("equip", request["op"]?.ToString());
        Assert.Equal(18927, request["query"]?["itemId"]?.GetValue<int>());
    }

    [Fact]
    public void Item_checks_follow_skua_name_and_quantity_rules_on_the_items_flash_finds()
    {
        static JsonObject Item(int id, string name, int quantity, string type = "Item") =>
            new() { ["ItemID"] = id, ["sName"] = name, ["iQty"] = quantity, ["sType"] = type };
        var bot = Offline.Bot(new()
        {
            ["skua.findItemsByName:world.myAvatar.items"] = new JsonArray(Item(1, "Gold Bar", 50), Item(2, " Gold ", 3), Item(3, "gold", 99)).ToJsonString(),
            ["skua.findItemsById:world.myAvatar.items"] = new JsonArray(Item(4, "Necromancer", 0, "Class")).ToJsonString(),
            ["skua.findItemsByName:world.bankinfo.items"] = new JsonArray(Item(5, "Gold", 10)).ToJsonString(),
        });

        Assert.True(bot.Inventory.Contains("Gold", 3));
        Assert.False(bot.Inventory.Contains("Gold", 4));
        Assert.Equal(3, bot.Inventory.GetQuantity("Gold"));
        Assert.Equal(2, bot.Inventory.GetItem("Gold")?.ID);
        Assert.True(bot.Inventory.Contains(4, 300));
        Assert.False(bot.Inventory.Contains(5));
        Assert.True(bot.Inventory.Contains("Silver", 0));
        Assert.True(bot.Bank.Contains("Gold", 10));
        Assert.False(bot.Bank.Contains("Gold", 11));

        var unloaded = new Offline((_, _) => throw new MiraException("The game isn't loaded.")).Bot();
        Assert.False(unloaded.Inventory.Contains("Gold"));
        Assert.Null(unloaded.Inventory.GetItem(4));
    }
}
