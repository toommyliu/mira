using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class Player(Game game, Paths paths, Func<IScriptPlayer> player, Func<IScriptInventory> inventory)
{
    public bool Playing => player().LoggedIn && player().Alive;
    public bool IsMember => game.Request("player", "member")?.GetValue<bool>() ?? false;
    public InventoryItem? CurrentClass => Playing
        ? inventory().Items?.Find(item => item is { Equipped: true, Category: ItemCategory.Class })
        : null;
    public bool HasTarget => player().Target?.Alive ?? false;
    public bool Loaded => paths.Get<int>("world.myAvatar.items.length") > 0
        && !paths.Get<bool>("world.mapLoadInProgress")
        && paths.Call<bool>("world.myAvatar.pMC.artLoaded");

    public void Goto(string name) => game.Request("player", "goToPlayer", new { username = name });

    public void SetSpawnPoint(string cell, string pad) => game.Request("world", "setSpawnPoint", new { cell, pad });

    public void WalkTo(int x, int y, int speed = 8) => game.Request("player", "walkTo", new { x, y, speed = speed == 8 ? (int?)null : speed });

    public void Rest(bool full = false) => game.Request("player", "rest", new { untilFull = full });
}
