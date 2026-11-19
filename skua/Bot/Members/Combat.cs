using Skua.Core.Interfaces;

namespace Mira.Skua.Bot.Members;

public sealed class Combat(Game game, IFlashUtil flash)
{
    public bool EnableCounterHandler { get; set; }
    public bool StopAttacking { get; set; }

    public bool Attack(int id) => game.Request("combat", "attack", new { query = new { monMapId = id } })?.GetValue<bool>() == true;

    public bool Attack(string name) => game.Request("combat", "attack", new { query = name })?.GetValue<bool>() == true;

    public bool AttackPlayer(string name) => flash.Call<bool>("attackPlayer", name);

    public void CancelAutoAttack() => game.Request("combat", "cancelAutoAttack");

    public void CancelTarget() => game.Request("combat", "cancelTarget");

    public void Exit() => game.Request("combat", "exit");
}
