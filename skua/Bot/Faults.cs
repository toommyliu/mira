using System.Reflection;

namespace Mira.Skua.Bot;

public static class Faults
{
    internal static Action<Exception>? Fail { get; set; }
    internal static Action<string>? Warn { get; set; }

    public static void Report(Exception error)
    {
        if (error is not OperationCanceledException)
            Fail?.Invoke(error);
    }

    public static ThreadStart Guard(ThreadStart start) => () =>
    {
        try
        {
            start();
        }
        catch (Exception error)
        {
            Report(error);
        }
    };

    public static ParameterizedThreadStart Guard(ParameterizedThreadStart start) => state =>
    {
        try
        {
            start(state);
        }
        catch (Exception error)
        {
            Report(error);
        }
    };

    internal static void Invoke(string name, Delegate? handlers, params object?[] args)
    {
        foreach (var handler in handlers?.GetInvocationList() ?? [])
        {
            try
            {
                handler.DynamicInvoke(args);
            }
            catch (Exception error)
            {
                Failed(name, error is TargetInvocationException { InnerException: { } inner } ? inner : error);
            }
        }
    }

    internal static void Failed(string handler, Exception error)
    {
        if (error is not OperationCanceledException)
            Warn?.Invoke($"The script's {handler} handler failed: {error}");
    }
}
