using System.Text.Json.Nodes;
using Mira.Skua.Bot;
using Newtonsoft.Json.Linq;

namespace Mira.Skua.Tests;

public class EventsTests
{
    [Fact]
    public void A_string_packet_does_not_stop_later_commands_from_arriving()
    {
        var bot = Offline.Bot();

        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"str","dataObj":["chatm","hi"]}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"buyItem","bitSuccess":1,"CharItemID":7}}}"""));

        Assert.True(bot.Wait.ForItemBuy(1));
    }

    [Fact]
    public void Game_packets_raise_the_events_scripts_subscribe_to()
    {
        var bot = Offline.Bot(new Dictionary<string, JsonNode?> { ["flash.getGameObjectS:loginInfo.strUsername"] = "\"Hero\"" });
        var maps = new List<string>();
        var kills = new List<int>();
        var afk = 0;
        bot.Events.MapChanged += maps.Add;
        bot.Events.MonsterKilled += kills.Add;
        bot.Events.PlayerAFK += () => afk++;

        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"moveToArea","strMapName":"battleon"}}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"addGoldExp","typ":"m","id":4}}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"addGoldExp","typ":"q","id":9}}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"str","dataObj":["uotls","-1","someone","afk:true"]}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"str","dataObj":["uotls","-1","hero","afk:true"]}}"""));

        Assert.Equal(["battleon"], maps);
        Assert.Equal([4], kills);
        Assert.Equal(1, afk);
    }

    [Fact]
    public void Every_item_in_a_drop_packet_raises_its_own_event()
    {
        var bot = Offline.Bot();
        var dropped = new List<string>();
        bot.Events.ItemDropped += (item, _, _) => dropped.Add(item.Name);

        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"dropItem","items":{"1":{"ItemID":1,"sName":"Gem"},"2":{"ItemID":2,"sName":"Ore"}}}}}"""));

        Assert.Equal(["Gem", "Ore"], dropped);
    }

    [Fact]
    public void A_sent_packet_reaches_scripts_without_the_debug_length()
    {
        var bot = Offline.Bot();
        var cells = new List<(string Cell, string Pad)>();
        var packets = new List<string>();
        bot.Events.CellChanged += (_, cell, pad) => cells.Add((cell, pad));
        bot.Flash.FlashCall += (function, args) => { if (function == "packet") packets.Add((string)args[0]); };
        void Send(string line) => Runner.Deliver(bot, (Flash)bot.Flash, new JsonObject { ["event"] = "smartFoxDebug", ["line"] = line });

        Send("[Sending - STR]: %xt%zm%moveToCell%1%Enter%Spawn%, (len: 33)");
        Send("[Sending - STR]: %xt%zm%moveToCell%1%Enter%, (len: 27)");

        Assert.Equal([("Enter", "Spawn")], cells);
        Assert.Equal(["%xt%zm%moveToCell%1%Enter%Spawn%", "%xt%zm%moveToCell%1%Enter%"], packets);
    }

    [Fact]
    public void A_malformed_sent_packet_raises_nothing()
    {
        var bot = Offline.Bot();
        var accepted = 0;
        bot.Events.QuestAccepted += _ => accepted++;

        bot.EventSource.Sent("%xt%zm%acceptQuest%1%abc%");
        bot.EventSource.Sent("%xt%zm%acceptQuest%1%%42%");
        bot.EventSource.Sent("%xt%zm%acceptQuest%1%42%");

        Assert.Equal(1, accepted);
    }

    [Fact]
    public void An_afk_packet_reads_the_name_only_for_listeners_and_survives_a_failed_read()
    {
        var game = new Offline((key, _) => key == "flash.getGameObjectS:loginInfo.strUsername"
            ? throw new InvalidOperationException("Mira refused the read.")
            : null);
        var bot = game.Bot();
        var packets = 0;
        bot.Events.ExtensionPacketReceived += _ => packets++;
        var afk = JObject.Parse("""{"params":{"type":"str","dataObj":["uotls","-1","hero","afk:true"]}}""");

        bot.EventSource.Extension(afk);
        Assert.DoesNotContain(game.Calls, call => call["args"]?[0]?.ToString() == "loginInfo.strUsername");

        bot.Events.PlayerAFK += () => { };
        bot.EventSource.Extension(afk);

        Assert.Equal(2, packets);
    }
}
