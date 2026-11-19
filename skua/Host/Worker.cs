using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json.Nodes;
using Mira.Skua.Protocol;

namespace Mira.Skua;

static partial class Worker
{
    private const int MaxFrame = 16 << 20;
    private const int Channel = 3;
    private const int Interrupted = 4;

    [LibraryImport("libc", EntryPoint = "read", SetLastError = true)]
    private static partial nint ReadChannel(int fd, ref byte buffer, nint count);

    [LibraryImport("libc", EntryPoint = "write", SetLastError = true)]
    private static partial nint WriteChannel(int fd, ref byte buffer, nint count);

    public static int Run()
    {
        var host = new Host();
        host.Send(new JsonObject { ["type"] = "ready", ["protocol"] = 1 });
        host.Listen();
        Environment.Exit(0);
        return 0;
    }

    private sealed class Host
    {
        private static readonly TimeSpan Closing = TimeSpan.FromSeconds(5);

        private readonly object writing = new();
        private readonly ConcurrentDictionary<long, Session> runs = new();
        private long calls;
        private bool closed;

        public long NextCall() => Interlocked.Increment(ref calls);

        public void Send(JsonObject frame)
        {
            var body = Encoding.UTF8.GetBytes(frame.ToJsonString());
            Span<byte> length = stackalloc byte[4];
            BinaryPrimitives.WriteInt32BigEndian(length, body.Length);
            lock (writing)
            {
                if (closed)
                    return;
                try
                {
                    Write(length);
                    Write(body);
                }
                catch (IOException)
                {
                    closed = true;
                }
            }
        }

        public void Listen()
        {
            while (Read() is { } frame)
            {
                var run = frame["run"]?.GetValue<long>() ?? 0;
                switch (frame["type"]?.GetValue<string>())
                {
                    case "start":
                        var started = new Session(this, Parse(frame));
                        if (runs.TryAdd(run, started))
                            started.Begin();
                        break;
                    case "reply" when runs.TryGetValue(run, out var replied):
                        replied.Reply(frame);
                        break;
                    case "stop" when runs.TryGetValue(run, out var stopped):
                        stopped.Stop();
                        break;
                }
            }
            foreach (var run in runs.Values)
                run.Stop();
            SpinWait.SpinUntil(() => runs.IsEmpty, Closing);
        }

        public void Ended(Session run, Outcome outcome)
        {
            runs.TryRemove(run.Id, out _);
            Send(new JsonObject
            {
                ["type"] = "ended",
                ["run"] = run.Id,
                ["outcome"] = new JsonObject { ["kind"] = outcome.Kind, ["message"] = outcome.Message, ["stack"] = outcome.Stack },
            });
        }

        private static Start Parse(JsonObject frame) => new(
            frame["run"]!.GetValue<long>(),
            frame["assembly"]!.GetValue<string>(),
            frame["core"]?.GetValue<string>(),
            frame["class"]!.GetValue<string>(),
            frame["script"]!.GetValue<string>(),
            frame["options"]?.AsObject() is { } options ? (JsonObject)options.DeepClone() : []);

        private JsonObject? Read()
        {
            Span<byte> length = stackalloc byte[4];
            if (!Fill(length))
                return null;
            var size = BinaryPrimitives.ReadInt32BigEndian(length);
            if (size is < 0 or > MaxFrame)
                throw new InvalidDataException($"A frame of {size} bytes is out of range.");
            var body = new byte[size];
            return Fill(body) ? JsonNode.Parse(body)?.AsObject() : null;
        }

        private static void Write(ReadOnlySpan<byte> buffer)
        {
            while (!buffer.IsEmpty)
            {
                var count = WriteChannel(Channel, ref MemoryMarshal.GetReference(buffer), buffer.Length);
                if (count < 0 && Marshal.GetLastPInvokeError() == Interrupted)
                    continue;
                if (count <= 0)
                    throw new IOException($"Couldn't write to Mira (errno {Marshal.GetLastPInvokeError()}).");
                buffer = buffer[(int)count..];
            }
        }

        private bool Fill(Span<byte> buffer)
        {
            var read = 0;
            while (read < buffer.Length)
            {
                var count = ReadChannel(Channel, ref buffer[read], buffer.Length - read);
                if (count < 0 && Marshal.GetLastPInvokeError() == Interrupted)
                    continue;
                if (count <= 0)
                    return false;
                read += (int)count;
            }
            return true;
        }
    }

    private sealed class Session(Host host, Start start) : IConnection
    {
        private readonly CancellationTokenSource stopping = new();
        private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonNode?>> pending = new();
        private readonly object sending = new();
        private bool ended;

        public long Id => start.Run;

        public CancellationToken Stopping => stopping.Token;

        public void Begin() => new Thread(Body) { Name = $"mira-skua-run-{Id}", IsBackground = true }.Start();

        public void Stop() => stopping.Cancel();

        private void Body()
        {
            var context = new RunContext($"run-{Id}", start.Core);
            Outcome outcome;
            try
            {
                outcome = context.Runner().Run(start, this);
            }
            catch (Exception error)
            {
                outcome = Outcome.Failed(error);
            }
            lock (sending)
                ended = true;
            foreach (var call in pending.Values)
                call.TrySetCanceled();
            host.Ended(this, outcome);
            context.Unload();
        }

        public JsonNode? Call(JsonObject request, CancellationToken cancel) => Wait("call", request, cancel);

        public JsonNode? Ask(JsonObject dialog, CancellationToken cancel) => Wait("ask", dialog, cancel);

        public void Log(string level, string text) =>
            host.Send(new JsonObject { ["type"] = "log", ["run"] = Id, ["level"] = level, ["text"] = text });

        public void Reply(JsonObject frame)
        {
            if (!pending.TryRemove(frame["id"]!.GetValue<long>(), out var call))
                return;
            if (frame["error"] is { } error)
                call.TrySetException(new MiraException(error.GetValue<string>()));
            else
                call.TrySetResult(frame["ok"]?.DeepClone());
        }

        private JsonNode? Wait(string type, JsonObject request, CancellationToken cancel)
        {
            var id = host.NextCall();
            var call = new TaskCompletionSource<JsonNode?>(TaskCreationOptions.RunContinuationsAsynchronously);
            lock (sending)
            {
                if (ended)
                    throw new OperationCanceledException();
                cancel.ThrowIfCancellationRequested();
                pending[id] = call;
                host.Send(new JsonObject { ["type"] = type, ["run"] = Id, ["id"] = id, ["request"] = request });
            }
            try
            {
                call.Task.Wait(cancel);
            }
            catch (OperationCanceledException)
            {
                if (pending.TryRemove(id, out _))
                    host.Send(new JsonObject { ["type"] = "cancel", ["run"] = Id, ["id"] = id });
                throw;
            }
            catch (AggregateException failed) when (failed.InnerException is { } inner)
            {
                throw inner is TaskCanceledException ? new OperationCanceledException(cancel) : inner;
            }
            return call.Task.Result;
        }
    }
}
