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

            var outcome = Run(
                new Start(1, compiled["assembly"]!.ToString(), null, compiled["class"]!.ToString(), "Joiner.cs", []), game);

            Assert.Equal("completed", outcome.Kind);
            Assert.Equal(["subscribed", "joined"], order);
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_run_takes_back_the_drops_quests_and_settings_it_changed_and_leaves_the_players_own()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-run-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            File.WriteAllText(Path.Combine(scripts, "Farmer.cs"), """
                public class Farmer
                {
                    public void ScriptMain(IScriptInterface bot)
                    {
                        bot.Drops.Clear();
                        bot.Drops.Add("Gold Voucher 100k", " player pick ");
                        bot.Drops.Add(5150);
                        bot.Drops.Clear();
                        bot.Drops.Add("Gold Voucher 100k");
                        bot.Drops.Remove("Player Pick", "Never Added");
                        bot.Drops.RejectElse = true;
                        bot.Drops.Start();
                        bot.Options.AcceptAllDrops = true;
                        bot.Quests.RegisterQuests(4242, 1);
                        bot.Options.InfiniteRange = true;
                        bot.Options.CustomName = "Skua user";
                        bot.Options.InfiniteRange = false;
                        bot.Options.InfiniteRange = true;
                        bot.Options.AutoRelogin = true;
                        bot.Options.LagKiller = true;
                        bot.Boosts.UseGoldBoost = true;
                        bot.Boosts.GoldBoostID = 900;
                        bot.Boosts.UseClassBoost = true;
                        bot.Boosts.ClassBoostID = 901;
                        bot.Boosts.Start();
                        bot.Boosts.Stop();
                        bot.Boosts.Start();
                    }
                }
                """);
            var compiled = Compiler.Compile(scripts, Path.Combine(root.FullName, "cache"), "Farmer.cs");
            var registry = new Registry();
            var game = new Offline((key, request) => registry.Answer(key, request));

            var outcome = Run(
                new Start(1, compiled["assembly"]!.ToString(), null, compiled["class"]!.ToString(), "Farmer.cs", []), game);

            Assert.Equal("completed", outcome.Kind);
            Assert.Equal(["Player Pick"], registry.Drops.Select(drop => drop.ToJsonString().Trim('"')));
            Assert.DoesNotContain(game.Calls, call => (string?)call["domain"] == "dropRegistry" && (string?)call["op"] == "unregister" && (call["items"]!.ToJsonString().Contains("Player Pick", StringComparison.OrdinalIgnoreCase) || call["items"]!.ToJsonString().Contains("Never Added")));
            Assert.Contains(game.Calls, call => (string?)call["domain"] == "dropRegistry" && (bool?)call["acceptNonAc"] == true);
            Assert.Equal("""{"enabled":false,"acceptAc":false,"acceptAcMemberOnly":false,"acceptNonAc":false,"acceptNonAcMemberOnly":false,"rejectOthers":false}""", registry.DropConfig.ToJsonString());
            Assert.Equal([(1, 77)], registry.Quests);
            Assert.Equal(["\"Gold Boost! (20 min)\"", """{"itemId":900}"""], registry.Boosts.Select(boost => boost.ToJsonString()));
            Assert.DoesNotContain(game.Calls, call => (string?)call["domain"] == "boostRegistry" && (string?)call["op"] == "unregister" && call["items"]!.ToJsonString().Contains("900"));
            Assert.Contains(game.Calls, call => (string?)call["domain"] == "boostRegistry" && (bool?)call["enabled"] == true);
            Assert.Equal("""{"enabled":false}""", registry.BoostConfig.ToJsonString());
            Assert.Equal("""{"enabled":false}""", registry.QuestConfig.ToJsonString());
            Assert.Equal("""{"infiniteRange":false,"customName":"Hero","rendering":"minimal"}""", registry.Settings.ToJsonString());
            Assert.DoesNotContain(game.Calls, call => (string?)call["op"] == "update" && call.ContainsKey("rendering"));
            Assert.Equal("""{"enabled":false,"server":null}""", registry.Relogin.ToJsonString());
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_run_warns_when_mira_drops_game_events_it_could_not_deliver_in_time()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-run-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            File.WriteAllText(Path.Combine(scripts, "Joiner.cs"),
                """public class Joiner { public void ScriptMain(IScriptInterface bot) => bot.Map.Join("battleon"); }""");
            var compiled = Compiler.Compile(scripts, Path.Combine(root.FullName, "cache"), "Joiner.cs");
            var reads = 0;
            using var caughtUp = new ManualResetEventSlim();
            var game = new Offline((key, _) =>
            {
                switch (key)
                {
                    case "feeds.next" when Interlocked.Increment(ref reads) == 1:
                        return new JsonObject { ["lagged"] = true, ["items"] = new JsonArray() };
                    case "feeds.next":
                        caughtUp.Set();
                        Thread.Sleep(10);
                        break;
                    case "player.joinMap":
                        caughtUp.Wait(TimeSpan.FromSeconds(5));
                        break;
                }
                return null;
            });

            var outcome = Run(
                new Start(1, compiled["assembly"]!.ToString(), null, compiled["class"]!.ToString(), "Joiner.cs", []), game);

            Assert.Equal("completed", outcome.Kind);
            Assert.Contains(game.Logs, log => log.Level == "warn" && log.Text.Contains("fell behind"));
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_stop_callback_that_writes_late_cannot_undo_the_restore()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-run-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            File.WriteAllText(Path.Combine(scripts, "Stopper.cs"), """
                public class Stopper
                {
                    public void ScriptMain(IScriptInterface bot)
                    {
                        bot.Options.CustomName = "Skua user";
                        bot.Events.ScriptStopping += _ =>
                        {
                            Task.Run(() =>
                            {
                                Thread.Sleep(200);
                                bot.Options.CustomName = "Late";
                            });
                            return true;
                        };
                        bot.Stop();
                        bot.Sleep(5000);
                    }
                }
                """);
            var compiled = Compiler.Compile(scripts, Path.Combine(root.FullName, "cache"), "Stopper.cs");
            var registry = new Registry();
            var game = new Offline((key, request) => registry.Answer(key, request));

            var outcome = Run(
                new Start(1, compiled["assembly"]!.ToString(), null, compiled["class"]!.ToString(), "Stopper.cs", []), game);
            Thread.Sleep(600);

            Assert.Equal("stopped", outcome.Kind);
            Assert.Equal("Hero", (string?)registry.Settings["customName"]);
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    private static Outcome Run(Start start, IConnection game)
    {
        var context = new RunContext($"test-run-{start.Run}", start.Core);
        try
        {
            return context.Runner().Run(start, game);
        }
        finally
        {
            context.Unload();
        }
    }

    private sealed class Registry
    {
        public List<JsonNode> Drops { get; } = ["Player Pick"];
        public JsonObject DropConfig { get; } = new()
        {
            ["enabled"] = false,
            ["acceptAc"] = false,
            ["acceptAcMemberOnly"] = false,
            ["acceptNonAc"] = false,
            ["acceptNonAcMemberOnly"] = false,
            ["rejectOthers"] = false,
        };
        public List<(int Id, int? Reward)> Quests { get; } = [(1, 77)];
        public List<JsonNode> Boosts { get; } = ["Gold Boost! (20 min)", new JsonObject { ["itemId"] = 900 }];
        public JsonObject QuestConfig { get; } = new() { ["enabled"] = false };
        public JsonObject BoostConfig { get; } = new() { ["enabled"] = false };
        public JsonObject Settings { get; } = new() { ["infiniteRange"] = false, ["customName"] = "Hero", ["rendering"] = "minimal" };
        public JsonObject Relogin { get; } = new() { ["enabled"] = false, ["server"] = null };

        public JsonNode? Answer(string key, JsonObject request)
        {
            lock (this)
            {
                switch (key)
                {
                    case "dropRegistry.registered":
                        return new JsonArray(Drops.Select(drop => drop.DeepClone()).ToArray());
                    case "dropRegistry.config":
                        return DropConfig.DeepClone();
                    case "dropRegistry.register":
                        foreach (var item in request["items"]!.AsArray())
                            if (!Drops.Any(drop => Same(drop, item)))
                                Drops.Add(item!.DeepClone());
                        break;
                    case "dropRegistry.unregister":
                        Drops.RemoveAll(drop => request["items"]!.AsArray().Any(item => Same(drop, item)));
                        break;
                    case "dropRegistry.unregisterAll":
                        Drops.Clear();
                        break;
                    case "dropRegistry.configure":
                        Configure(DropConfig, request);
                        return DropConfig.DeepClone();
                    case "questRegistry.registered":
                        return new JsonArray(Quests.Select(quest => (JsonNode)new JsonObject { ["questId"] = quest.Id, ["rewardItemId"] = quest.Reward }).ToArray());
                    case "questRegistry.config":
                        return QuestConfig.DeepClone();
                    case "questRegistry.register":
                        foreach (var quest in request["quests"]!.AsArray())
                        {
                            var id = (int)quest!["questId"]!;
                            var reward = (int?)quest["rewardItemId"];
                            var at = Quests.FindIndex(entry => entry.Id == id);
                            if (at < 0)
                                Quests.Add((id, reward));
                            else
                                Quests[at] = (id, reward);
                        }
                        break;
                    case "questRegistry.unregister":
                        Quests.RemoveAll(entry => request["questIds"]!.AsArray().Any(quest => (int)quest! == entry.Id));
                        break;
                    case "questRegistry.unregisterAll":
                        Quests.Clear();
                        break;
                    case "questRegistry.configure":
                        Configure(QuestConfig, request);
                        return QuestConfig.DeepClone();
                    case "boostRegistry.registered":
                        return new JsonArray(Boosts.Select(boost => boost.DeepClone()).ToArray());
                    case "boostRegistry.register":
                        foreach (var item in request["items"]!.AsArray())
                            if (!Boosts.Any(boost => JsonNode.DeepEquals(boost, item)))
                                Boosts.Add(item!.DeepClone());
                        break;
                    case "boostRegistry.config":
                        return BoostConfig.DeepClone();
                    case "boostRegistry.configure":
                        Configure(BoostConfig, request);
                        return BoostConfig.DeepClone();
                    case "boostRegistry.unregister":
                        Boosts.RemoveAll(boost => request["items"]!.AsArray().Any(item => JsonNode.DeepEquals(boost, item)));
                        break;
                    case "settings.get":
                        return Settings.DeepClone();
                    case "settings.update":
                        Configure(Settings, request);
                        return Settings.DeepClone();
                    case "autoRelogin.get":
                        return Relogin.DeepClone();
                    case "autoRelogin.update":
                        Configure(Relogin, request);
                        return Relogin.DeepClone();
                    case "feeds.next":
                        Thread.Sleep(10);
                        break;
                }
                return null;
            }
        }

        private static bool Same(JsonNode? drop, JsonNode? item) =>
            drop is JsonValue name && item is JsonValue other
                ? string.Equals(name.GetValue<string>().Trim(), other.GetValue<string>().Trim(), StringComparison.OrdinalIgnoreCase)
                : JsonNode.DeepEquals(drop, item);

        private static void Configure(JsonObject config, JsonObject request)
        {
            foreach (var (field, value) in request)
                if (field is not ("domain" or "op") && (value is not null || config.ContainsKey(field)))
                    config[field] = value?.DeepClone();
        }
    }
}
