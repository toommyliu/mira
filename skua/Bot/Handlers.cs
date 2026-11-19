using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Handlers : IScriptHandlers
{
    private const int Tick = 20;
    private readonly object gate = new();
    private readonly List<(Handler Handler, long Due)> handlers = [];

    public IEnumerable<IHandler> CurrentHandlers
    {
        get { lock (gate) return handlers.Select(entry => (IHandler)entry.Handler).ToList(); }
    }

    public IHandler RegisterHandler(int ticks, Func<IScriptInterface, bool> function, string name = null!)
    {
        var handler = new Handler(function, name ?? Guid.NewGuid().ToString(), ticks);
        lock (gate) handlers.Add((handler, Environment.TickCount64 + ticks * Tick));
        return handler;
    }

    public bool Remove(string name)
    {
        lock (gate) return handlers.RemoveAll(entry => entry.Handler.Name == name) > 0;
    }

    public bool Remove(IHandler handler)
    {
        lock (gate) return handlers.RemoveAll(entry => ReferenceEquals(entry.Handler, handler)) > 0;
    }

    public bool Remove(List<IHandler> removed)
    {
        lock (gate) return handlers.RemoveAll(entry => removed.Contains(entry.Handler)) > 0;
    }

    public void Clear()
    {
        lock (gate) handlers.Clear();
    }

    public void Run(IScriptInterface bot, CancellationToken stopping)
    {
        while (!stopping.WaitHandle.WaitOne(Tick))
        {
            List<Handler> due;
            var now = Environment.TickCount64;
            lock (gate)
            {
                due = handlers.Where(entry => entry.Due <= now).Select(entry => entry.Handler).ToList();
                for (int index = 0; index < handlers.Count; index++)
                    if (handlers[index].Due <= now)
                        handlers[index] = (handlers[index].Handler, now + handlers[index].Handler.Ticks * Tick);
            }
            foreach (var handler in due)
            {
                try
                {
                    if (!handler.Function(bot))
                        Remove(handler);
                }
                catch (Exception error)
                {
                    Faults.Failed(handler.Name, error);
                }
            }
        }
    }

    private sealed record Handler(Func<IScriptInterface, bool> Function, string Name, int Ticks) : IHandler;
}
