using Skua.Core.Interfaces;
using Skua.Core.Models;
using Skua.Core.ViewModels;

namespace Mira.Skua.Bot;

sealed class Services(MiraBot bot, Game game, Dialogs dialogs) : IServiceProvider
{
    private readonly LogService logs = new(game);
    private readonly DialogService prompts = new(bot, dialogs);

    public ILogService Logs => logs;

    public object? GetService(Type type) => type switch
    {
        _ when type == typeof(ILogService) => logs,
        _ when type == typeof(IDialogService) => prompts,
        _ when type == typeof(IScriptInterface) => bot,
        _ => throw new NotSupportedException($"Mira doesn't support Skua's {type.Name} yet."),
    };
}

sealed class LogService(Game game) : ILogService
{
    private const int Kept = 500;
    private readonly Queue<string> script = new();

    public void ScriptLog(string message)
    {
        lock (script)
        {
            script.Enqueue(message);
            if (script.Count > Kept)
                script.Dequeue();
        }
        game.Log(message);
    }

    public void DebugLog(string message) => game.Connection.Log("debug", message);

    public void FlashLog(string message) => game.Connection.Log("debug", message);

    public void ClearLog(LogType logType)
    {
        if (logType == LogType.Script)
            lock (script) script.Clear();
    }

    public List<string> GetLogs(LogType logType)
    {
        if (logType != LogType.Script)
            return [];
        lock (script) return [.. script];
    }
}

sealed class DialogService(MiraBot bot, Dialogs dialogs) : IDialogService
{
    public bool? ShowDialog<TViewModel>(TViewModel viewModel) where TViewModel : class => viewModel switch
    {
        InputDialogViewModel input => Ask(input),
        _ => throw new NotSupportedException($"Mira can't show Skua's {typeof(TViewModel).Name}."),
    };

    public bool? ShowDialog<TViewModel>(TViewModel viewModel, string Title) where TViewModel : class => ShowDialog(viewModel);

    public bool? ShowDialog<TViewModel>(TViewModel viewModel, Action<TViewModel> callback) where TViewModel : class
    {
        var answered = ShowDialog(viewModel);
        callback(viewModel);
        return answered;
    }

    public void ShowMessageBox(string message, string caption) => bot.ShowMessageBox(message, caption);

    public bool? ShowMessageBox(string message, string caption, bool yesAndNo) => bot.ShowMessageBox(message, caption, yesAndNo);

    public DialogResult ShowMessageBox(string message, string caption, params string[] buttons) => bot.ShowMessageBox(message, caption, buttons);

    private bool Ask(InputDialogViewModel input)
    {
        var message = string.IsNullOrEmpty(input.DialogHint) ? input.TextBoxHint : input.DialogHint;
        while (true)
        {
            if (dialogs.Prompt(message, input.Title, input.DialogTextInput) is not { } answer)
                return false;
            input.DialogTextInput = answer.Trim();
            if (!input.NumberOnly || long.TryParse(input.DialogTextInput, out _))
                return true;
        }
    }
}
