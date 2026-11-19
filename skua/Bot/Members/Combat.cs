using Skua.Core.Interfaces;

namespace Mira.Skua.Bot.Members;

public sealed class Combat(Game game, IFlashUtil flash, Paths paths, Func<IScriptMonster> monsters) : IScriptCombat
{
    public bool EnableCounterHandler { get; set; }
    public bool StopAttacking { get; set; }

    public void ApproachTarget() => paths.Call("world.approachTarget");

    public bool Attack(int id) =>
        monsters().CurrentAvailableMonsters
            .Where(monster => monster.MapID == id || monster.ID == id)
            .OrderBy(monster => monster.HP <= 0).ThenBy(monster => monster.HP).ThenBy(monster => monster.MapID)
            .FirstOrDefault() is { } target
        && game.Request("combat", "attack", new { query = new { monMapId = target.MapID } })?.GetValue<bool>() == true;

    public bool Attack(string name) => game.Request("combat", "attack", new { query = name })?.GetValue<bool>() == true;

    public bool AttackPlayer(string name) => flash.Call<bool>("attackPlayer", name);

    public void CancelAutoAttack() => game.Request("combat", "cancelAutoAttack");

    public void CancelTarget() => game.Request("combat", "cancelTarget");

    public void Exit() => game.Request("combat", "exit");

    public void UntargetSelf() => flash.Call("untargetSelf");
}
