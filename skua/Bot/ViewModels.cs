namespace Skua.Core.ViewModels;

public class InputDialogViewModel(string title, string dialogHint, string textBoxHint, bool numericInputOnly)
{
    public InputDialogViewModel(string title)
        : this(title, string.Empty, "Quantity", true)
    {
    }

    public InputDialogViewModel(string title, string dialogHint)
        : this(title, dialogHint, "Quantity", true)
    {
    }

    public InputDialogViewModel(string title, string dialogHint, string textBoxHint)
        : this(title, dialogHint, textBoxHint, true)
    {
    }

    public InputDialogViewModel(string title, bool numericInputOnly)
        : this(title, string.Empty, string.Empty, numericInputOnly)
    {
    }

    public InputDialogViewModel(string title, string dialogHint, bool numericInputOnly)
        : this(title, dialogHint, string.Empty, numericInputOnly)
    {
    }

    public string Title { get; } = title;
    public bool NumberOnly { get; } = numericInputOnly;
    public string DialogHint { get; } = dialogHint;
    public string TextBoxHint { get; } = textBoxHint;
    public string DialogTextInput { get; set; } = string.Empty;
}
