using System.Collections.Concurrent;
using System.Xml.Linq;
using Skua.Core.Flash;
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

    private readonly ConcurrentDictionary<int, string> links = new();
    private int lastLink;

    public event FlashCallHandler? FlashCall;

    public void Raise(string function, params object[] args) => Faults.Invoke(nameof(FlashCall), FlashCall, function, args);

    public void InitializeFlash() { }

    public string? Call(string function, params object[] args) => (string?)Call(function, typeof(string), args);

    public T? Call<T>(string function, params object[] args) => (T?)Call(function, typeof(T), args);

    public object? Call(string function, Type type, params object[] args) => function switch
    {
        "catchPackets" or "setBackgroundValues" => Json.Convert(null, type),
        "isTrue" => Json.Convert(System.Text.Json.Nodes.JsonValue.Create(Json.Text(Loader("getGameObject", args)) == "true"), type),
        ['l', 'n', 'k', ..] => Json.Convert(System.Text.Json.Nodes.JsonValue.Create(Link(function, args)), type),
        "canUseSkill" when args.Length == 1 => Json.Convert(game.Request("combat", "canUseSkill", new { skill = args[0] }), type),
        "useSkill" => Json.Convert(game.Request("combat", "useSkill", new { skill = args[0] }), type),
        "skillCooldownRemaining" => Json.Convert(System.Text.Json.Nodes.JsonValue.Create(
            (int)(game.Request("combat", "skillCooldown", new { skill = args[0] })?.GetValue<double>() ?? 0)), type),
        _ => Json.Convert(Loader(function, args), type),
    };

    public IFlashObject<T> CreateFlashObject<T>(string path) => new FlashObject<T>(Call<int>("lnkCreate", path), this);

    private object? Link(string function, object[] args)
    {
        string Path(int arg) => links.TryGetValue(Convert.ToInt32(args[arg]), out var path)
            ? path
            : throw new InvalidOperationException($"Flash object {args[arg]} was already destroyed.");
        switch (function)
        {
            case "lnkCreate":
                return Open((string)args[0]);
            case "lnkGetChild":
                return Open($"{Path(0)}.{args[1]}");
            case "lnkGetArray":
                return Open($"{Path(0)}.{args[1]}");
            case "lnkGetValue":
                return ((IFlashUtil)this).GetGameObject(Path(0));
            case "lnkSetValue":
                ((IFlashUtil)this).SetGameObject(Path(0), args[1]);
                return null;
            case "lnkSetArray":
                ((IFlashUtil)this).SetGameObject($"{Path(0)}.{args[1]}", args[2]);
                return null;
            case "lnkCall":
                return ((IFlashUtil)this).CallGameFunction(Path(0), args[1..]);
            case "lnkDestroy":
                links.TryRemove(Convert.ToInt32(args[0]), out _);
                return null;
            default:
                throw new NotSupportedException($"Mira doesn't support Bot.Flash.Call(\"{function}\") yet.");
        }
    }

    private int Open(string path)
    {
        var id = Interlocked.Increment(ref lastLink);
        links[id] = path;
        return id;
    }

    private System.Text.Json.Nodes.JsonNode? Loader(string function, object[] args) =>
        game.Loader((MiraOwned.Contains(function) ? "flash." : "skua.") + function, args);

    public object FromFlashXml(XElement el) => throw Unsupported.Member("Flash.FromFlashXml");

    public void Dispose() { }
}
