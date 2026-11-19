using System.Text.Json.Nodes;
using Mira.Skua.Bot;
using Skua.Core.Models.Items;

namespace Mira.Skua.Tests;

public interface ITally
{
    int Count { get; }

    bool TryCount(out int count)
    {
        count = Count;
        return count > 0;
    }

    void Add(ref int total) => total += Count;
}

public sealed class Tally
{
    public int Count => 3;
}

public class SurfaceTests
{
    [Fact]
    public void Default_methods_with_out_parameters_answer_through_the_bot()
    {
        var bot = Offline.Bot();

        Assert.False(bot.Inventory.TryGetItem("Gem", out InventoryItem? item));
        Assert.Null(item);
    }

    [Fact]
    public void Default_methods_write_value_type_out_and_ref_parameters_back()
    {
        var tally = Surface.Over<ITally>("Tally", new Tally(), new Bindings(null!));

        Assert.True(tally.TryCount(out var count));
        Assert.Equal(3, count);
        var total = 4;
        tally.Add(ref total);
        Assert.Equal(7, total);
    }

    [Fact]
    public void A_bound_game_function_keeps_its_overloads_apart()
    {
        var game = new Offline(new Dictionary<string, JsonNode?>
        {
            ["flash.getGameObject:world.strFrame"] = "\"Enter\"",
            ["flash.getGameObject:world.strPad"] = "\"Spawn\"",
        });

        game.Bot().Player.SetSpawnPoint();

        var spawn = game.Calls.Single(call => call["op"]?.ToString() == "setSpawnPoint");
        Assert.Equal("""{"cell":"Enter","pad":"Spawn","domain":"world","op":"setSpawnPoint"}""", spawn.ToJsonString());
    }
}
