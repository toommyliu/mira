using Skua.Core.Interfaces;
using Skua.Core.Models.Items;
using Skua.Core.Models.Players;

namespace Mira.Skua.Bot.Members;

public sealed class Map(Game game, Paths paths, IFlashUtil flash, IScriptOption options, Func<IScriptPlayer> player, Func<IScriptWait> wait) : IScriptMap
{
    public string LastMap { get; set; } = string.Empty;
    public bool Loading => paths.Get("world.mapLoadInProgress", false);
    public string FilePath { get; set; } = string.Empty;
    public string FileName => string.IsNullOrEmpty(FilePath) ? string.Empty : FilePath.Split('/', '\\').Last();
    public string Name => paths.Get("world.strMapName", string.Empty, "world");
    public string FullName => Loaded
        ? paths.Get<string>("ui.mcInterface.areaList.title.t1.text", null!)?.Split(' ').Last().Replace("\"", string.Empty) ?? string.Empty
        : string.Empty;
    public bool Loaded => !Loading && flash.IsNull("mcConnDetail.stage");
    public int RoomID => paths.Get<int>("world.curRoom");
    public int PlayerCount => paths.Get<int>("world.areaUsers.length");
    public List<string> Cells => flash.SelectArrayObjects<string>("world.map.currentScene.labels", "name");
    public List<string>? PlayerNames => paths.Get<List<string>>("world.areaUsers", []);
    public List<PlayerInfo>? Players => paths.Get<Dictionary<string, PlayerInfo>?>("world.uoTree", null)?.Values.ToList();
    public List<PlayerInfo>? CellPlayers => Players?.FindAll(other => other.Cell == player().Cell);

    public void Reload() => paths.Call("world.reloadCurrentMap");

    public void Join(string map, string cell = "Enter", string pad = "Spawn", bool ignoreCheck = false, bool autoCorrect = true)
    {
        LastMap = Name;
        if ((options.PrivateRooms && !map.Contains('-')) || map.Contains("-1e9"))
            map = $"{map.Split('-')[0]}-{(options.PrivateNumber > 0 ? options.PrivateNumber : 100000)}";
        game.Request("player", "joinMap", new { map, cell, pad, rooms = new { kind = "public" } });
    }

    public void JoinPacket(string map, string cell = "Enter", string pad = "Spawn") => Join(map, cell, pad);

    public void Jump(string cell, string pad, bool autoCorrect = true, bool clientOnly = false)
    {
        flash.Call("jumpCorrectRoom", cell, pad, autoCorrect, clientOnly);
        wait().ForCellChange(cell);
    }

    public void GetMapItem(int id) => game.Request("world", "mapItem", new { itemId = id });

    public PlayerInfo? GetPlayer(string username) =>
        Players?.Find(other => string.Equals(other.Name, username, StringComparison.OrdinalIgnoreCase));

    public List<MapItem>? FindMapItems(bool forceRefresh = false) => throw Unsupported.Member("Map.FindMapItems");

    public void ClearMapItemsCache(string? specificMap = null) => throw Unsupported.Member("Map.ClearMapItemsCache");
}
