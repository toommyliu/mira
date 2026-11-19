using System.Text.Json.Nodes;
using Newtonsoft.Json.Linq;

namespace Mira.Skua.Tests;

public class EventsTests
{
    [Fact]
    public void A_string_packet_does_not_stop_later_commands_from_arriving()
    {
        var bot = Offline.Bot();

        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"str","dataObj":["chatm","hi"]}}"""));
        bot.EventSource.Extension(JObject.Parse("""{"params":{"type":"json","dataObj":{"cmd":"buyItem"}}}"""));

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
