using Skua.Core.Interfaces;
using Skua.Core.Models.Items;
using Skua.Core.Models.Monsters;
using Skua.Core.Models.Players;
using Skua.Core.Models.Skills;

namespace Mira.Skua.Bot.Members;

public sealed class Player(Game game, Paths paths, IFlashUtil flash, Func<IScriptInventory> inventory) : IScriptPlayer
{
    private IScriptPlayer Self => this;

    public string Guild => paths.Get<string>("world.myAvatar.objData.guild.Name", null!);
    public int ID => paths.Get<int>("world.myAvatar.uid");
    public int XP => paths.Get<int>("world.myAvatar.objData.intExp");
    public int RequiredXP => paths.Get<int>("world.myAvatar.objData.intExpToLevel");
    public int AccessLevel
    {
        get => paths.Get<int>("world.myAvatar.objData.intAccessLevel");
        set => paths.Set("world.myAvatar.objData.intAccessLevel", value);
    }
    public string Cell => paths.Get("world.strFrame", string.Empty);
    public string Pad => paths.Get("world.strPad", string.Empty);
    public string ServerIP => paths.Static("serverIP", string.Empty);
    public int State => paths.Get("world.myAvatar.dataLeaf.intState", 0, "world.myAvatar");
    public bool LoggedIn => flash.Call<bool>("isLoggedIn");
    public bool Playing => LoggedIn && Self.Alive;
    public string Username => paths.Static<string>("loginInfo.strUsername", null!);
    public string Password => paths.Static<string>("loginInfo.strPassword", null!);
    public bool Kicked => flash.Call<bool>("isKicked");
    public bool IsMember => game.Request("player", "member")?.GetValue<bool>() ?? false;
    public int Health => paths.Get<int>("world.myAvatar.dataLeaf.intHP");
    public int MaxHealth => paths.Get<int>("world.myAvatar.dataLeaf.intHPMax");
    public int Mana => paths.Get<int>("world.myAvatar.objData.intMP");
    public int MaxMana => paths.Get<int>("world.myAvatar.dataLeaf.intMPMax");
    public int Level => paths.Get<int>("world.myAvatar.dataLeaf.intLevel");
    public int Gold => paths.Get<int>("world.myAvatar.objData.intGold");
    public int Coins => paths.Get<int>("world.myAvatar.objData.intCoins");
    public InventoryItem? CurrentClass => Playing
        ? inventory().Items?.Find(item => item is { Equipped: true, Category: ItemCategory.Class })
        : null;
    public int CurrentClassRank => paths.Get<int>("world.myAvatar.objData.iRank");
    public Monster? Target => flash.Call<Monster>("getTargetMonster") ?? new();
    public bool HasTarget => Target?.Alive ?? false;
    public bool Loaded => paths.Get<int>("world.myAvatar.items.length") > 0
        && !paths.Get<bool>("world.mapLoadInProgress")
        && paths.Call<bool>("world.myAvatar.pMC.artLoaded");
    public SkillInfo[]? Skills => paths.Get("world.actions.active", Array.Empty<SkillInfo>());
    public PlayerStats? Stats => paths.Get<PlayerStats>("world.myAvatar.dataLeaf.sta", null!);
    public bool AFK => paths.Get<bool>("world.myAvatar.dataLeaf.afk");
    public int X => paths.Get<int>("world.myAvatar.pMC.x");
    public int Y => paths.Get<int>("world.myAvatar.pMC.y");
    public int Scale
    {
        get => paths.Get<int>("world.SCALE");
        set => paths.Set("world.SCALE", value);
    }
    public int WalkSpeed
    {
        get => paths.Get("world.WALKSPEED", 8);
        set => paths.Set("world.WALKSPEED", value);
    }

    public void Goto(string name) => paths.Call("world.goto", name);

    public void SetSpawnPoint(string cell, string pad) => game.Request("world", "setSpawnPoint", new { cell, pad });

    public void WalkTo(int x, int y, int speed = 8) => game.Request("player", "walkTo", new { x, y, speed = speed == 8 ? (int?)null : speed });

    public void Rest(bool full = false) => game.Request("player", "rest", new { untilFull = full });
}
