using Skua.Core.Interfaces;
using Skua.Core.Scripts;

namespace Mira.Skua.Bot.Members;

public sealed class Hunt(
    Game game,
    Func<IScriptInterface> bot,
    Lazy<IScriptOption> options,
    Lazy<IScriptCombat> combat,
    Lazy<IScriptPlayer> player,
    Lazy<IScriptMonster> monsters,
    Lazy<IScriptDrop> drops,
    Lazy<IScriptInventory> inventory,
    Lazy<IScriptTempInv> tempInv,
    Lazy<IScriptManager> manager,
    Lazy<IScriptMap> map,
    Lazy<IScriptKill> kill,
    ILogService logger)
    : ScriptHunt(options, combat, player, monsters, drops, inventory, tempInv, manager, map, kill, logger), IScriptHunt
{
    public new void Monster(int id) => Monster(id, null);

    public new void Monster(int id, CancellationToken? token)
    {
        var cancel = token ?? CancellationToken.None;
        if (cancel.IsCancellationRequested)
            return;
        if (!bot().Monsters.MapMonsters.Exists(monster => monster.MapID == id))
        {
            base.Monster(id, token);
            return;
        }
        using var calls = CancellationTokenSource.CreateLinkedTokenSource(game.Calls, cancel);
        try
        {
            game.Request("combat", "hunt", new { query = new { monMapId = id } }, calls.Token);
        }
        catch (OperationCanceledException) when (cancel.IsCancellationRequested)
        {
            return;
        }
        if (!cancel.IsCancellationRequested)
            bot().Kill.Monster(id, token);
    }
}
