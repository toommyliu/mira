namespace Mira.Skua.Tests;

public class BotTests
{
    [Fact]
    public void Reading_an_option_returns_what_the_script_set_without_calling_the_game()
    {
        var game = new Offline();
        var bot = game.Bot();

        bot.Options.InfiniteRange = true;
        bot.Options.SkipCutscenes = true;
        var calls = game.Calls.Count;

        Assert.True(bot.Options.InfiniteRange);
        Assert.True(bot.Options.SkipCutscenes);
        Assert.False(bot.Options.Magnetise);
        Assert.Equal(calls, game.Calls.Count);
    }

    [Fact]
    public void Joining_a_map_takes_the_room_from_the_scripts_options_and_not_mira()
    {
        var game = new Offline();
        var bot = game.Bot();

        bot.Map.Join("yulgar");
        bot.Options.PrivateRooms = true;
        bot.Options.PrivateNumber = 4242;
        bot.Map.Join("tercessuinotlim");
        bot.Map.Join("bludrut-9999");
        bot.Options.PrivateNumber = 0;
        bot.Map.Join("battleon");

        var joins = game.Calls.Where(call => (string?)call["op"] == "joinMap").ToList();
        Assert.Equal(["yulgar", "tercessuinotlim-4242", "bludrut-9999", "battleon-100000"],
            joins.Select(call => (string?)call["map"]));
        Assert.All(joins, call => Assert.Equal("public", (string?)call["rooms"]?["kind"]));
    }

    [Fact]
    public async Task Waiting_for_a_value_matches_an_equal_value()
    {
        var bot = Offline.Bot();
        var health = 100;

        Assert.True(bot.Wait.For(() => health, 100, 1));
        Assert.True(await bot.Wait.ForAsync(() => health, 100, 1, TestContext.Current.CancellationToken));
        Assert.False(bot.Wait.For(() => health, 50, 1));
    }

    [Fact]
    public void Closing_a_choice_returns_cancelled_instead_of_a_button()
    {
        var game = new Offline { Answer = _ => null };

        var result = game.Bot().ShowMessageBox("Use tickets?", "Mode Selector", "Select Amount", "GO FOR BROKE!", "Just crunch the numbers");

        Assert.Equal(new global::Skua.Core.Models.DialogResult("Cancel", -1), result);
        Assert.Equal(["Select Amount", "GO FOR BROKE!", "Just crunch the numbers"],
            Assert.Single(game.Asks)["choices"]!.AsArray().Select(choice => (string?)choice));
    }

    [Fact]
    public void Picking_a_choice_returns_its_text_and_position()
    {
        var game = new Offline { Answer = _ => 2 };

        var result = game.Bot().ShowMessageBox("Use tickets?", "Mode Selector", "Select Amount", "GO FOR BROKE!", "Just crunch the numbers");

        Assert.Equal(new global::Skua.Core.Models.DialogResult("Just crunch the numbers", 2), result);
    }

    [Fact]
    public void A_stopped_run_sends_no_more_requests()
    {
        var game = new Recorder();
        using var run = new CancellationTokenSource();
        var mira = new Mira.Skua.Bot.Game(game, run.Token);
        run.Cancel();

        Assert.ThrowsAny<OperationCanceledException>(() => mira.Request("player", "member"));
        Assert.Empty(game.Calls);
    }

    private sealed class Recorder : Mira.Skua.Protocol.IConnection
    {
        public List<System.Text.Json.Nodes.JsonObject> Calls { get; } = [];
        public CancellationToken Stopping => CancellationToken.None;
        public System.Text.Json.Nodes.JsonNode? Call(System.Text.Json.Nodes.JsonObject request, CancellationToken cancel)
        {
            Calls.Add(request);
            return null;
        }
        public System.Text.Json.Nodes.JsonNode? Ask(System.Text.Json.Nodes.JsonObject dialog, CancellationToken cancel) => null;
        public void Log(string level, string text) { }
    }
}
