using System.Text.Json.Nodes;

namespace Mira.Skua.Protocol;

public interface IConnection
{
    CancellationToken Stopping { get; }
    JsonNode? Call(JsonObject request, CancellationToken cancel);
    JsonNode? Ask(JsonObject dialog, CancellationToken cancel);
    void Log(string level, string text);
}

public interface IRunner
{
    Outcome Run(Start start, IConnection connection);
    JsonObject Describe(Start start);
}

public sealed record Start(long Run, string Assembly, string? Core, string Class, string Script, JsonObject Options);

public sealed record Outcome(string Kind, string? Message = null, string? Stack = null)
{
    public static readonly Outcome Completed = new("completed");
    public static readonly Outcome Stopped = new("stopped");

    public static Outcome Failed(Exception error) => new("failed", error.Message, error.StackTrace);
}

public sealed class MiraException(string message) : Exception(message);
