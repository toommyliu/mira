using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Status(string script, string assembly, CancellationTokenSource run) : IScriptManager
{
    public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged { add { } remove { } }

    public bool ShouldExit => run.IsCancellationRequested;
    public bool ScriptRunning => !run.IsCancellationRequested;
    public string LoadedScript { get; private set; } = script;
    public string CompiledScript => assembly;
    public IScriptOptionContainer? Config { get; set; }
    public bool RunStopping { get; private set; } = true;
    public CancellationTokenSource? ScriptCts => run;

    public object? Compile(string source) => throw Unsupported.Member("Manager.Compile");

    public void LoadScriptConfig(object? script) => throw Unsupported.Member("Manager.LoadScriptConfig");

    public Task RestartScriptAsync() => throw new NotSupportedException("Restart Skua scripts from Mira.");

    public Task<Exception?> StartScript() => Task.FromResult<Exception?>(new NotSupportedException("Start Skua scripts from Mira."));

    public void SetLoadedScript(string path) => LoadedScript = path;

    public ValueTask StopScript(bool runScriptStoppingEvent = true)
    {
        RunStopping = runScriptStoppingEvent;
        run.Cancel();
        return ValueTask.CompletedTask;
    }
}
