using System.Text.Json.Nodes;
using Skua.Core.Models;

namespace Mira.Skua.Bot;

public sealed class Dialogs(Game game)
{
    public static Dialogs? Current { get; set; }

    public void Alert(string message, string? title) =>
        game.Ask(new JsonObject { ["op"] = "alert", ["message"] = message, ["title"] = Title(title) });

    public bool Confirm(string message, string? title, string confirm, string cancel) =>
        game.Ask(new JsonObject
        {
            ["op"] = "confirm", ["message"] = message, ["title"] = Title(title), ["confirm"] = confirm, ["cancel"] = cancel,
        }) is JsonValue answer && answer.TryGetValue(out bool yes) && yes;

    public string? Prompt(string message, string? title, string? value) =>
        Json.Text(game.Ask(new JsonObject
        {
            ["op"] = "prompt", ["message"] = message, ["title"] = Title(title), ["default"] = value,
        }));

    public DialogResult Choose(string message, string? title, string[] buttons)
    {
        switch (buttons.Length)
        {
            case 0:
                Alert(message, title);
                return new DialogResult("OK", 0);
            case 1:
                Alert(message, title);
                return new DialogResult(buttons[0], 0);
            default:
                var first = Confirm(message, title, buttons[0], buttons[1]);
                return first ? new DialogResult(buttons[0], 0) : new DialogResult(buttons[1], 1);
        }
    }

    private static string? Title(string? title) => string.IsNullOrWhiteSpace(title) ? null : title;
}
