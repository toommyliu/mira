using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json.Nodes;
using Mira.Skua.Protocol;
using Newtonsoft.Json.Linq;
using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Runner : IRunner
{
    private static readonly TimeSpan Cleanup = TimeSpan.FromSeconds(5);

    public JsonObject Describe(Start start)
    {
        var options = new ScriptOptions(start.Options);
        options.Initialize(Activator.CreateInstance(Load(start).Type)!);
        return options.Describe();
    }

    public Outcome Run(Start start, IConnection connection)
    {
        using var run = CancellationTokenSource.CreateLinkedTokenSource(connection.Stopping);
        var game = new Game(connection, run.Token);
        var dialogs = Dialogs.Current = new Dialogs(game);
        var options = new ScriptOptions(start.Options);
        var status = new Status(start.Script, start.Assembly, run) { Config = options };
        var bot = new MiraBot(game, status, options, dialogs);
        CommunityToolkit.Mvvm.DependencyInjection.Ioc.Default.ConfigureServices(bot.Services);
        Exception? failure = null;
        Faults.Fail = error =>
        {
            if (run.IsCancellationRequested)
                return;
            Interlocked.CompareExchange(ref failure, error, null);
            run.Cancel();
        };
        Faults.Warn = game.Warn;
        Thread? events = null;
        var drops = (Members.Drops)bot.Drops;
        var quests = (Members.Quests)bot.Quests;
        var settings = (Members.Options)bot.Options;
        var boosts = (Members.Boosts)bot.Boosts;
        var handlers = Start(() => bot.HandlerSource.Run(bot, run.Token));
        try
        {
            game.Request("skua", "events", new { id = 1 }, game.Stopping);
            drops.Remember();
            quests.Remember();
            boosts.Remember();
            events = Start(() => Listen(bot, game));
            var (type, main) = Load(start);
            var script = Activator.CreateInstance(type)!;
            options.Initialize(script);
            var result = main.Invoke(script, BindingFlags.DoNotWrapExceptions, null, [bot], null);
            if (result is Task task)
                task.GetAwaiter().GetResult();
        }
        catch (OperationCanceledException) when (run.IsCancellationRequested)
        {
        }
        catch (Exception error)
        {
            Interlocked.CompareExchange(ref failure, error, null);
        }
        var stopped = run.IsCancellationRequested && failure is null;
        StopSkills(bot);
        run.Cancel();
        events?.Join(Cleanup);
        handlers.Join(Cleanup);
        if (failure is not null || (stopped && status.RunStopping))
        {
            using var cleanup = new CancellationTokenSource(Cleanup);
            game.Calls = cleanup.Token;
            bot.EventSource.Stopping(failure);
            cleanup.Cancel();
        }
        using (var teardown = new CancellationTokenSource(Cleanup))
        {
            try
            {
                drops.Restore(teardown.Token);
                quests.Restore(teardown.Token);
                settings.Restore(teardown.Token);
                boosts.Restore(teardown.Token);
            }
            catch (Exception error)
            {
                if (error is not OperationCanceledException)
                    game.Warn($"Mira couldn't undo this script's drop, quest, boost and setting changes: {error.Message}");
            }
        }
        bot.Detach();
        Dialogs.Current = null;
        Faults.Fail = null;
        Faults.Warn = null;
        return failure is not null ? Outcome.Failed(failure) : stopped ? Outcome.Stopped : Outcome.Completed;
    }

    private static void StopSkills(MiraBot bot)
    {
        bot.Skills.Resume();
        try
        {
            bot.Skills.Stop();
        }
        catch (OperationCanceledException)
        {
        }
    }

    private static Thread Start(Action body)
    {
        var thread = new Thread(() =>
        {
            try
            {
                body();
            }
            catch (Exception error)
            {
                Faults.Report(error);
            }
        }) { IsBackground = true };
        thread.Start();
        return thread;
    }

    private static void Listen(MiraBot bot, Game game)
    {
        var flash = (Flash)bot.Flash;
        while (!game.Stopping.IsCancellationRequested)
        {
            var next = game.Request("feeds", "next", new { id = 1 }, game.Stopping);
            if (next?["lagged"]?.GetValue<bool>() == true)
                game.Warn("Mira fell behind on game packets and dropped some, so this script may miss one it's waiting for.");
            foreach (var item in next?["items"]?.AsArray() ?? [])
                Deliver(bot, flash, item!.AsObject());
        }
    }

    internal static void Deliver(MiraBot bot, Flash flash, JsonObject item)
    {
        switch (Json.Text(item["event"]))
        {
            case "extensionResponse":
                var packet = new JObject { ["params"] = JToken.Parse(Json.Text(item["packet"]) ?? "null") };
                flash.Raise("pext", packet.ToString(Newtonsoft.Json.Formatting.None));
                bot.EventSource.Extension(packet);
                break;
            case "smartFoxDebug":
                var line = Json.Text(item["line"]) ?? string.Empty;
                const string sending = "[Sending - STR]: ";
                if (line.StartsWith(sending + "%xt%zm%", StringComparison.Ordinal))
                {
                    var sent = WithoutLength(line[sending.Length..].Trim());
                    flash.Raise("packet", sent);
                    bot.EventSource.Sent(sent);
                }
                else
                    flash.Raise("packetFromServer", Received(line));
                break;
        }
    }

    private static string WithoutLength(string text)
    {
        var suffix = text.LastIndexOf(", (len: ", StringComparison.Ordinal);
        if (suffix < 0 || !text.EndsWith(')'))
            return text;
        var digits = text[(suffix + ", (len: ".Length)..^1];
        return digits.Length > 0 && digits.All(char.IsAsciiDigit) ? text[..suffix].TrimEnd() : text;
    }

    private static string Received(string line)
    {
        line = line.Replace("[Sending - STR]: ", "").Replace("[ RECEIVED ]: ", "").Replace("[Sending]: ", "");
        var length = line.IndexOf(", (len: ", StringComparison.Ordinal);
        return length > -1 ? line[..length] : line;
    }

    private static (Type Type, MethodInfo Main) Load(Start start)
    {
        var context = AssemblyLoadContext.GetLoadContext(typeof(Runner).Assembly)!;
        var assembly = context.LoadFromAssemblyPath(start.Assembly);
        var type = assembly.GetType(start.Class, throwOnError: true)!;
        var main = type.GetMethod("ScriptMain", [typeof(IScriptInterface)])
            ?? throw new MissingMethodException(start.Class, "ScriptMain(IScriptInterface)");
        return (type, main);
    }
}
