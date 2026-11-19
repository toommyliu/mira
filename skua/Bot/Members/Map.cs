using Skua.Core.Interfaces;
using Skua.Core.Models.Players;

namespace Mira.Skua.Bot.Members;

public sealed class Map(Game game, Paths paths, Func<IScriptInterface> bot)
{
    public string LastMap { get; set; } = string.Empty;
    public string FilePath { get; set; } = string.Empty;
    public string FileName => string.IsNullOrEmpty(FilePath) ? string.Empty : FilePath.Split('/', '\\').Last();
    public bool Loaded => !bot().Map.Loading && bot().Flash.IsNull("mcConnDetail.stage");
    public string FullName => Loaded
        ? paths.Get<string>("ui.mcInterface.areaList.title.t1.text", null!)?.Split(' ').Last().Replace("\"", string.Empty) ?? string.Empty
        : string.Empty;
    public List<PlayerInfo>? Players => paths.Get<Dictionary<string, PlayerInfo>?>("world.uoTree", null)?.Values.ToList();
    public List<PlayerInfo>? CellPlayers => Players?.FindAll(player => player.Cell == bot().Player.Cell);

    public void Join(string map, string cell = "Enter", string pad = "Spawn", bool ignoreCheck = false, bool autoCorrect = true)
    {
        LastMap = bot().Map.Name;
        game.Request("player", "joinMap", new { map, cell, pad });
    }

    public void JoinPacket(string map, string cell = "Enter", string pad = "Spawn") => Join(map, cell, pad);

    public void Jump(string cell, string pad, bool autoCorrect = true, bool clientOnly = false) =>
        game.Request("player", "jumpToCell", new { cell, pad });

    public void GetMapItem(int id) => game.Request("world", "mapItem", new { itemId = id });

    public PlayerInfo? GetPlayer(string username) =>
        Players?.Find(player => string.Equals(player.Name, username, StringComparison.OrdinalIgnoreCase));
}
