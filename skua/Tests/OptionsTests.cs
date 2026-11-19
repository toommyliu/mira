namespace Mira.Skua.Tests;

public class OptionsTests
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
