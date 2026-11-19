using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using Mira.Skua.Bot;
using Mira.Skua.Protocol;

namespace Mira.Skua.Tests;

sealed class Offline(Func<string, JsonObject, JsonNode?> answer) : IConnection
{
    public Offline(Dictionary<string, JsonNode?>? answers = null)
        : this((key, _) => answers?.GetValueOrDefault(key)?.DeepClone())
    {
    }

    public ConcurrentQueue<JsonObject> Calls { get; } = new();

    public CancellationToken Stopping => CancellationToken.None;

    public JsonNode? Call(JsonObject request, CancellationToken cancel)
    {
        cancel.ThrowIfCancellationRequested();
        Calls.Enqueue(request);
        var key = request["function"] is { } function
            ? $"{function}:{request["args"]?.AsArray().FirstOrDefault()}"
            : $"{request["domain"]}.{request["op"]}";
        return answer(key, request);
    }

    public JsonNode? Ask(JsonObject dialog, CancellationToken cancel) => null;

    public void Log(string level, string text) { }

    public MiraBot Bot()
    {
        var run = new CancellationTokenSource();
        var game = new Game(this, run.Token);
        return new MiraBot(game, new Status("Test.cs", "Test.dll", run), new ScriptOptions([]), new Dialogs(game));
    }

    public static MiraBot Bot(Dictionary<string, JsonNode?>? answers = null) => new Offline(answers).Bot();
}
