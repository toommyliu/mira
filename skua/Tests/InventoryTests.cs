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
}
