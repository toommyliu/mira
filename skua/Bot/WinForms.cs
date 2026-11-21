namespace System.Windows.Forms;

public enum DialogResult { None, OK, Cancel, Abort, Retry, Ignore, Yes, No }

public enum MessageBoxButtons { OK, OKCancel, AbortRetryIgnore, YesNoCancel, YesNo, RetryCancel }

public enum MessageBoxIcon { None, Error = 16, Hand = 16, Stop = 16, Question = 32, Exclamation = 48, Warning = 48, Asterisk = 64, Information = 64 }

public static class MessageBox
{
    public static DialogResult Show(string text) => Show(text, string.Empty);

    public static DialogResult Show(string text, string caption) => Show(text, caption, MessageBoxButtons.OK);

    public static DialogResult Show(string text, string caption, MessageBoxButtons buttons, MessageBoxIcon icon = MessageBoxIcon.None)
    {
        var dialogs = Mira.Skua.Bot.Dialogs.Current ?? throw new InvalidOperationException("No Skua script is running.");
        switch (buttons)
        {
            case MessageBoxButtons.OK:
                dialogs.Alert(text, caption);
                return DialogResult.OK;
            case MessageBoxButtons.YesNo or MessageBoxButtons.YesNoCancel:
                return dialogs.Confirm(text, caption, "Yes", "No") ? DialogResult.Yes : DialogResult.No;
            default:
                return dialogs.Confirm(text, caption, "OK", "Cancel") ? DialogResult.OK : DialogResult.Cancel;
        }
    }
}
