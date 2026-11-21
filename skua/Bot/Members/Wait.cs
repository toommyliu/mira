using Skua.Core.Interfaces;
using Skua.Core.Scripts;

namespace Mira.Skua.Bot.Members;

public sealed class Wait(
    Lazy<IFlashUtil> flash,
    Lazy<IScriptPlayer> player,
    Lazy<IScriptManager> manager,
    Lazy<IScriptMap> map,
    Lazy<IScriptCombat> combat,
    Lazy<IScriptInventory> inventory,
    Lazy<IScriptMonster> monsters,
    Lazy<IScriptHouseInv> house,
    Lazy<IScriptQuest> quests,
    Lazy<IScriptDrop> drops,
    Lazy<IScriptSkill> skills)
    : ScriptWait(flash, player, manager, map, combat, inventory, monsters, house, quests, drops, skills), IScriptWait
{
    public new bool For(Func<object> function, object value, int timeout = 10) => ForTrue(() => Equals(function(), value), timeout);

    public new ValueTask<bool> ForAsync(Func<object> function, object value, int timeout = 10, CancellationToken token = default) =>
        ForTrueAsync(() => Equals(function(), value), timeout, token: token);
}
