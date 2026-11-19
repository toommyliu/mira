using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using Mira.Skua.Bot;
using Mira.Skua.Protocol;

namespace Mira.Skua.Tests;

public class RunnerTests
{
    [Fact]
    public void A_script_starts_after_its_game_events_are_subscribed()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-run-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            File.WriteAllText(Path.Combine(scripts, "Joiner.cs"),
                """public class Joiner { public void ScriptMain(IScriptInterface bot) => bot.Map.Join("battleon"); }""");
            var compiled = Compiler.Compile(scripts, Path.Combine(root.FullName, "cache"), "Joiner.cs");
            var order = new ConcurrentQueue<string>();
            var game = new Offline((key, _) =>
            {
                switch (key)
                {
                    case "skua.events":
                        Thread.Sleep(300);
                        order.Enqueue("subscribed");
                        break;
                    case "player.joinMap":
                        order.Enqueue("joined");
                        break;
                    case "feeds.next":
                        Thread.Sleep(10);
                        break;
                }
                return null;
            });

            var outcome = new Runner().Run(
                new Start(1, compiled["assembly"]!.ToString(), null, compiled["class"]!.ToString(), "Joiner.cs", []), game);

            Assert.Equal("completed", outcome.Kind);
            Assert.Equal(["subscribed", "joined"], order);
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }
}
