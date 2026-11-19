using System.Xml.Linq;
using Skua.Core.Interfaces;

namespace Mira.Skua.Bot;

public sealed class Flash(Game game) : IFlashUtil
{
    private static readonly HashSet<string> MiraOwned =
    [
        "callGameFunction", "callGameFunction0", "getArrayObject", "getGameObject", "getGameObjectKey",
        "getGameObjectS", "isNull", "selectArrayObjects", "sendClientPacket", "setArrayObject",
        "setGameObject", "setGameObjectKey",
    ];

    public event FlashCallHandler? FlashCall;

    public void Raise(string function, params object[] args) => Faults.Invoke(nameof(FlashCall), FlashCall, function, args);

    public void InitializeFlash() { }

    public string? Call(string function, params object[] args) => (string?)Call(function, typeof(string), args);

    public T? Call<T>(string function, params object[] args) => (T?)Call(function, typeof(T), args);

    public object? Call(string function, Type type, params object[] args) => function switch
    {
        "catchPackets" or "setBackgroundValues" => Json.Convert(null, type),
        "isTrue" => Json.Convert(System.Text.Json.Nodes.JsonValue.Create(Json.Text(Loader("getGameObject", args)) == "true"), type),
        _ => Json.Convert(Loader(function, args), type),
    };

    private System.Text.Json.Nodes.JsonNode? Loader(string function, object[] args) =>
        game.Loader((MiraOwned.Contains(function) ? "flash." : "skua.") + function, args);

    public object FromFlashXml(XElement el) => throw new NotSupportedException("Mira doesn't support Bot.Flash.FromFlashXml.");

    public IFlashObject<T> CreateFlashObject<T>(string path) => throw new NotSupportedException("Mira doesn't support Bot.Flash.CreateFlashObject yet.");

    public void Dispose() { }
}
