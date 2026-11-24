using System.Text.Json.Nodes;
using Skua.Core.Models.Items;

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
    public void A_jump_keeps_its_pad_correction_and_client_only_flags()
    {
        var game = new Offline();
        var bot = game.Bot();

        bot.Map.Jump("r2", "Left");
        bot.Map.Jump("r3", "Right", autoCorrect: false, clientOnly: true);

        Assert.Equal(["""["r2","Left",true,false]""", """["r3","Right",false,true]"""], game.Calls
            .Where(call => (string?)call["function"] == "skua.jumpCorrectRoom")
            .Select(call => call["args"]!.ToJsonString()));
    }

    [Fact]
    public void A_map_item_waits_the_action_delay_before_it_is_sent()
    {
        var clock = new System.Diagnostics.Stopwatch();
        TimeSpan? sent = null;
        var bot = new Offline((key, _) => key == "world.mapItem" && (sent = clock.Elapsed) is { } ? true : null).Bot();
        bot.Options.ActionDelay = 200;

        clock.Start();
        bot.Map.GetMapItem(14397);

        Assert.True(sent >= TimeSpan.FromMilliseconds(200), $"sent after {sent?.TotalMilliseconds} ms");
    }

    [Fact]
    public void Drop_options_decide_what_mira_accepts_and_rejects()
    {
        var config = new JsonObject
        {
            ["enabled"] = false,
            ["acceptAc"] = false,
            ["acceptAcMemberOnly"] = false,
            ["acceptNonAc"] = false,
            ["acceptNonAcMemberOnly"] = false,
            ["rejectOthers"] = false,
        };
        var bot = new Offline((key, request) =>
        {
            if (key == "dropRegistry.configure")
                foreach (var (field, value) in request)
                    if (config.ContainsKey(field))
                        config[field] = value?.DeepClone();
            return null;
        }).Bot();
        string Config() => config.ToJsonString();

        bot.Options.AcceptACDrops = true;
        Assert.Equal("""{"enabled":true,"acceptAc":true,"acceptAcMemberOnly":true,"acceptNonAc":false,"acceptNonAcMemberOnly":false,"rejectOthers":false}""", Config());

        bot.Options.RejectAllDrops = true;
        Assert.Equal("""{"enabled":true,"acceptAc":true,"acceptAcMemberOnly":true,"acceptNonAc":false,"acceptNonAcMemberOnly":false,"rejectOthers":true}""", Config());

        bot.Options.AcceptAllDrops = true;
        Assert.False(bot.Options.RejectAllDrops);
        Assert.Equal("""{"enabled":true,"acceptAc":true,"acceptAcMemberOnly":true,"acceptNonAc":true,"acceptNonAcMemberOnly":true,"rejectOthers":false}""", Config());

        bot.Options.AcceptAllDrops = false;
        bot.Drops.RejectElse = true;
        Assert.Equal("""{"enabled":true,"acceptAc":true,"acceptAcMemberOnly":true,"acceptNonAc":false,"acceptNonAcMemberOnly":false,"rejectOthers":true}""", Config());
    }

    [Fact]
    public void A_boost_is_found_by_its_kind_and_not_by_words_in_its_name()
    {
        static JsonObject Item(int id, string name, string link) =>
            new() { ["ItemID"] = id, ["sName"] = name, ["sType"] = "ServerUse", ["sLink"] = link };
        var items = new JsonArray(
            Item(1, "Golden Gate Ticket", ""),
            Item(2, "Short XP Boost", "xpboost::20"),
            Item(3, "Gold Boost! (20 min)", "gboost::20"),
            Item(4, "Class Boost! (60 min)", "cpboost::60"));
        var bot = new Offline((key, _) => key switch
        {
            "skua.isLoggedIn:" => true,
            "flash.getGameObject:world.myAvatar.items" => items.ToJsonString(),
            _ => null,
        }).Bot();

        Assert.Equal(2, bot.Boosts.GetBoostID(BoostType.Experience, searchBank: false));
        Assert.Equal(3, bot.Boosts.GetBoostID(BoostType.Gold, searchBank: false));
        Assert.Equal(4, bot.Boosts.GetBoostID(BoostType.Class, searchBank: false));
        Assert.Equal(0, bot.Boosts.GetBoostID(BoostType.Reputation, searchBank: false));
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
    public void Setting_the_login_to_the_tabs_own_account_succeeds_and_another_account_is_refused()
    {
        var bot = Offline.Bot(new Dictionary<string, JsonNode?>
        {
            ["flash.getGameObjectS:loginInfo.strUsername"] = "\"Hero\"",
            ["flash.getGameObjectS:loginInfo.strPassword"] = "\"hunter2\"",
        });

        bot.Servers.SetLoginInfo("hero", "hunter2");
        Assert.Throws<NotSupportedException>(() => bot.Servers.SetLoginInfo("Villain", "hunter2"));
        Assert.Throws<NotSupportedException>(() => bot.Servers.SetLoginInfo("Hero", "wrong"));
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
